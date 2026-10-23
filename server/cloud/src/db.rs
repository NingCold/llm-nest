//! PostgreSQL-only SQLx imports and the migrations embedded in the server binary.
pub use sqlx_core::{
    error::Error, executor::Executor, query::query, query_scalar::query_scalar, raw_sql::raw_sql,
    row::Row, transaction::Transaction,
};
pub use sqlx_postgres as postgres;
pub use sqlx_postgres::{PgPool, Postgres};

pub fn migrator() -> sqlx_core::migrate::Migrator {
    use sqlx_core::migrate::{Migration, MigrationType, Migrator};
    Migrator {
        migrations: vec![
            Migration::new(
                1,
                "accounts".into(),
                MigrationType::Simple,
                include_str!("../migrations/0001_accounts.sql").into(),
                false,
            ),
            Migration::new(
                2,
                "chat".into(),
                MigrationType::Simple,
                include_str!("../migrations/0002_chat.sql").into(),
                false,
            ),
            Migration::new(
                3,
                "nodes".into(),
                MigrationType::Simple,
                include_str!("../migrations/0003_nodes.sql").into(),
                false,
            ),
            Migration::new(
                4,
                "groups".into(),
                MigrationType::Simple,
                include_str!("../migrations/0004_groups.sql").into(),
                false,
            ),
            Migration::new(
                5,
                "consciousness".into(),
                MigrationType::Simple,
                include_str!("../migrations/0005_consciousness.sql").into(),
                false,
            ),
            Migration::new(
                6,
                "participation".into(),
                MigrationType::Simple,
                include_str!("../migrations/0006_participation.sql").into(),
                false,
            ),
        ]
        .into(),
        ..Migrator::DEFAULT
    }
}
