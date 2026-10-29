//! Neutral social domain contracts. No provider, transport, or persistence dependencies.
pub mod attachment;
pub mod consciousness;
pub mod contact_intent;
pub mod conversation;
pub mod id;
pub mod identity;
pub mod message;

pub use consciousness::*;
pub use contact_intent::*;
pub use conversation::*;
pub use id::*;
pub use identity::*;
pub use message::*;
