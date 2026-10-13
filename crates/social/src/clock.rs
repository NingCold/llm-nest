use chrono::{DateTime, Utc};
use std::{future::Future, pin::Pin, sync::RwLock};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    fn wait_until(&self, deadline: DateTime<Utc>) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            while self.now() < deadline {
                let delay = (deadline - self.now()).to_std().unwrap_or_default();
                // Recheck UTC after sleep/resume or a system-clock adjustment.
                tokio::time::sleep(delay.min(std::time::Duration::from_secs(30))).await;
            }
        })
    }
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
/// Controllable time and wakeups for deterministic storage and scheduling tests.
pub struct ManualClock(RwLock<DateTime<Utc>>, tokio::sync::Notify);
impl ManualClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self(RwLock::new(now), tokio::sync::Notify::new())
    }
    pub fn set(&self, now: DateTime<Utc>) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = now;
        self.1.notify_waiters();
    }
}
impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.read().unwrap_or_else(|e| e.into_inner())
    }
    fn wait_until(&self, deadline: DateTime<Utc>) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            loop {
                let changed = self.1.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if self.now() >= deadline {
                    return;
                }
                changed.await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn manual_wait_observes_advances_without_sleeping_and_rollback_does_not_fire() {
        let clock = ManualClock::new("2026-10-13T00:00:00Z".parse().unwrap());
        let deadline = clock.now() + chrono::Duration::seconds(1);
        let wait = clock.wait_until(deadline);
        tokio::pin!(wait);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1), &mut wait)
                .await
                .is_err()
        );
        clock.set(clock.now() - chrono::Duration::hours(1));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1), &mut wait)
                .await
                .is_err()
        );
        clock.set(deadline);
        tokio::time::timeout(std::time::Duration::from_secs(1), wait)
            .await
            .unwrap();
    }
}
