use std::fs;
use std::path::{Path, PathBuf};

use crate::{DataDirectoryGuard, Result};

/// Default directory for persisted sessions.
///
/// Resolution order:
/// 1. `LLMN_DATA_DIR` environment variable (for a custom data location),
/// 2. the user's home directory + `.llmn`,
/// 3. fallback to `./.llmn` when no home directory is available.
pub fn default_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LLMN_DATA_DIR") {
        return PathBuf::from(dir);
    }
    home_data_dir(dirs::home_dir())
}

fn home_data_dir(home: Option<PathBuf>) -> PathBuf {
    home.unwrap_or_else(|| PathBuf::from(".")).join(".llmn")
}

/// Only the former default is eligible for migration. Explicit directories are isolated.
pub fn legacy_data_dir() -> Option<PathBuf> {
    if std::env::var_os("LLMN_DATA_DIR").is_some() {
        return None;
    }
    dirs::data_dir().map(|base| base.join("llmn"))
}

/// Call before loading config/.env or opening the session store in host binaries.
pub fn prepare_default_data_dir() -> Result<PathBuf> {
    let path = default_data_dir();
    prepare_data_dir(&path, legacy_data_dir().as_deref())?;
    Ok(path)
}

/// Copy an unused legacy directory once, publishing the complete copy by rename.
/// An existing destination always wins; never merge, overwrite, or delete old data.
pub fn prepare_data_dir(path: &Path, legacy: Option<&Path>) -> Result<()> {
    if path.try_exists()? {
        if !path.is_dir() {
            return Err(
                std::io::Error::other(format!("not a data directory: {}", path.display())).into(),
            );
        }
        return Ok(());
    }
    let Some(legacy) = legacy.filter(|old| *old != path) else {
        return create_private_dir(path);
    };
    if !legacy.try_exists()? {
        return create_private_dir(path);
    }
    // The old application's writer lock prevents copying a moving snapshot.
    let _source_lock = DataDirectoryGuard::acquire(legacy)?;
    if path.try_exists()? {
        return prepare_data_dir(path, None);
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".llmn-import-{}", common::MessageId::new()));
    create_private_dir(&staging)?;
    let copy = || -> std::io::Result<()> {
        for entry in fs::read_dir(legacy)? {
            let entry = entry?;
            let source = entry.path();
            let name = entry.file_name();
            if name != "llmn.toml"
                && name != ".env"
                && source.extension().is_none_or(|ext| ext != "json")
            {
                continue;
            }
            if !entry.file_type()?.is_file() {
                return Err(std::io::Error::other(format!(
                    "cannot migrate non-regular file: {}",
                    source.display()
                )));
            }
            let destination = staging.join(name);
            let mut input = fs::File::open(&source)?;
            let mut output = fs::File::create(&destination)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
            fs::set_permissions(&destination, input.metadata()?.permissions())?;
        }
        fs::rename(&staging, path)
    };
    if let Err(error) = copy() {
        // This unique staging directory was created above and contains only copied files.
        let _ = fs::remove_dir_all(&staging);
        return Err(std::io::Error::other(format!(
            "cannot migrate {} to {}: {error}",
            legacy.display(),
            path.display()
        ))
        .into());
    }
    Ok(())
}

fn create_private_dir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataDirectoryGuard;

    #[test]
    fn env_var_wins() {
        temp_env::with_var("LLMN_DATA_DIR", Some("/tmp/llmn-env-test"), || {
            assert_eq!(default_data_dir(), PathBuf::from("/tmp/llmn-env-test"));
        });
        // without the variable the user's home is used
        temp_env::with_var("LLMN_DATA_DIR", None::<&str>, || {
            assert_ne!(default_data_dir(), PathBuf::from("/tmp/llmn-env-test"));
            assert!(!default_data_dir().as_os_str().is_empty());
            assert_eq!(default_data_dir(), home_data_dir(dirs::home_dir()));
            assert_eq!(
                home_data_dir(Some(PathBuf::from("user-home"))),
                PathBuf::from("user-home/.llmn")
            );
        });
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("llmn-migration-{}", common::MessageId::new()));
            fs::create_dir_all(dir.join("old")).unwrap();
            Self(dir)
        }
        fn old(&self) -> PathBuf {
            self.0.join("old")
        }
        fn new_dir(&self) -> PathBuf {
            self.0.join(".llmn")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn migration_preserves_config_env_sessions_and_source() {
        let fixture = Fixture::new();
        for name in ["llmn.toml", ".env", "session.json"] {
            fs::write(fixture.old().join(name), format!("fixture {name}")).unwrap();
        }
        fs::write(fixture.old().join(".partial.tmp"), "ignore").unwrap();
        prepare_data_dir(&fixture.new_dir(), Some(&fixture.old())).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(fixture.new_dir())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        for name in ["llmn.toml", ".env", "session.json"] {
            assert_eq!(
                fs::read(fixture.new_dir().join(name)).unwrap(),
                fs::read(fixture.old().join(name)).unwrap()
            );
        }
        assert!(!fixture.new_dir().join(".llmn.lock").exists());
        assert!(!fixture.new_dir().join(".partial.tmp").exists());
        fs::write(fixture.new_dir().join("llmn.toml"), "new settings").unwrap();
        prepare_data_dir(&fixture.new_dir(), Some(&fixture.old())).unwrap();
        assert_eq!(
            fs::read_to_string(fixture.new_dir().join("llmn.toml")).unwrap(),
            "new settings"
        );
    }

    #[test]
    fn locked_source_does_not_create_destination_and_retry_succeeds() {
        let fixture = Fixture::new();
        let source = DataDirectoryGuard::acquire(fixture.old()).unwrap();
        assert!(prepare_data_dir(&fixture.new_dir(), Some(&fixture.old())).is_err());
        assert!(!fixture.new_dir().exists());
        drop(source);
        prepare_data_dir(&fixture.new_dir(), Some(&fixture.old())).unwrap();
    }

    #[test]
    fn failed_copy_does_not_publish_partial_directory() {
        let fixture = Fixture::new();
        fs::write(fixture.old().join("session.json"), "keep").unwrap();
        fs::create_dir(fixture.old().join("llmn.toml")).unwrap();
        assert!(prepare_data_dir(&fixture.new_dir(), Some(&fixture.old())).is_err());
        assert!(!fixture.new_dir().exists());
        assert_eq!(
            fs::read_to_string(fixture.old().join("session.json")).unwrap(),
            "keep"
        );
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[test]
    fn existing_destination_and_explicit_paths_do_not_import() {
        let fixture = Fixture::new();
        fs::write(fixture.old().join("llmn.toml"), "old").unwrap();
        prepare_data_dir(&fixture.new_dir(), None).unwrap();
        prepare_data_dir(&fixture.new_dir(), Some(&fixture.old())).unwrap();
        assert!(!fixture.new_dir().join("llmn.toml").exists());
        temp_env::with_var("LLMN_DATA_DIR", Some(fixture.new_dir()), || {
            assert_eq!(legacy_data_dir(), None);
        });
    }
}
