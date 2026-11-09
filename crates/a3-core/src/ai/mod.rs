//! AI does a small set of fixed jobs inside one A3 (section 9). Memory belongs to the A3 and is
//! rebuilt from the database on every call. Rust checks every output before anyone sees it.

pub mod context;
pub mod guard;
pub mod jobs;
pub mod prompts;
pub mod provider;

pub use jobs::*;
pub use provider::{make_provider, AiConfig, AiProvider, Completion, CompletionRequest};
