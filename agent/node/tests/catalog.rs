use connector::management::{AGENT_CATALOG_PROTOCOL_VERSION, AgentCatalog};
use std::process::Command;

#[test]
fn catalog_uses_builtin_models_without_node_configuration_or_secrets() {
    let workspace = tempfile::tempdir().unwrap();
    let directory = workspace.path().join("missing-node");
    let output = Command::new(env!("CARGO_BIN_EXE_llmn-agent"))
        .arg("catalog")
        .arg(&directory)
        .current_dir(workspace.path())
        .env_clear()
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let wire: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(wire["protocolVersion"], AGENT_CATALOG_PROTOCOL_VERSION);
    assert_eq!(wire["agentVersion"], env!("CARGO_PKG_VERSION"));
    let catalog: AgentCatalog = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        catalog.providers.len(),
        ai_client::catalog::BUILTIN_PROVIDERS.len()
    );
    for (provider, source) in catalog
        .providers
        .iter()
        .zip(ai_client::catalog::BUILTIN_PROVIDERS)
    {
        assert_eq!(provider.id, source.id);
        assert_eq!(provider.name, source.display_name);
        assert_eq!(provider.models.len(), source.models.len());
        for (model, source) in provider.models.iter().zip(source.models) {
            assert_eq!(model.id, source.id);
            assert_eq!(model.name, source.display_name);
            assert_eq!(model.context_window, source.context_window);
            let efforts: Vec<String> = source
                .reasoning
                .map(|r| r.levels.iter().map(|e| e.as_wire().into()).collect())
                .unwrap_or_default();
            assert_eq!(model.reasoning_efforts, efforts);
        }
    }
    let model = &wire["providers"][0]["models"][0];
    assert!(model.get("contextWindow").is_some());
    assert!(model.get("reasoningEfforts").is_some());
    assert_eq!(std::fs::read_dir(workspace.path()).unwrap().count(), 0);
}

#[test]
fn version_requires_no_node_directory() {
    let workspace = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_llmn-agent"))
        .arg("--version")
        .current_dir(workspace.path())
        .env_clear()
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        concat!("llmn-agent ", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(std::fs::read_dir(workspace.path()).unwrap().count(), 0);
}

#[test]
fn check_rejects_unknown_model_without_opening_node_storage() {
    let directory = tempfile::tempdir().unwrap();
    let config = r#"
server="http://127.0.0.1:1"
node_id="test"
node_name="test"
daily_calls=20
daily_tokens=200000
[profile]
revision="1"
persona="test"
[profile.selection]
provider="chatecnu"
model="missing-model"
[providers.chatecnu]
api_key={env="UNSET_CATALOG_TEST_KEY"}
"#;
    std::fs::write(directory.path().join("node.toml"), config).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_llmn-agent"))
        .arg("check")
        .arg(directory.path())
        .env_clear()
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing-model"));
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}
