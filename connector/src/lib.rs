//! Shared social subscription protocol for HTTP and desktop consumers.
//! Includes independently usable cloud node HTTP/WSS transport.

pub mod social_event;
pub use social_event::{SocialChange, SocialEvent};
pub mod social_generation;
pub use social_generation::{SocialFailure, SocialGenerationEvent};
mod subscription;
pub use subscription::{LiveEvent, SubscriptionEvent};
pub mod node;
