use chrono::{DateTime, Utc};
use std::sync::RwLock;

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
/// Controllable time for deterministic storage and future scheduling tests.
pub struct ManualClock(RwLock<DateTime<Utc>>);
impl ManualClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self(RwLock::new(now))
    }
    pub fn set(&self, now: DateTime<Utc>) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = now;
    }
}
impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.read().unwrap_or_else(|e| e.into_inner())
    }
}
