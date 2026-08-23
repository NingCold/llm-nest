use std::path::PathBuf;

/// Default directory for persisted sessions.
///
/// Resolution order:
/// 1. `LLMN_DATA_DIR` environment variable (useful for demos and tests),
/// 2. the platform data directory (`dirs::data_dir`) + `llmn`,
/// 3. fallback to `./data` when no platform directory exists.
pub fn default_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LLMN_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .map(|base| base.join("llmn"))
        .unwrap_or_else(|| PathBuf::from("data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_wins() {
        temp_env::with_var("LLMN_DATA_DIR", Some("/tmp/llmn-env-test"), || {
            assert_eq!(default_data_dir(), PathBuf::from("/tmp/llmn-env-test"));
        });
        // without the variable the platform data dir (or the fallback) is used
        temp_env::with_var("LLMN_DATA_DIR", None::<&str>, || {
            assert_ne!(default_data_dir(), PathBuf::from("/tmp/llmn-env-test"));
            assert!(!default_data_dir().as_os_str().is_empty());
        });
    }
}
