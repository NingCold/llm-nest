use super::{SocialStorageError as Error, SocialStorageResult as Result};
use rusqlite::{Connection, backup::StepResult};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    time::{Duration, Instant},
};

const SCHEMA: &str = include_str!("schema.sql");
const SCHEMA_V2: &str = include_str!("schema-v2.sql");
const SCHEMA_V3: &str = include_str!("schema-v3.sql");
const SCHEMA_V4: &str = include_str!("schema-v4.sql");
const SCHEMA_V5: &str = include_str!("schema-v5.sql");
const SCHEMA_V6: &str = include_str!("schema-v6.sql");
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn migration_digest(script: &str) -> String {
    // Git checkout line endings must not invalidate a database across platforms.
    digest(script.replace("\r\n", "\n").as_bytes())
}
pub(super) fn sql_error(path: &Path, source: rusqlite::Error) -> Error {
    Error::Sqlite {
        path: path.to_owned(),
        source,
    }
}

pub(super) fn backup(connection: &Connection, destination: &Path) -> Result<()> {
    // Reserve a new destination: never overwrite a caller's file or our source.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|source| Error::Io {
            path: destination.to_owned(),
            source,
        })?;
    drop(file);
    let result = (|| {
        let mut target = Connection::open(destination).map_err(|e| sql_error(destination, e))?;
        let backup = rusqlite::backup::Backup::new(connection, &mut target)
            .map_err(|e| sql_error(destination, e))?;
        let started = Instant::now();
        loop {
            match backup.step(64).map_err(|e| sql_error(destination, e))? {
                StepResult::Done => return Ok(()),
                StepResult::More => {}
                StepResult::Busy | StepResult::Locked => {
                    if started.elapsed() > Duration::from_secs(2) {
                        return Err(sql_error(
                            destination,
                            rusqlite::Error::SqliteFailure(
                                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                                Some("backup remained busy".into()),
                            ),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => return Err(Error::Invalid("unknown backup step result".into())),
            }
        }
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(destination);
    }
    result
}

pub(super) fn open(path: &Path, now: &str) -> Result<Connection> {
    let mut connection = Connection::open(path).map_err(|e| sql_error(path, e))?;
    connection
        .busy_timeout(Duration::from_millis(500))
        .map_err(|e| sql_error(path, e))?;
    // Inspect before changing journal settings or attempting a write.
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| sql_error(path, e))?;
    if !(0..=6).contains(&version) {
        return Err(Error::Schema {
            path: path.to_owned(),
            detail: format!("unsupported version {version}; expected 0..=6"),
        });
    }
    let tables: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| sql_error(path, e))?;
    if version == 0 && tables != 0 {
        return Err(Error::Schema {
            path: path.to_owned(),
            detail: "unversioned nonempty database".into(),
        });
    }
    if version > 0 {
        let mut statement = connection
            .prepare("SELECT version,checksum FROM schema_migrations ORDER BY version")
            .map_err(|e| sql_error(path, e))?;
        let ledger = statement
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| sql_error(path, e))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| sql_error(path, e))?;
        let expected = [
            (1, migration_digest(SCHEMA)),
            (2, migration_digest(SCHEMA_V2)),
            (3, migration_digest(SCHEMA_V3)),
            (4, migration_digest(SCHEMA_V4)),
            (5, migration_digest(SCHEMA_V5)),
            (6, migration_digest(SCHEMA_V6)),
        ];
        if ledger != expected[..version as usize] {
            return Err(Error::Schema {
                path: path.to_owned(),
                detail: "migration checksum mismatch".into(),
            });
        }
    } else {
        // Even an existing empty v0 database gets a SQLite-consistent backup.
        let backup_path = path.with_file_name(format!(
            "social.before-v1-{}.sqlite3",
            ::social::CommandId::new()
        ));
        backup(&connection, &backup_path)?;
        apply_initial_migration(&mut connection, path, now, SCHEMA)?;
    }
    for (next, script) in [
        (2, SCHEMA_V2),
        (3, SCHEMA_V3),
        (4, SCHEMA_V4),
        (5, SCHEMA_V5),
        (6, SCHEMA_V6),
    ] {
        if version >= next {
            continue;
        }
        let backup_path = path.with_file_name(format!(
            "social.before-v{next}-{}.sqlite3",
            ::social::CommandId::new()
        ));
        backup(&connection, &backup_path)?;
        let transaction = connection.transaction().map_err(|e| sql_error(path, e))?;
        transaction
            .execute_batch(script)
            .map_err(|e| sql_error(path, e))?;
        transaction
            .execute(
                "INSERT INTO schema_migrations VALUES(?1,?2,?3)",
                rusqlite::params![next, migration_digest(script), now],
            )
            .map_err(|e| sql_error(path, e))?;
        transaction
            .pragma_update(None, "user_version", next)
            .map_err(|e| sql_error(path, e))?;
        transaction.commit().map_err(|e| sql_error(path, e))?;
    }
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| sql_error(path, e))?;
    let mode: String = connection
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .map_err(|e| sql_error(path, e))?;
    if mode.to_lowercase() != "wal" {
        return Err(Error::Schema {
            path: path.to_owned(),
            detail: format!("WAL unavailable: {mode}"),
        });
    }
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|e| sql_error(path, e))?;
    Ok(connection)
}

fn apply_initial_migration(
    connection: &mut Connection,
    path: &Path,
    now: &str,
    script: &str,
) -> Result<()> {
    let transaction = connection.transaction().map_err(|e| sql_error(path, e))?;
    transaction
        .execute_batch(script)
        .map_err(|e| sql_error(path, e))?;
    transaction
        .execute(
            "INSERT INTO schema_migrations VALUES(1,?1,?2)",
            rusqlite::params![migration_digest(script), now],
        )
        .map_err(|e| sql_error(path, e))?;
    transaction
        .pragma_update(None, "user_version", 1)
        .map_err(|e| sql_error(path, e))?;
    transaction.commit().map_err(|e| sql_error(path, e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v1_migration_preserves_data_and_creates_a_consistent_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("social.sqlite3");
        let mut old = Connection::open(&path).unwrap();
        apply_initial_migration(&mut old, &path, "test", SCHEMA).unwrap();
        let id = ::social::ActorId::new().to_string();
        let payload=serde_json::json!({"id":id,"kind":"human","display_name":"synthetic","avatar_ref":null,"status":"active","revision":"1","created_at":"2026-10-09T00:00:00Z"}).to_string();
        old.execute(
            "INSERT INTO actors VALUES(?1,'human','active',1,?2)",
            rusqlite::params![id, payload],
        )
        .unwrap();
        drop(old);
        let current = open(&path, "test-v4").unwrap();
        assert_eq!(
            current
                .pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            6
        );
        let migrated: serde_json::Value = serde_json::from_str(
            &current
                .query_row::<String, _, _>("SELECT payload FROM actors WHERE id=?1", [&id], |r| {
                    r.get(0)
                })
                .unwrap(),
        )
        .unwrap();
        let mut expected: serde_json::Value = serde_json::from_str(&payload).unwrap();
        expected["bio"] = serde_json::json!("");
        assert_eq!(migrated, expected);
        assert_eq!(
            current
                .query_row::<i64, _, _>("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))
                .unwrap(),
            6
        );
        let backup_path = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("social.before-v2-")
            })
            .unwrap();
        let backup = Connection::open(backup_path).unwrap();
        assert_eq!(
            backup
                .pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            1
        );
        assert_eq!(
            backup
                .query_row::<String, _, _>("SELECT payload FROM actors WHERE id=?1", [&id], |r| r
                    .get(0))
                .unwrap(),
            payload
        );
        let v2_backup_path = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("social.before-v3-")
            })
            .unwrap();
        let v2_backup = Connection::open(v2_backup_path).unwrap();
        assert_eq!(
            v2_backup
                .pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            2
        );
        assert_eq!(
            v2_backup
                .query_row::<String, _, _>("SELECT payload FROM actors WHERE id=?1", [&id], |r| r
                    .get(0))
                .unwrap(),
            payload
        );
    }
    #[test]
    fn v3_migration_removes_history_limits_and_preserves_member_versions_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("social.sqlite3");
        let mut old = Connection::open(&path).unwrap();
        apply_initial_migration(&mut old, &path, "v1", SCHEMA).unwrap();
        for (version, script) in [(2, SCHEMA_V2), (3, SCHEMA_V3)] {
            old.execute_batch(script).unwrap();
            old.execute(
                "INSERT INTO schema_migrations VALUES(?1,?2,'fixture')",
                rusqlite::params![version, migration_digest(script)],
            )
            .unwrap();
            old.pragma_update(None, "user_version", version).unwrap();
        }
        let actor = ::social::ActorId::new();
        let conversation = ::social::ConversationId::new();
        old.execute(
            "INSERT INTO actors VALUES(?1,'human','active',1,?2)",
            rusqlite::params![
                actor.to_string(),
                serde_json::json!({"id":actor}).to_string()
            ],
        )
        .unwrap();
        old.execute(
            "INSERT INTO conversations VALUES(?1,'group',NULL,'active',0,1,1,1,0,?2)",
            rusqlite::params![
                conversation.to_string(),
                serde_json::json!({"id":conversation}).to_string()
            ],
        )
        .unwrap();
        let member=serde_json::json!({"conversation_id":conversation,"actor_id":actor,"status":"active","history_from_seq":"75","revision":"3"}).to_string();
        old.execute(
            "INSERT INTO memberships VALUES(?1,?2,'active',75,3,?3)",
            rusqlite::params![conversation.to_string(), actor.to_string(), member],
        )
        .unwrap();
        drop(old);
        let current = open(&path, "v4").unwrap();
        let payload: String = current
            .query_row("SELECT payload FROM memberships", [], |r| r.get(0))
            .unwrap();
        let migrated: ::social::Membership = serde_json::from_str(&payload).unwrap();
        assert_eq!(migrated.revision.get(), 3);
        assert_eq!(migrated.status, ::social::MembershipStatus::Active);
        assert!(
            serde_json::from_str::<serde_json::Value>(&payload)
                .unwrap()
                .get("history_from_seq")
                .is_none()
        );
        assert!(
            current
                .prepare("SELECT history_from_seq FROM memberships")
                .is_err()
        );
        assert_eq!(
            current
                .query_row::<i64, _, _>("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                    .get(0))
                .unwrap(),
            0
        );
        let backup_path = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("social.before-v4-")
            })
            .unwrap();
        let backup = Connection::open(backup_path).unwrap();
        assert_eq!(
            backup
                .pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            3
        );
        assert_eq!(
            backup
                .query_row::<String, _, _>("SELECT payload FROM memberships", [], |r| r.get(0))
                .unwrap(),
            member
        );
        drop(current);
        let reopened = open(&path, "again").unwrap();
        assert_eq!(
            reopened
                .query_row::<String, _, _>("SELECT payload FROM memberships", [], |r| r.get(0))
                .unwrap(),
            payload
        );
    }
    #[test]
    fn migration_failure_rolls_back_schema_and_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("failure.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        backup(&connection, &dir.path().join("before.sqlite3")).unwrap();
        assert!(
            apply_initial_migration(
                &mut connection,
                &path,
                "synthetic-time",
                &format!("{SCHEMA}\nINVALID SQL;")
            )
            .is_err()
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM sqlite_master WHERE type='table'",
                    [],
                    |r| r.get(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            0
        );
    }
}
