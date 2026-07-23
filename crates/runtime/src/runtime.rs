use std::sync::Arc;

use common::{ChatChunk, ChatOptions, ChatRequest, Message, RequestId, SessionId, config::ProviderId};
use futures_util::StreamExt;
use provider::{manager::ProviderManager, traits::Provider};
use tokio::sync::{broadcast, RwLock};

use crate::{
    builder::RuntimeBuilder, command::Command, config::RuntimeConfig, error::{Result, RuntimeError}, event::RuntimeEvent, event_bus::EventBus, session::Session, state::RuntimeState
};

#[derive(Debug, Clone)]
pub struct Runtime {
    state: Arc<RwLock<RuntimeState>>,
    event_bus: EventBus<RuntimeEvent>,
    provider_mgr: ProviderManager,
}

impl Runtime {
    pub(super) fn new(
        state: RuntimeState,
        event_bus: EventBus<RuntimeEvent>,
        provider_mgr: ProviderManager,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(state)),
            event_bus,
            provider_mgr,
        }
    }

    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::new()
    }

    pub fn from_config(
        config: RuntimeConfig,
    ) -> Result<Self> {
        let provider_mgr = ProviderManager::new(&config.providers)?;

        Ok(Self::new(
            RuntimeState::default(),
            EventBus::new(),
            provider_mgr,
        ))
    }

    fn emit(
        &self,
        event: RuntimeEvent,
    ) {
        self.event_bus.publish(event);
    }

    pub fn subscribe(
        &self,
    ) -> broadcast::Receiver<RuntimeEvent> {
        self.event_bus.subscribe()
    }


    pub async fn create_session(
        &self,
        title: Option<String>,
    ) -> Result<SessionId> {
        let session = Session::new(title);
        let id = session.id();
        self.state.write().await.insert(session);
        self.emit(RuntimeEvent::SessionCreated { session_id: id });
        Ok(id)
    }

    pub async fn delete_session(
        &self,
        session_id: SessionId,
    ) -> Result<()> {
        self.state
            .write()
            .await
            .remove(&session_id)
            .ok_or(RuntimeError::SessionNotFound(session_id))?;

        self.emit(RuntimeEvent::SessionDeleted { session_id });
        Ok(())
    }

    pub async fn rename_session(
        &self,
        session_id: SessionId,
        title: String,
    ) -> Result<()> {
        let mut state = self.state.write().await;
        let session = state
            .get_mut(&session_id)
            .ok_or(
                RuntimeError::SessionNotFound(
                    session_id
                ))?;
            
        session.set_title(title);
        drop(state);
        self.emit(RuntimeEvent::SessionChange { session_id });
        Ok(())
    }

    pub async fn get_session(
        &self,
        session_id: &SessionId,
    ) -> Option<Session> {
        self.state.read().await.get(session_id).cloned()
    }

    pub async fn list_sessions(
        &self,
    ) -> Vec<SessionId> {
        self.state
            .read()
            .await
            .iter()
            .map(|(id, _)| *id)
            .collect()
    }

    fn provider(
        &self,
        id: &ProviderId,
    ) -> Result<Arc<dyn Provider>> {
        self.provider_mgr
            .get(id)
            .ok_or_else(|| RuntimeError::ProviderNotFound(id.clone()))
    }

    pub fn create_request_id(
        &self,
    ) -> RequestId {
        RequestId::new()
    }

    pub async fn execute(
        &self,
        command: Command,
    ) -> Result<()> {
        match command {
            Command::CreateSession { title } => {
                self.create_session(title).await?;
            },

            Command::DeleteSession { session_id } => {
                self.delete_session(session_id).await?;
            },

            Command::RenameSession { session_id, title } => {
                self.rename_session(session_id, title).await?;
            },

            Command::SendMessage { session_id, request_id, content, options } => {
                self.send(session_id, request_id, content, options).await?;
            },
        }

        Ok(())
    }
    
    pub async fn send(
        &self,
        session_id: SessionId,
        request_id: RequestId,
        content: impl Into<String>,
        options: ChatOptions,
    ) -> Result<RequestId> {
        
        {
            let mut state = self.state.write().await;

            let session = state
                .get_mut(&session_id)
                .ok_or(RuntimeError::SessionNotFound(session_id))?;

            session.push(Message::user(content));
        }
        
        let request = {
            let state = self.state.read().await;
            let session = state
                .get(&session_id)
                .ok_or(RuntimeError::SessionNotFound(session_id))?;
            ChatRequest {
                messages: session.messages().to_vec(),
                options: options.clone(),
            }
        };

        let provider = match self.provider(&options.provider) {
            Ok(p) => p,
            Err(e) => {
                self.emit(RuntimeEvent::Error {
                    request_id,
                    session_id: Some(session_id),
                    kind: "provider_not_found".into(),
                    message: e.to_string(),
                });
                return Err(e);
            }
        };

        self.emit(RuntimeEvent::ResponseStarted { request_id, session_id });

        if options.stream {
            let mut stream = match provider.chat_stream(request).await {
                Ok(s) => s,
                Err(e) => {
                    self.emit(RuntimeEvent::Error {
                        request_id,
                        session_id: Some(session_id),
                        kind: "provider_error".into(),
                        message: e.to_string(),
                    });
                    return Err(e.into());
                }
            };

            let mut assistant_content = String::new();

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        self.emit(RuntimeEvent::Error {
                            request_id,
                            session_id: Some(session_id),
                            kind: "stream_error".into(),
                            message: e.to_string(),
                        });
                        return Err(e.into());
                    }
                };
                match chunk {
                    ChatChunk::Delta {
                        content
                    } => {
                        assistant_content.push_str(&content);

                        self.emit(
                            RuntimeEvent::ResponseDelta {
                                request_id,
                                session_id,
                                delta: content,
                            }
                        );
                    }

                    ChatChunk::Done => {
                        self.emit(
                            RuntimeEvent::ResponseFinished {
                                request_id,
                                session_id,
                            }
                        );
                    }
                }
            }

            {
                let mut state = self.state.write().await;
                let session = state
                    .get_mut(&session_id)
                    .ok_or(RuntimeError::SessionNotFound(session_id))?;
                session.push(Message::assistant(assistant_content));
            }

        } else {
            let response = match provider.chat(request).await {
                Ok(r) => r,
                Err(e) => {
                    self.emit(RuntimeEvent::Error {
                        request_id,
                        session_id: Some(session_id),
                        kind: "provider_error".into(),
                        message: e.to_string(),
                    });
                    return Err(e.into());
                }
            };

            {
                let mut state = self.state.write().await;
                let session = state
                    .get_mut(&session_id)
                    .ok_or(RuntimeError::SessionNotFound(session_id))?;
                session.push(response.message.clone());
            }

            self.emit(
                RuntimeEvent::ResponseFinished {
                    request_id,
                    session_id,
                }
            );
        }
        Ok(request_id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use common::config::{ModelConfig, ProviderConfig};
    use provider::manager::ProviderManager;
    use super::*;

    fn mock_provider_mgr() -> ProviderManager {
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("test"),
            ProviderConfig {
                protocol: common::config::Protocol::OpenAI,
                api_key: common::config::ApiKey::Direct("sk-test".into()),
                base_url: "https://api.openai.com".into(),
                models: {
                    let mut m = HashMap::new();
                    m.insert(
                        common::config::ModelId::new("gpt-4"),
                        ModelConfig { model: "gpt-4".into(), display_name: None },
                    );
                    m
                },
            },
        );
        ProviderManager::new(&configs).unwrap()
    }

    #[tokio::test]
    async fn runtime_create_session() {
        let rt = Runtime::new(
            RuntimeState::default(),
            EventBus::new(),
            mock_provider_mgr(),
        );
        let id = rt.create_session(Some("test".into())).await.unwrap();
        assert!(rt.get_session(&id).await.is_some());
    }

    #[tokio::test]
    async fn runtime_delete_session() {
        let rt = Runtime::new(
            RuntimeState::default(),
            EventBus::new(),
            mock_provider_mgr(),
        );
        let id = rt.create_session(Some("test".into())).await.unwrap();
        rt.delete_session(id).await.unwrap();
        assert!(rt.get_session(&id).await.is_none());
    }

    #[tokio::test]
    async fn runtime_delete_nonexistent_session() {
        let rt = Runtime::new(
            RuntimeState::default(),
            EventBus::new(),
            mock_provider_mgr(),
        );
        let err = rt.delete_session(SessionId::new()).await.unwrap_err();
        assert!(matches!(err, RuntimeError::SessionNotFound(_)));
    }

    #[tokio::test]
    async fn runtime_rename_session() {
        let rt = Runtime::new(
            RuntimeState::default(),
            EventBus::new(),
            mock_provider_mgr(),
        );
        let id = rt.create_session(None).await.unwrap();
        rt.rename_session(id, "updated".into()).await.unwrap();
        let session = rt.get_session(&id).await.unwrap();
        assert_eq!(session.title(), Some("updated"));
    }

    #[tokio::test]
    async fn runtime_list_sessions() {
        let rt = Runtime::new(
            RuntimeState::default(),
            EventBus::new(),
            mock_provider_mgr(),
        );
        let id1 = rt.create_session(None).await.unwrap();
        let id2 = rt.create_session(None).await.unwrap();
        let sessions = rt.list_sessions().await;
        assert_eq!(sessions.len(), 2);
        assert!(sessions.contains(&id1));
        assert!(sessions.contains(&id2));
    }

    #[tokio::test]
    async fn runtime_execute_create_command() {
        let rt = Runtime::new(
            RuntimeState::default(),
            EventBus::new(),
            mock_provider_mgr(),
        );
        rt.execute(Command::CreateSession { title: Some("cmd".into()) }).await.unwrap();
        assert_eq!(rt.list_sessions().await.len(), 1);
    }

    #[tokio::test]
    async fn runtime_execute_delete_command() {
        let rt = Runtime::new(
            RuntimeState::default(),
            EventBus::new(),
            mock_provider_mgr(),
        );
        let id = rt.create_session(None).await.unwrap();
        rt.execute(Command::DeleteSession { session_id: id }).await.unwrap();
        assert_eq!(rt.list_sessions().await.len(), 0);
    }

    #[tokio::test]
    async fn runtime_execute_rename_command() {
        let rt = Runtime::new(
            RuntimeState::default(),
            EventBus::new(),
            mock_provider_mgr(),
        );
        let id = rt.create_session(None).await.unwrap();
        rt.execute(Command::RenameSession { session_id: id, title: "renamed".into() }).await.unwrap();
        let session = rt.get_session(&id).await.unwrap();
        assert_eq!(session.title(), Some("renamed"));
    }

    #[tokio::test]
    async fn runtime_event_emitted_on_create() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let rt = Runtime::new(RuntimeState::default(), bus, mock_provider_mgr());
        rt.create_session(None).await.unwrap();
        let event = rx.recv().await.unwrap();
        assert!(matches!(event, RuntimeEvent::SessionCreated { .. }));
    }

    #[tokio::test]
    async fn runtime_create_request_id() {
        let rt = Runtime::new(
            RuntimeState::default(),
            EventBus::new(),
            mock_provider_mgr(),
        );
        let id1 = rt.create_request_id();
        let id2 = rt.create_request_id();
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn runtime_build_from_config() {
        use common::config::ModelId;
        let mut models = HashMap::new();
        models.insert(
            ModelId::new("gpt-4"),
            ModelConfig { model: "gpt-4".into(), display_name: None },
        );
        let mut providers_map = HashMap::new();
        providers_map.insert(
            ProviderId::new("test"),
            ProviderConfig {
                protocol: common::config::Protocol::OpenAI,
                api_key: common::config::ApiKey::Direct("sk-test".into()),
                base_url: "https://api.openai.com".into(),
                models,
            },
        );
        let config = crate::config::RuntimeConfig { providers: providers_map };
        let rt = Runtime::from_config(config).unwrap();
        assert!(rt.list_sessions().await.is_empty());
    }
}