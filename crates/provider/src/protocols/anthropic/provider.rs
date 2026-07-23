use common::config::ProviderConfig;

pub struct AnthropicProvider {
    client: reqwest::Client,
    config: ProviderConfig,
}