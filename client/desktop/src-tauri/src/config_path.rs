use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPaths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    #[serde(skip)]
    legacy_dir: Option<PathBuf>,
    #[serde(skip)]
    create_config: bool,
}

impl AppPaths {
    /// Freeze paths before .env loading, independent of Runtime/config validity.
    pub fn resolve() -> Result<Self, String> {
        let candidates = if cfg!(debug_assertions) {
            vec![
                PathBuf::from("config/llmn.toml"),
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../config/llmn.toml"),
            ]
        } else {
            Vec::new()
        };
        Self::select(
            storage::default_data_dir(),
            storage::legacy_data_dir(),
            std::env::var_os("LLMN_CONFIG").map(PathBuf::from),
            candidates,
        )
    }

    fn select(
        data_dir: PathBuf,
        legacy_dir: Option<PathBuf>,
        explicit: Option<PathBuf>,
        candidates: Vec<PathBuf>,
    ) -> Result<Self, String> {
        let selected = explicit.or_else(|| candidates.into_iter().find(|p| p.is_file()));
        let create_config = selected.is_none();
        let config_file = selected.unwrap_or_else(|| data_dir.join("llmn.toml"));
        Ok(Self {
            config_file: std::path::absolute(config_file).map_err(|e| e.to_string())?,
            data_dir: std::path::absolute(data_dir).map_err(|e| e.to_string())?,
            legacy_dir,
            create_config,
        })
    }

    pub fn prepare(&self) -> Result<(), String> {
        storage::prepare_data_dir(&self.data_dir, self.legacy_dir.as_deref())
            .map_err(|e| e.to_string())?;
        if self.create_config {
            ensure_user_config(&self.config_file)?;
        } else {
            explicit_config(self.config_file.clone())?;
        }
        Ok(())
    }
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

    #[test]
    fn explicit_paths_win_and_remain_available_when_config_is_invalid() {
        let dir = std::env::temp_dir().join(format!("llmn-paths-{}", uuid::Uuid::new_v4()));
        let explicit = dir.join("custom/llmn.toml");
        let paths =
            AppPaths::select(dir.join("data"), None, Some(explicit.clone()), Vec::new()).unwrap();
        assert_eq!(paths.config_file, explicit);
        assert!(!paths.create_config);
        assert!(paths.prepare().is_err());
        assert!(!explicit.exists());
        std::fs::create_dir_all(explicit.parent().unwrap()).unwrap();
        std::fs::write(&explicit, "invalid toml [[[").unwrap();
        paths.prepare().unwrap(); // Paths and opening the folder don't parse config.
        assert!(runtime::config::ConfigLoader::load(&explicit).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn default_config_is_migrated_before_first_run_bootstrap() {
        let dir = std::env::temp_dir().join(format!("llmn-paths-{}", uuid::Uuid::new_v4()));
        let old = dir.join("old");
        std::fs::create_dir_all(&old).unwrap();
        let content = "# preserved settings\n[providers]\n";
        std::fs::write(old.join("llmn.toml"), content).unwrap();
        let paths = AppPaths::select(dir.join(".llmn"), Some(old), None, Vec::new()).unwrap();
        paths.prepare().unwrap();
        assert_eq!(std::fs::read_to_string(paths.config_file).unwrap(), content);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
