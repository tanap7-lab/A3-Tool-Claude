//! Everything in the A3 Sheet app that does not need a window: rules, database access,
//! file reading, AI jobs with their guardrails, and export. `src-tauri` is a thin shell over this.

pub mod ai;
pub mod connect;
pub mod domain;
pub mod email;
pub mod error;
pub mod export;
pub mod extract;
pub mod model;
pub mod pipeline;
pub mod repo;
pub mod richtext;
pub mod scram;
pub mod store;

pub use error::{CoreError, Result};

/// The schema version this build expects. Checked at sign-in.
pub const SCHEMA_VERSION: i32 = 1;
