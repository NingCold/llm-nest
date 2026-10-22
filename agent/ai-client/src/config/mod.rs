pub mod model;
pub mod provider;

pub use model::*;
pub use provider::*;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::reasoning::{ReasoningEffort, ReasoningFormat};

    /// Pre-routing config files (no capability/default fields) must still
    /// parse, with every new field defaulted away.
    #[test]
    fn parses_legacy_config() {
        let text = r#"
[chatecnu]
protocol = "openai"
api_key = "sk-test"
base_url = "https://example.com/v1/"

[chatecnu.models.ecnu-max]
model = "ecnu-max"
display_name = "DeepSeek-V4-Flash"
"#;
        let providers: HashMap<ProviderId, ProviderConfig> =
            toml::from_str(text).expect("legacy config parses");
        let cfg = providers.get(&ProviderId::new("chatecnu")).unwrap();
        assert!(cfg.default_model.is_none());
        assert!(cfg.headers.is_empty());
        assert!(cfg.timeout_ms.is_none());
        let model = cfg.models.get(&ModelId::new("ecnu-max")).unwrap();
        assert_eq!(model.model, "ecnu-max");
        assert!(model.context_window.is_none());
        assert!(model.max_tokens.is_none());
        assert!(model.reasoning.is_none());
    }

    /// New fields parse into the routing types.
    #[test]
    fn parses_routing_config() {
        let text = r#"
[deepseek]
protocol = "openai"
api_key = { env = "DEEPSEEK_API_KEY" }
base_url = "https://api.deepseek.com"
default_model = "chat"
[deepseek.headers]
X-Custom = "v1"

[deepseek.models.chat]
model = "deepseek-chat"
context_window = 131072
max_tokens = 16384

[deepseek.models.reasoner]
model = "deepseek-reasoner"
reasoning = { levels = ["low", "high"], format = "deepseek-thinking" }
"#;
        let providers: HashMap<ProviderId, ProviderConfig> =
            toml::from_str(text).expect("routing config parses");
        let cfg = providers.get(&ProviderId::new("deepseek")).unwrap();
        assert_eq!(cfg.default_model.as_deref(), Some("chat"));
        assert_eq!(cfg.headers.get("X-Custom").map(String::as_str), Some("v1"));
        let chat = cfg.models.get(&ModelId::new("chat")).unwrap();
        assert_eq!(chat.context_window, Some(131072));
        assert_eq!(chat.max_tokens, Some(16384));
        let reasoner = cfg.models.get(&ModelId::new("reasoner")).unwrap();
        let capability = reasoner.reasoning.as_ref().unwrap();
        assert_eq!(capability.format, ReasoningFormat::DeepSeekThinking);
        assert_eq!(
            capability.levels,
            vec![ReasoningEffort::Low, ReasoningEffort::High]
        );
    }

    /// ECNU ecnu-max style capability: thinking switch + effort levels
    /// (low/high/max), parsed from `deepseek-effort`.
    #[test]
    fn parses_deepseek_effort_capability() {
        let text = r#"
[chatecnu]
protocol = "openai"
api_key = "sk-test"
base_url = "https://chat.ecnu.edu.cn/open/api/v1/"

[chatecnu.models.ecnu-max]
model = "ecnu-max"
reasoning = { levels = ["off", "low", "high", "max"], format = "deepseek-effort" }
"#;
        let providers: HashMap<ProviderId, ProviderConfig> =
            toml::from_str(text).expect("deepseek-effort config parses");
        let cfg = providers.get(&ProviderId::new("chatecnu")).unwrap();
        let model = cfg.models.get(&ModelId::new("ecnu-max")).unwrap();
        let capability = model.reasoning.as_ref().unwrap();
        assert_eq!(capability.format, ReasoningFormat::DeepSeekEffort);
        assert_eq!(
            capability.levels,
            vec![
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ]
        );
    }
}
