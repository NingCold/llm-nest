//! One acknowledged delivery per WebView. Slow consumers cannot grow the IPC queue.
use serde::Serialize;
use social_feature::{SocialSubscription, wire};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use uuid::Uuid;

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Delivery {
    Event {
        sequence: u32,
        event: wire::SubscriptionEvent,
    },
    Error {
        error: wire::ErrorBody,
    },
    Closed,
}

struct Entry {
    id: Uuid,
    stop: CancellationToken,
    acknowledged: u32,
    pending: Option<(u32, oneshot::Sender<()>)>,
}
#[derive(Default)]
struct Registry {
    closed: bool,
    entries: HashMap<String, Entry>,
}
#[derive(Default)]
pub struct Subscriptions {
    registry: Mutex<Registry>,
    tasks: TaskTracker,
}

pub struct Lease {
    subscriptions: Arc<Subscriptions>,
    owner: String,
    id: Uuid,
    pub stop: CancellationToken,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.subscriptions.remove(&self.owner, self.id);
    }
}
pub fn unavailable(message: impl Into<String>) -> wire::ErrorBody {
    wire::ErrorBody {
        code: "unavailable",
        message: message.into(),
        retryable: true,
    }
}
fn invalid(message: &str) -> wire::ErrorBody {
    wire::ErrorBody {
        code: "invalid_input",
        message: message.into(),
        retryable: false,
    }
}

impl Subscriptions {
    pub fn start(self: &Arc<Self>, owner: &str, id: Uuid) -> Result<Lease, wire::ErrorBody> {
        let mut registry = self.registry.lock().expect("subscription registry");
        if registry.closed {
            return Err(unavailable("应用正在关闭"));
        }
        if registry.entries.values().any(|entry| entry.id == id) {
            return Err(invalid("订阅 ID 已存在"));
        }
        let stop = CancellationToken::new();
        if let Some(previous) = registry.entries.insert(
            owner.into(),
            Entry {
                id,
                stop: stop.clone(),
                acknowledged: 0,
                pending: None,
            },
        ) {
            previous.stop.cancel();
        }
        Ok(Lease {
            subscriptions: self.clone(),
            owner: owner.into(),
            id,
            stop,
        })
    }

    pub fn acknowledge(&self, owner: &str, id: Uuid, sequence: u32) -> Result<(), wire::ErrorBody> {
        let mut registry = self.registry.lock().expect("subscription registry");
        let entry = registry
            .entries
            .get_mut(owner)
            .filter(|entry| entry.id == id)
            .ok_or_else(|| unavailable("订阅已释放，请重新连接"))?;
        if sequence <= entry.acknowledged {
            return Ok(());
        }
        if entry.pending.as_ref().map(|pending| pending.0) != Some(sequence) {
            return Err(invalid("订阅确认序号不匹配"));
        }
        let (_, sender) = entry.pending.take().expect("pending acknowledgement");
        entry.acknowledged = sequence;
        let _ = sender.send(()); // The consumer may have disconnected concurrently.
        Ok(())
    }

    pub fn remove(&self, owner: &str, id: Uuid) {
        let mut registry = self.registry.lock().expect("subscription registry");
        if registry
            .entries
            .get(owner)
            .is_some_and(|entry| entry.id == id)
        {
            registry
                .entries
                .remove(owner)
                .expect("subscription entry")
                .stop
                .cancel();
        }
    }
    pub fn release_owner(&self, owner: &str) {
        if let Some(entry) = self
            .registry
            .lock()
            .expect("subscription registry")
            .entries
            .remove(owner)
        {
            entry.stop.cancel();
        }
    }
    pub async fn shutdown(&self) {
        {
            let mut registry = self.registry.lock().expect("subscription registry");
            registry.closed = true;
            for (_, entry) in registry.entries.drain() {
                entry.stop.cancel();
            }
            self.tasks.close();
        }
        self.tasks.wait().await;
    }
    pub fn spawn(
        &self,
        lease: Lease,
        mut subscription: SocialSubscription,
        channel: tauri::ipc::Channel<Delivery>,
    ) -> Result<(), wire::ErrorBody> {
        let registry = self.registry.lock().expect("subscription registry");
        if registry.closed || lease.stop.is_cancelled() {
            return Err(unavailable("订阅已释放"));
        }
        self.tasks.spawn(async move {
            let result = async {
                let mut sequence = 0u32;
                loop {
                    let event = tokio::select! {
                        _ = lease.stop.cancelled() => return Ok(()),
                        next = subscription.next() => next.map_err(|error| error.wire())?,
                    };
                    let Some(event) = event else {
                        return Ok(());
                    };
                    sequence = sequence
                        .checked_add(1)
                        .ok_or_else(|| unavailable("订阅序号已耗尽，请重新连接"))?;
                    let receiver = {
                        let mut registry = lease
                            .subscriptions
                            .registry
                            .lock()
                            .expect("subscription registry");
                        let Some(entry) = registry
                            .entries
                            .get_mut(&lease.owner)
                            .filter(|entry| entry.id == lease.id)
                        else {
                            return Ok(());
                        };
                        let (sender, receiver) = oneshot::channel();
                        entry.pending = Some((sequence, sender));
                        receiver
                    };
                    channel
                        .send(Delivery::Event { sequence, event })
                        .map_err(|_| unavailable("桌面事件通道已断开"))?;
                    tokio::select! {
                        biased;
                        _ = lease.stop.cancelled() => return Ok(()),
                        ack = tokio::time::timeout(Duration::from_secs(30), receiver) => {
                            ack.map_err(|_| unavailable("桌面事件确认超时"))?
                                .map_err(|_| unavailable("桌面事件订阅已释放"))?;
                        },
                    }
                }
            }
            .await;
            if let Err(error) = result {
                let _ = channel.send(Delivery::Error { error });
            }
            let _ = channel.send(Delivery::Closed);
            // Lease and Channel drop release both native and JavaScript callbacks.
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::runtime::Runtime;
    use social::CommandId;
    use social_feature::SocialFeature;

    async fn fixture() -> (tempfile::TempDir, Runtime, Arc<SocialFeature>) {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("llmn.toml");
        std::fs::write(&config, "[providers]\n").unwrap();
        let runtime =
            Runtime::from_config_social_persistent(&config, directory.path().join("data"))
                .await
                .unwrap();
        let feature = Arc::new(SocialFeature::new());
        runtime.register_feature(feature.clone()).await;
        runtime.initialize_features().await.unwrap();
        (directory, runtime, feature)
    }

    #[tokio::test]
    async fn missing_ack_times_out_and_releases_channel() {
        let (_directory, runtime, feature) = fixture().await;
        let cursor = feature.init().await.unwrap().cursor;
        feature
            .create_ai(wire::AiInput {
                request_id: CommandId::new(),
                name: "timeout".into(),
                bio: String::new(),
                persona: "fixture".into(),
                binding: None,
            })
            .await
            .unwrap();
        let subscriptions = Arc::new(Subscriptions::default());
        let id = Uuid::new_v4();
        let lease = subscriptions.start("main", id).unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let channel = tauri::ipc::Channel::new(move |body| {
            let tauri::ipc::InvokeResponseBody::Json(json) = body else {
                panic!("JSON delivery");
            };
            sender
                .send(serde_json::from_str::<serde_json::Value>(&json).unwrap())
                .unwrap();
            Ok(())
        });
        subscriptions
            .spawn(lease, feature.subscribe(cursor).await.unwrap(), channel)
            .unwrap();
        assert_eq!(receiver.recv().await.unwrap()["kind"], "event");
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(31)).await;
        let error = receiver.recv().await.unwrap();
        assert_eq!(error["kind"], "error");
        assert_eq!(error["error"]["code"], "unavailable");
        assert_eq!(receiver.recv().await.unwrap()["kind"], "closed");
        subscriptions.shutdown().await;
        runtime.shutdown_features().await.unwrap();
    }

    #[tokio::test]
    async fn replacement_reload_and_shutdown_release_only_owned_subscription() {
        let subscriptions = Arc::new(Subscriptions::default());
        let first = subscriptions.start("main", Uuid::new_v4()).unwrap();
        let second = subscriptions.start("main", Uuid::new_v4()).unwrap();
        assert!(first.stop.is_cancelled());
        drop(first);
        assert!(
            !second.stop.is_cancelled(),
            "old lease must not remove its replacement"
        );
        subscriptions.remove("other", second.id);
        assert!(!second.stop.is_cancelled());
        assert!(subscriptions.acknowledge("other", second.id, 1).is_err());
        subscriptions.release_owner("main");
        assert!(second.stop.is_cancelled());
        let third = subscriptions.start("main", Uuid::new_v4()).unwrap();
        subscriptions.shutdown().await;
        assert!(third.stop.is_cancelled());
        assert!(subscriptions.start("main", Uuid::new_v4()).is_err());
    }

    #[tokio::test]
    async fn channel_waits_for_exact_ack_and_shutdown_interrupts_wait() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("llmn.toml");
        std::fs::write(&config, "[providers]\n").unwrap();
        let runtime =
            Runtime::from_config_social_persistent(&config, directory.path().join("data"))
                .await
                .unwrap();
        let feature = Arc::new(SocialFeature::new());
        runtime.register_feature(feature.clone()).await;
        runtime.initialize_features().await.unwrap();
        let cursor = feature.init().await.unwrap().cursor;
        let subscriptions = Arc::new(Subscriptions::default());
        let id = Uuid::new_v4();
        let lease = subscriptions.start("main", id).unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let channel = tauri::ipc::Channel::new(move |body| {
            let tauri::ipc::InvokeResponseBody::Json(json) = body else {
                panic!("JSON delivery");
            };
            sender
                .send(serde_json::from_str::<serde_json::Value>(&json).unwrap())
                .unwrap();
            Ok(())
        });
        subscriptions
            .spawn(lease, feature.subscribe(cursor).await.unwrap(), channel)
            .unwrap();
        // Commits remain independent of a stalled WebView.
        for name in ["one", "two"] {
            feature
                .create_ai(wire::AiInput {
                    request_id: CommandId::new(),
                    name: name.into(),
                    bio: String::new(),
                    persona: "fixture".into(),
                    binding: None,
                })
                .await
                .unwrap();
        }
        let first = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first["sequence"], 1);
        assert_eq!(first["event"]["kind"], "change");
        assert!(
            tokio::time::timeout(Duration::from_millis(30), receiver.recv())
                .await
                .is_err()
        );
        assert_eq!(
            subscriptions.acknowledge("main", id, 2).unwrap_err().code,
            "invalid_input"
        );
        subscriptions.acknowledge("main", id, 1).unwrap();
        let second = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second["sequence"], 2);
        subscriptions.acknowledge("main", id, 1).unwrap();
        assert!(
            subscriptions.registry.lock().unwrap().entries["main"]
                .pending
                .is_some(),
            "duplicate ack cannot release next delivery"
        );
        tokio::time::timeout(Duration::from_secs(1), subscriptions.shutdown())
            .await
            .unwrap();
        assert_eq!(receiver.recv().await.unwrap()["kind"], "closed");
        assert!(!feature.init().await.unwrap().address_book.is_empty());
        runtime.shutdown_features().await.unwrap();
    }
}
