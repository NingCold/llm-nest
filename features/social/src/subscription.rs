//! Durable replay and bounded live delivery shared by HTTP and desktop IPC.
use crate::{Result, SocialFeature, wire};
use social::EventCursor;
use std::{sync::Arc, time::Duration, vec::IntoIter};
use tokio::sync::broadcast;

pub struct SocialSubscription {
    feature: Arc<SocialFeature>,
    cursor: EventCursor,
    pending: IntoIter<events::SocialEvent>,
    live: broadcast::Receiver<wire::LiveEvent>,
    tick: tokio::time::Interval,
}

impl SocialFeature {
    pub async fn subscribe(self: &Arc<Self>, after: EventCursor) -> Result<SocialSubscription> {
        // Listen first so generation started during the durable read is observable.
        let live = self.subscribe_live();
        let pending = self.events(after).await?.into_iter();
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Ok(SocialSubscription {
            feature: self.clone(),
            cursor: after,
            pending,
            live,
            tick,
        })
    }
}

impl SocialSubscription {
    /// Dropping a subscription releases its receiver, never the model run.
    pub async fn next(&mut self) -> Result<Option<wire::SubscriptionEvent>> {
        loop {
            if self.feature.stop.is_cancelled() {
                return Ok(None);
            }
            if let Some(event) = self.pending.next() {
                self.cursor = event.cursor;
                return Ok(Some(wire::SubscriptionEvent::Change { event }));
            }
            tokio::select! {
                _ = self.feature.stop.cancelled() => return Ok(None),
                _ = self.tick.tick() => {
                    self.pending = self.feature.events(self.cursor).await?.into_iter();
                },
                event = self.live.recv() => return Ok(match event {
                    Ok(event) => Some(wire::SubscriptionEvent::Generation { event }),
                    Err(broadcast::error::RecvError::Lagged(_)) => Some(wire::SubscriptionEvent::Resync),
                    Err(broadcast::error::RecvError::Closed) => None,
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::runtime::Runtime;
    use social::{CommandId, ConversationId};

    async fn fixture() -> (tempfile::TempDir, Runtime, Arc<SocialFeature>) {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("llmn.toml");
        std::fs::write(&config, "[providers]\n").unwrap();
        let runtime = Runtime::open(&config, directory.path().join("data"))
            .await
            .unwrap();
        let feature = Arc::new(SocialFeature::new());
        runtime.register_feature(feature.clone()).await;
        runtime.initialize_features().await.unwrap();
        (directory, runtime, feature)
    }
    async fn contact(feature: &SocialFeature, name: &str) -> wire::Account {
        feature
            .create_ai(wire::AiInput {
                request_id: CommandId::new(),
                name: name.into(),
                bio: String::new(),
                persona: "fixture".into(),
                binding: None,
            })
            .await
            .unwrap()
            .account
    }

    #[tokio::test]
    async fn durable_gap_replay_disconnect_and_shutdown() {
        let (_directory, runtime, feature) = fixture().await;
        let cursor = feature.init().await.unwrap().cursor;
        let created = contact(&feature, "between init and subscribe").await;
        let mut subscription = feature.subscribe(cursor).await.unwrap();
        let Some(wire::SubscriptionEvent::Change { event }) = subscription.next().await.unwrap()
        else {
            panic!("durable change");
        };
        assert!(
            matches!(event.change, events::SocialChange::ActorChanged { actor_id, .. } if actor_id == created.id)
        );
        let replay_cursor = event.cursor;
        drop(subscription);
        assert!(!feature.stop.is_cancelled());
        let mut subscription = feature.subscribe(replay_cursor).await.unwrap();
        // No repeated durable event; new commits are observed by the common poller.
        let created = contact(&feature, "after reconnect").await;
        let event = tokio::time::timeout(Duration::from_secs(1), subscription.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            matches!(event, wire::SubscriptionEvent::Change { event: events::SocialEvent { change: events::SocialChange::ActorChanged { actor_id, .. }, .. } } if actor_id == created.id)
        );
        runtime.shutdown_features().await.unwrap();
        assert!(subscription.next().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn bounded_live_overflow_requests_resync_and_keeps_durable_cursor() {
        let (_directory, runtime, feature) = fixture().await;
        let cursor = feature.init().await.unwrap().cursor;
        let mut subscription = feature.subscribe(cursor).await.unwrap();
        for i in 0..300 {
            feature
                .live
                .send(wire::LiveEvent::Notice {
                    conversation_id: ConversationId::new(),
                    error: events::SocialFailure {
                        code: "fixture".into(),
                        message: i.to_string(),
                    },
                })
                .unwrap();
        }
        assert!(matches!(
            subscription.next().await.unwrap(),
            Some(wire::SubscriptionEvent::Resync)
        ));
        assert_eq!(subscription.cursor, cursor);
        assert!(matches!(
            subscription.next().await.unwrap(),
            Some(wire::SubscriptionEvent::Generation { .. })
        ));
        runtime.shutdown_features().await.unwrap();
    }
}
