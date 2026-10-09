//! Neutral social domain contracts. No provider, transport, or persistence dependencies.
pub mod clock;
pub mod consciousness;
pub mod conversation;
pub mod id;
pub mod identity;
pub mod message;
pub mod run;

pub use clock::*;
pub use consciousness::*;
pub use conversation::*;
pub use id::*;
pub use identity::*;
pub use message::*;
pub use run::*;
