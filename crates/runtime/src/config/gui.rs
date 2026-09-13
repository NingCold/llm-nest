//! Shared Web/Tauri settings. Only this section is edited; provider secrets stay server-side.
use crate::error::{Result, RuntimeError};
use ai_client::ModelSelection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiConfig {
    pub current_model: ModelSelection,
    pub temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

impl GuiConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.temperature.is_finite() || !(0.0..=2.0).contains(&self.temperature) {
            return Err(RuntimeError::ConfigError(
                "temperature 必须在 0 到 2 之间".into(),
            ));
        }
        if self.max_tokens == Some(0) {
            return Err(RuntimeError::ConfigError(
                "maxTokens 必须是正整数，或留空".into(),
            ));
        }
        Ok(())
    }
}

pub fn read_document(path: &std::path::Path) -> Result<String> {
    std::fs::read_to_string(path)
        .map_err(|e| RuntimeError::ConfigError(format!("无法读取配置 {}: {e}", path.display())))
}

pub fn read_gui(text: &str) -> Result<Option<GuiConfig>> {
    #[derive(Deserialize)]
    struct Document {
        gui: Option<GuiConfig>,
    }
    let doc: Document = toml::from_str(text)
        .map_err(|e| RuntimeError::ConfigError(format!("GUI 设置格式错误: {e}")))?;
    if let Some(gui) = &doc.gui {
        gui.validate()?;
    }
    Ok(doc.gui)
}

pub fn render_gui(text: &str, config: &GuiConfig) -> Result<String> {
    use toml_edit::{DocumentMut, Item, Table};
    config.validate()?;
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e| RuntimeError::ConfigError(format!("配置格式错误: {e}")))?;
    let table = doc
        .entry("gui")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| RuntimeError::ConfigError("GUI 设置请使用 [gui] 分节格式".into()))?;
    let rendered: DocumentMut = toml::to_string(config)
        .map_err(|e| RuntimeError::ConfigError(e.to_string()))?
        .parse()
        .map_err(|e| RuntimeError::ConfigError(format!("{e}")))?;
    for key in ["currentModel", "temperature", "maxTokens"] {
        if let Some(item) = rendered.get(key) {
            table.insert(key, item.clone());
        } else {
            table.remove(key);
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use crate::runtime::Runtime;
    #[tokio::test]
    async fn gui_settings_roundtrip_validation_and_failed_write() {
        let dir = std::env::temp_dir().join(format!("llmn-gui-{}", common::SessionId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let source = "# preserved comment\n[providers.test]\nprotocol = \"openai\"\nbase_url = \"http://127.0.0.1:1/v1\"\napi_key = \"test-only\"\n[providers.test.models.chat]\nmodel = \"chat\"\n";
        std::fs::write(&path, source).unwrap();
        let runtime = Runtime::from_config(&path).unwrap();
        let mut config = runtime.gui_config().await.unwrap();
        config.temperature = 0.2;
        config.max_tokens = Some(2048);
        runtime.set_gui_config(&config).await.unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.starts_with("# preserved comment"));
        assert!(saved.contains("test-only"));
        let reopened = Runtime::from_config(&path)
            .unwrap()
            .gui_config()
            .await
            .unwrap();
        assert_eq!(reopened.temperature, 0.2);
        assert_eq!(reopened.max_tokens, Some(2048));
        config.temperature = f64::NAN;
        assert!(runtime.set_gui_config(&config).await.is_err());
        config.temperature = 0.2;
        config.max_tokens = Some(0);
        assert!(runtime.set_gui_config(&config).await.is_err());
        config.max_tokens = None;
        config.current_model.model = "missing".into();
        assert!(runtime.set_gui_config(&config).await.is_err());
        assert_eq!(saved, std::fs::read_to_string(&path).unwrap());
        config.current_model.model = "chat".into();
        runtime.set_gui_config(&config).await.unwrap();
        assert_eq!(runtime.gui_config().await.unwrap().max_tokens, None);
        // File replaced with a directory: saving must return an error, not success.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(runtime.set_gui_config(&config).await.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
