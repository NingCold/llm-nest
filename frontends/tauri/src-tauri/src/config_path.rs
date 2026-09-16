use std::io::Write;
use std::path::{Path, PathBuf};

pub fn find_config_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("LLMN_CONFIG") {
        return explicit_config(PathBuf::from(path));
    }

    // Development can use the repository configuration. An installed release
    // must not depend on its working directory or write into Program Files.
    if cfg!(debug_assertions) {
        for path in [
            PathBuf::from("config/llmn.toml"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../config/llmn.toml"),
        ] {
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    ensure_user_config(&storage::default_data_dir().join("llmn.toml"))
}

fn explicit_config(path: PathBuf) -> Result<PathBuf, String> {
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("LLMN_CONFIG not found: {}", path.display()))
    }
}

fn ensure_user_config(path: &Path) -> Result<PathBuf, String> {
    let create = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => {
                file.write_all(b"# LLM Nest desktop settings\n[providers]\n")?;
                file.sync_all()
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error),
        }
    };
    create().map_err(|error| format!("cannot create config {}: {error}", path.display()))?;
    explicit_config(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_creates_valid_empty_config_and_preserves_existing_contents() {
        let dir = std::env::temp_dir().join(format!("llmn-config-{}", uuid::Uuid::new_v4()));
        let path = dir.join("llmn.toml");
        assert_eq!(ensure_user_config(&path).unwrap(), path);
        assert!(
            runtime::config::ConfigLoader::load(&path)
                .unwrap()
                .providers
                .is_empty()
        );
        std::fs::write(&path, "# user settings\n[providers]\n").unwrap();
        ensure_user_config(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# user settings\n[providers]\n"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_explicit_config_is_an_error_without_creating_a_file() {
        let path = std::env::temp_dir().join(format!("llmn-missing-{}.toml", uuid::Uuid::new_v4()));
        assert!(
            explicit_config(path.clone())
                .unwrap_err()
                .contains("LLMN_CONFIG not found")
        );
        assert!(!path.exists());
    }
}
