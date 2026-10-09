use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};
use uuid::Uuid;

macro_rules! ids {
    ($($name:ident),* $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);
        impl $name {
            pub fn new() -> Self { Self(Uuid::new_v4()) }
        }
        impl Default for $name { fn default() -> Self { Self::new() } }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
        }
        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> { Ok(Self(Uuid::parse_str(s)?)) }
        }
    )*};
}
ids!(
    ActorId,
    RelationshipId,
    ConversationId,
    MessageId,
    RunId,
    BoutId,
    MemoryId,
    SummaryId,
    BindingId,
    IntentId,
    PlanId,
    CommandId
);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("counter must be a canonical decimal string in 0..=i64::MAX")]
pub struct CounterError;

/// Shared bounded counter for revisions, sequences, epochs and event cursors.
/// Decimal-string serialization preserves exact values in JavaScript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Counter(u64);
pub type Revision = Counter;
pub type Sequence = Counter;
pub type Epoch = Counter;
pub type EventCursor = Counter;

impl Counter {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1);
    pub fn new(value: u64) -> Result<Self, CounterError> {
        if value > i64::MAX as u64 {
            return Err(CounterError);
        }
        Ok(Self(value))
    }
    pub fn get(self) -> u64 {
        self.0
    }
    pub fn next(self) -> Result<Self, CounterError> {
        Self::new(self.0.checked_add(1).ok_or(CounterError)?)
    }
}
impl fmt::Display for Counter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl Serialize for Counter {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for Counter {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let n = s.parse::<u64>().map_err(serde::de::Error::custom)?;
        if s != n.to_string() {
            return Err(serde::de::Error::custom(CounterError));
        }
        Self::new(n).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counters_are_lossless_and_bounded() {
        let n = Counter::new(9_007_199_254_740_993).unwrap();
        assert_eq!(serde_json::to_string(&n).unwrap(), "\"9007199254740993\"");
        assert_eq!(
            serde_json::from_str::<Counter>("\"9007199254740993\"").unwrap(),
            n
        );
        for s in ["1", "\"01\"", "\"-1\"", "\"9223372036854775808\""] {
            assert!(serde_json::from_str::<Counter>(s).is_err());
        }
        assert!(Counter::new(i64::MAX as u64).unwrap().next().is_err());
    }
}
