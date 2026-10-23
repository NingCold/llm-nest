use crate::store::{Config, Profile, Store};
use ai_client::{ModelRouter, ModelSelection};
use anyhow::{Result, ensure};
use connector::node::*;
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};

pub fn models(config: &Config) -> Result<Vec<AgentModelChoice>> {
    Ok(ModelRouter::new(&config.providers)?
        .list_models()
        .into_iter()
        .map(|m| AgentModelChoice {
            provider_id: m.provider,
            model_id: m.spec.id,
            name: m.spec.display_name,
            reasoning_efforts: m
                .spec
                .reasoning
                .map(|r| r.levels.into_iter().map(|e| e.as_wire().into()).collect())
                .unwrap_or_default(),
            image_input: match m.spec.image_input.support {
                ai_client::image_input::ImageSupport::Supported => "supported",
                ai_client::image_input::ImageSupport::Unsupported => "unsupported",
                ai_client::image_input::ImageSupport::Unknown => "unknown",
            }
            .into(),
        })
        .collect())
}
#[derive(Debug)]
struct ProfileValidation(String);
impl std::fmt::Display for ProfileValidation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ProfileValidation {}
fn invalid(error: impl std::fmt::Display) -> anyhow::Error {
    ProfileValidation(error.to_string()).into()
}
pub fn public_error(error: &anyhow::Error) -> ConsciousnessError {
    let (code, message) = if let Some(validation) = error.downcast_ref::<ProfileValidation>() {
        ("profile_invalid", validation.0.clone())
    } else {
        match error.to_string().as_str() {
            "revision_conflict" => (
                "revision_conflict",
                "配置已被修改，请刷新后重新保存。".into(),
            ),
            "request_conflict" => (
                "revision_conflict",
                "请求标识已用于另一项修改，请刷新后重新保存。".into(),
            ),
            _ => (
                "node_storage_failed",
                "节点未能保存或读取配置，请以原命令重试。".into(),
            ),
        }
    };
    ConsciousnessError {
        code: code.into(),
        message,
    }
}
pub fn validate(config: &Config, profile: &Profile) -> Result<()> {
    if profile.persona.len() > 64 * 1024 {
        return Err(invalid("persona 必须不超过 65536 字节"));
    }
    if profile
        .auto_daily_tokens
        .is_some_and(|v| !(1024..=4_000_000).contains(&v))
    {
        return Err(invalid("autoDailyTokens 必须在 1024–4000000 之间"));
    }
    let resolved = ModelRouter::new(&config.providers)?
        .resolve(&profile.selection)
        .map_err(invalid)?;
    builtin_agent::generation::options(
        &builtin_agent::generation::Parameters {
            temperature: profile.temperature,
            max_tokens: profile.max_tokens,
        },
        &resolved,
    )
    .map_err(invalid)?;
    Ok(())
}
pub fn manage(
    config: &Config,
    store: &Store,
    ai: &str,
    request: &str,
    operator: &str,
    operation: AgentManagementOperation,
) -> Result<AgentManagementView> {
    if !matches!(operation, AgentManagementOperation::View) {
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(operator, &operation))?)
        );
        let tx = store.db.unchecked_transaction()?;
        let old: Option<String> = tx
            .query_row(
                "SELECT fingerprint FROM profile_receipts WHERE ai=?1 AND request=?2",
                params![ai, request],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = old {
            ensure!(old == fingerprint, "request_conflict");
        } else if let AgentManagementOperation::SaveProfile {
            expected_revision,
            persona,
            provider_id,
            model_id,
            reasoning_effort,
            temperature,
            max_tokens,
            auto_daily_tokens,
        } = operation
        {
            let old = store.profile(ai)?;
            ensure!(old.revision == expected_revision, "revision_conflict");
            let profile = Profile {
                revision: crate::id(),
                persona,
                selection: ModelSelection {
                    provider: provider_id,
                    model: model_id,
                    reasoning_effort: reasoning_effort
                        .map(|e| serde_json::from_value(serde_json::Value::String(e)))
                        .transpose()
                        .map_err(|_| {
                            invalid("reasoningEffort 必须为 off、low、medium、high 或 max")
                        })?,
                },
                temperature,
                max_tokens,
                auto_daily_tokens,
            };
            validate(config, &profile)?;
            tx.execute(
                "UPDATE profiles SET json=?2 WHERE ai=?1",
                params![ai, serde_json::to_string(&profile)?],
            )?;
            tx.execute(
                "INSERT INTO profile_receipts VALUES(?1,?2,?3)",
                params![ai, request, fingerprint],
            )?;
        }
        tx.commit()?;
    }
    let p = store.profile(ai)?;
    Ok(AgentManagementView {
        profile: AgentProfile {
            revision: p.revision,
            persona: p.persona,
            provider_id: p.selection.provider,
            model_id: p.selection.model,
            reasoning_effort: p.selection.reasoning_effort.map(|e| e.as_wire().into()),
            temperature: p.temperature,
            max_tokens: p.max_tokens,
            auto_daily_tokens: p.auto_daily_tokens,
        },
        models: models(config)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        toml::from_str(
            r#"
server="http://127.0.0.1:1"
node_id="node"
node_name="test"
daily_calls=20
daily_tokens=200000
[profile]
revision="1"
persona="test"
[profile.selection]
provider="test"
model="small"
[providers.test]
protocol="openai"
base_url="http://127.0.0.1:1/v1"
api_key={env="UNUSED_TEST_KEY"}
[providers.test.models.small]
model="small"
context_window=8192
max_tokens=4096
"#,
        )
        .unwrap()
    }
    fn save(revision: &str, persona: &str) -> AgentManagementOperation {
        AgentManagementOperation::SaveProfile {
            expected_revision: revision.into(),
            persona: persona.into(),
            provider_id: "test".into(),
            model_id: "small".into(),
            reasoning_effort: None,
            temperature: None,
            max_tokens: Some(128),
            auto_daily_tokens: Some(1024),
        }
    }
    #[test]
    fn save_is_atomic_and_replays_current_view_without_storing_persona_in_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("node.sqlite")).unwrap();
        let config = config();
        store
            .bind(
                &crate::store::Binding {
                    ai: "ai".into(),
                    version: "1".into(),
                    secret: "local-only".into(),
                },
                &config.profile,
            )
            .unwrap();
        let first = manage(
            &config,
            &store,
            "ai",
            "request",
            "human",
            save("1", "first private persona"),
        )
        .unwrap();
        let second = manage(
            &config,
            &store,
            "ai",
            "second",
            "human",
            save(&first.profile.revision, "new private persona"),
        )
        .unwrap();
        let replay = manage(
            &config,
            &store,
            "ai",
            "request",
            "human",
            save("1", "first private persona"),
        )
        .unwrap();
        assert_eq!(replay, second);
        assert!(
            manage(
                &config,
                &store,
                "ai",
                "request",
                "human",
                save("1", "different")
            )
            .is_err()
        );
        assert!(manage(&config, &store, "ai", "stale", "human", save("1", "stale")).is_err());
        let mut invalid = save(&second.profile.revision, "invalid");
        if let AgentManagementOperation::SaveProfile { model_id, .. } = &mut invalid {
            *model_id = "missing".into();
        }
        let invalid_error = manage(&config, &store, "ai", "invalid", "human", invalid).unwrap_err();
        let public = public_error(&invalid_error);
        assert_eq!(public.code, "profile_invalid");
        assert!(public.message.contains("small"));
        assert!(
            public_error(&anyhow::anyhow!("revision_conflict"))
                .message
                .contains("刷新")
        );
        assert_eq!(
            store.profile("ai").unwrap().revision,
            second.profile.revision
        );
        let count: i64 = store
            .db
            .query_row("SELECT COUNT(*) FROM profile_receipts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let fingerprint: String = store
            .db
            .query_row(
                "SELECT fingerprint FROM profile_receipts LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fingerprint.len(), 64);
        let wire = serde_json::to_string(&replay).unwrap();
        assert!(!wire.contains("UNUSED_TEST_KEY"));
        assert!(!wire.contains("127.0.0.1"));
    }
}
