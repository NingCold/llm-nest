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
            .map(|(id, _)| id.clone())
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