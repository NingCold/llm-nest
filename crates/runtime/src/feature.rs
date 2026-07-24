use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use llm::LlmClient;
use tokio::sync::RwLock;

use crate::error::Result;
use crate::event::RuntimeEvent;
use crate::event_bus::EventBus;
use crate::session_manager::SessionManager;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone)]
pub struct FeatureContext {
    pub sessions: Arc<RwLock<SessionManager>>,
    pub llm: Arc<dyn LlmClient>,
    pub events: EventBus<RuntimeEvent>,
}

pub trait Feature: Send + Sync {
    fn id(&self) -> &'static str;
    fn as_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
    fn initialize(self: Arc<Self>, ctx: FeatureContext) -> BoxFuture<'static, Result<()>>;
    fn shutdown(self: Arc<Self>) -> BoxFuture<'static, Result<()>>;
}

pub struct FeatureRegistry {
    features: HashMap<&'static str, Arc<dyn Feature>>,
}

impl FeatureRegistry {
    pub fn new() -> Self {
        Self {
            features: HashMap::new(),
        }
    }

    pub fn register(&mut self, feature: Arc<dyn Feature>) {
        self.features.insert(feature.id(), feature);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Feature>> {
        self.features.get(id).cloned()
    }

    pub fn get_by_id<T: Feature + 'static>(&self, id: &str) -> Option<Arc<T>> {
        self.features
            .get(id)
            .and_then(|f| f.clone().as_any().downcast::<T>().ok())
    }

    pub async fn initialize_all(&self, ctx: FeatureContext) -> Result<()> {
        for feature in self.features.values() {
            feature.clone().initialize(ctx.clone()).await?;
        }
        Ok(())
    }

    pub async fn shutdown_all(&self) -> Result<()> {
        for feature in self.features.values() {
            feature.clone().shutdown().await?;
        }
        Ok(())
    }
}

impl Default for FeatureRegistry {
    fn default() -> Self {
        Self::new()
    }
}