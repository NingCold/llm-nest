use std::sync::Arc;

use common::{Message, SessionId};
use tokio::sync::broadcast;
use tokio::sync::RwLock;

use crate::command::Command;
use crate::error::{Result, RuntimeError};
use crate::event::RuntimeEvent;
use crate::event_bus::EventBus;
use crate::feature::{Feature, FeatureContext, FeatureRegistry};
use crate::llm_service::RuntimeLlmClient;
use crate::model_router::ModelRouter;
use crate::provider_manager::ProviderManager;
use crate::session::Session;
use crate::session_manager::SessionManager;

#[derive(Clone)]
pub struct Runtime {
    sessions: Arc<RwLock<SessionManager>>,
    llm: Arc<dyn llm::LlmClient>,
    event_bus: EventBus<RuntimeEvent>,
    features: Arc<RwLock<FeatureRegistry>>,
}

impl Runtime {
    pub fn new(
        sessions: SessionManager,
        event_bus: EventBus<RuntimeEvent>,
        provider_mgr: Arc<ProviderManager>,
    ) -> Self {
        let sessions = Arc::new(RwLock::new(sessions));
        let router = Arc::new(ModelRouter::new(provider_mgr));
        let llm = Arc::new(RuntimeLlmClient::new(router));

        Self {
            sessions,
            llm,
            event_bus,
            features: Arc::new(RwLock::new(FeatureRegistry::new())),
        }
    }

    pub fn context(&self) -> FeatureContext {
        FeatureContext {
            sessions: self.sessions.clone(),
            llm: self.llm.clone(),
            events: self.event_bus.clone(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.event_bus.subscribe()
    }

    pub fn emit(&self, event: RuntimeEvent) {
        self.event_bus.publish(event);
    }

    pub async fn register_feature(&self, feature: Arc<dyn Feature>) {
        let mut features = self.features.write().await;
        features.register(feature);
    }

    pub async fn initialize_features(&self) -> Result<()> {
        let ctx = self.context();
        let features = self.features.read().await;
        features.initialize_all(ctx).await
    }

    pub async fn shutdown_features(&self) -> Result<()> {
        let features = self.features.read().await;
        features.shutdown_all().await
    }

    pub async fn get_feature<T: Feature + 'static>(&self, id: &str) -> Option<Arc<T>> {
        self.features.read().await.get_by_id(id)
    }

    // --- Session management ---

    pub async fn create_session(&self, title: Option<String>) -> SessionId {
        let id = self.sessions.write().await.create(title);
        self.event_bus.publish(RuntimeEvent::SessionCreated { session_id: id });
        id
    }

    pub async fn delete_session(&self, session_id: SessionId) -> Result<()> {
        self.sessions
            .write()
            .await
            .remove(&session_id)
            .ok_or(RuntimeError::SessionNotFound(session_id))?;
        self.event_bus.publish(RuntimeEvent::SessionDeleted { session_id });
        Ok(())
    }

    pub async fn rename_session(&self, session_id: SessionId, title: String) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&session_id)
            .ok_or(RuntimeError::SessionNotFound(session_id))?;
        session.set_title(title);
        drop(sessions);
        self.event_bus.publish(RuntimeEvent::SessionChanged { session_id });
        Ok(())
    }

    pub async fn get_session(&self, session_id: &SessionId) -> Option<Session> {
        self.sessions.read().await.get(session_id).cloned()
    }

    pub async fn list_sessions(&self) -> Vec<SessionId> {
        self.sessions.read().await.iter().map(|(id, _)| *id).collect()
    }

    pub async fn push_message(&self, session_id: &SessionId, message: Message) {
        self.sessions.write().await.push_message(session_id, message);
    }

    pub async fn get_messages(&self, session_id: &SessionId) -> Vec<Message> {
        self.sessions.read().await.get_messages(session_id)
    }

    // --- Command execution ---

    pub async fn execute(&self, command: Command) -> Result<()> {
        match command {
            Command::CreateSession { title } => {
                self.create_session(title).await;
            }
            Command::DeleteSession { session_id } => {
                self.delete_session(session_id).await?;
            }
            Command::RenameSession { session_id, title } => {
                self.rename_session(session_id, title).await?;
            }
            _ => {}
        }
        Ok(())
    }
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime").finish()
    }
}