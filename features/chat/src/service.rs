use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context, Result};
use common::{ChatOptions, SessionId};
use futures_util::Stream;
use runtime::config::ConfigLoader;
use runtime::event::RuntimeEvent;
use runtime::runtime::Runtime;
use tokio::sync::{RwLock, mpsc};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::event::ChatEvent;

#[derive(Clone)]
pub struct ChatService {
    runtime: Arc<Runtime>,
    current_session: Arc<RwLock<Option<SessionId>>>,
}

impl ChatService {
    pub fn from_config(path: impl AsRef<Path>) -> Result<Self> {
        let config = ConfigLoader::load(path.as_ref())
            .with_context(|| format!("failed to load config from {:?}", path.as_ref()))?;
        let runtime = Runtime::builder()
            .config(config)
            .build()?;
        Ok(Self {
            runtime: Arc::new(runtime),
            current_session: Arc::new(RwLock::new(None)),
        })
    }

    pub async fn chat_stream(
        &self,
        input: String,
        options: ChatOptions,
    ) -> Result<impl Stream<Item = ChatEvent>> {
        let session_id = self.current_session.read().await
            .ok_or_else(|| anyhow::anyhow!("no active session, create one with /new"))?;
        let request_id = self.runtime.create_request_id();
        let mut rx = self.runtime.subscribe();
        let (tx, rx_local) = mpsc::unbounded_channel::<ChatEvent>();
        let rt = self.runtime.clone();

        let _ = tx.send(ChatEvent::ResponseStarted);

        tokio::spawn(async move {
            let send_handle = tokio::spawn(async move {
                rt.send(session_id, request_id, input, options).await
            });

            loop {
                match rx.recv().await {
                    Ok(RuntimeEvent::ResponseDelta { request_id: rid, delta, .. }) if rid == request_id => {
                        if tx.send(ChatEvent::ResponseDelta { content: delta }).is_err() {
                            break;
                        }
                    }
                    Ok(RuntimeEvent::ResponseFinished { request_id: rid, .. }) if rid == request_id => {
                        let _ = tx.send(ChatEvent::ResponseFinished);
                        break;
                    }
                    Ok(RuntimeEvent::Error { request_id: rid, message, .. }) if rid == request_id => {
                        let _ = tx.send(ChatEvent::Error { message });
                        break;
                    }
                    _ => {}
                }
            }

            let _ = send_handle.await;
        });

        Ok(UnboundedReceiverStream::new(rx_local))
    }

    pub async fn new_session(&self, title: Option<String>) -> Result<SessionId> {
        let id = self.runtime.create_session(title).await?;
        *self.current_session.write().await = Some(id);
        Ok(id)
    }

    pub async fn current_session(&self) -> Option<SessionId> {
        *self.current_session.read().await
    }

    pub async fn switch_session(&self, target: &str) -> Result<bool> {
        // try UUID match first
        if let Ok(id) = SessionId::from_str(target)
            && self.runtime.get_session(&id).await.is_some()
        {
            *self.current_session.write().await = Some(id);
            return Ok(true);
        }

        // try title match
        let ids = self.runtime.list_sessions().await;
        for id in &ids {
            if let Some(session) = self.runtime.get_session(id).await
                && session.title() == Some(target)
            {
                *self.current_session.write().await = Some(*id);
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub async fn rename_session(&self, id: &str, title: String) -> Result<()> {
        let sid = SessionId::from_str(id)
            .with_context(|| format!("invalid session id: {}", id))?;
        self.runtime.rename_session(sid, title).await?;
        Ok(())
    }

    pub async fn list_sessions(&self) -> Result<Vec<(String, String)>> {
        let ids = self.runtime.list_sessions().await;
        let mut result = Vec::new();
        for id in &ids {
            let title = self.runtime.get_session(id).await
                .and_then(|s| s.title().map(String::from))
                .unwrap_or_default();
            result.push((id.to_string(), title));
        }
        Ok(result)
    }

    pub async fn delete_session(&self, id: &str) -> Result<()> {
        let sid = SessionId::from_str(id)
            .with_context(|| format!("invalid session id: {}", id))?;
        self.runtime.delete_session(sid).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use common::config::{ModelConfig, ProviderConfig, ProviderId, ModelId};
    use runtime::config::RuntimeConfig;

    fn make_test_runtime() -> Arc<Runtime> {
        let mut models = HashMap::new();
        models.insert(ModelId::new("gpt-4"), ModelConfig { model: "gpt-4".into(), display_name: None });
        let mut providers = HashMap::new();
        providers.insert(ProviderId::new("test"), ProviderConfig {
            protocol: common::config::Protocol::OpenAI,
            api_key: common::config::ApiKey::Direct("sk-test".into()),
            base_url: "https://api.openai.com".into(),
            models,
        });
        let config = RuntimeConfig { providers };
        Arc::new(Runtime::from_config(config).unwrap())
    }

    #[tokio::test]
    async fn chat_service_new_session() {
        let svc = ChatService {
            runtime: make_test_runtime(),
            current_session: Arc::new(RwLock::new(None)),
        };
        let id = svc.new_session(Some("test session".into())).await.unwrap();
        assert_eq!(svc.current_session().await, Some(id));
    }

    #[tokio::test]
    async fn chat_service_new_session_then_list() {
        let svc = ChatService {
            runtime: make_test_runtime(),
            current_session: Arc::new(RwLock::new(None)),
        };
        let id = svc.new_session(None).await.unwrap();
        let sessions = svc.list_sessions().await.unwrap();
        assert!(sessions.iter().any(|(sid, _)| sid == &id.to_string()));
    }

    #[tokio::test]
    async fn chat_service_switch_session_by_id() {
        let svc = ChatService {
            runtime: make_test_runtime(),
            current_session: Arc::new(RwLock::new(None)),
        };
        let id = svc.new_session(None).await.unwrap();
        let switched = svc.switch_session(&id.to_string()).await.unwrap();
        assert!(switched);
        assert_eq!(svc.current_session().await, Some(id));
    }

    #[tokio::test]
    async fn chat_service_switch_session_by_title() {
        let svc = ChatService {
            runtime: make_test_runtime(),
            current_session: Arc::new(RwLock::new(None)),
        };
        let id = svc.new_session(Some("my-session".into())).await.unwrap();
        let switched = svc.switch_session("my-session").await.unwrap();
        assert!(switched);
        assert_eq!(svc.current_session().await, Some(id));
    }

    #[tokio::test]
    async fn chat_service_switch_nonexistent() {
        let svc = ChatService {
            runtime: make_test_runtime(),
            current_session: Arc::new(RwLock::new(None)),
        };
        let switched = svc.switch_session("nonexistent").await.unwrap();
        assert!(!switched);
    }

    #[tokio::test]
    async fn chat_service_chat_stream_needs_active_session() {
        let svc = ChatService {
            runtime: make_test_runtime(),
            current_session: Arc::new(RwLock::new(None)),
        };
        let result = svc.chat_stream("hello".into(), ChatOptions::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn chat_service_delete_session() {
        let svc = ChatService {
            runtime: make_test_runtime(),
            current_session: Arc::new(RwLock::new(None)),
        };
        let id = svc.new_session(None).await.unwrap();
        svc.delete_session(&id.to_string()).await.unwrap();
        let sessions = svc.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn chat_service_rename_session() {
        let svc = ChatService {
            runtime: make_test_runtime(),
            current_session: Arc::new(RwLock::new(None)),
        };
        let id = svc.new_session(None).await.unwrap();
        svc.rename_session(&id.to_string(), "new title".into()).await.unwrap();
        let sessions = svc.list_sessions().await.unwrap();
        let entry = sessions.iter().find(|(sid, _)| sid == &id.to_string()).unwrap();
        assert_eq!(entry.1, "new title");
    }
}