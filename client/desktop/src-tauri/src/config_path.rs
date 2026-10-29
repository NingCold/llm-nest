use std::path::PathBuf;
/// Preserve the existing desktop credentials and Agent data locations.
pub fn default_data_dir() -> PathBuf {
    std::env::var_os("LLMN_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".llmn")
        })
}
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPaths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
}
impl AppPaths {
    pub fn resolve() -> Result<Self, String> {
        let data_dir = std::path::absolute(default_data_dir()).map_err(|e| e.to_string())?;
        // Location hint only: the cloud client does not load or create host config.
        let config_file = std::env::var_os("LLMN_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("llmn.toml"));
        Ok(Self {
            config_file: std::path::absolute(config_file).map_err(|e| e.to_string())?,
            data_dir,
        })
    }
}
