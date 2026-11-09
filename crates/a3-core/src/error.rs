use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

pub type Result<T> = std::result::Result<T, CoreError>;

/// An error with one plain message for the user. `code` lets the interface offer the right next step.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct CoreError {
    pub code: String,
    pub message: String,
}

impl CoreError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
    pub fn ai(message: impl Into<String>) -> Self {
        Self::new("AI_FAILED", message)
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("INVALID", message)
    }
    pub fn not_found(what: &str) -> Self {
        Self::new("NOT_FOUND", format!("{what} not found."))
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("STALE", message)
    }
}

impl Serialize for CoreError {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("CoreError", 2)?;
        st.serialize_field("code", &self.code)?;
        st.serialize_field("message", &self.message)?;
        st.end()
    }
}

impl From<sqlx::Error> for CoreError {
    fn from(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::Database(db) => {
                let msg = db.message();
                // Rules raised in the database arrive as 'A3_CODE|Plain message'.
                if let Some((code, text)) = msg.split_once('|') {
                    if code.starts_with("A3_") {
                        return Self::new(code, text);
                    }
                }
                match db.code().as_deref() {
                    Some("42501") => Self::new(
                        "NO_PERMISSION",
                        "You don't have permission to do that. Only project members can change this A3.",
                    ),
                    Some("23505") => Self::new("DUPLICATE", "That value is already in use."),
                    Some("23514") => Self::new("INVALID", "One of the values is not allowed."),
                    _ => {
                        tracing::error!(code = ?db.code(), "database error");
                        Self::new("DB", format!("The database reported a problem: {msg}"))
                    }
                }
            }
            sqlx::Error::RowNotFound => Self::new("NOT_FOUND", "That item no longer exists."),
            sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Tls(_) => {
                Self::new("OFFLINE", "The connection to the database was lost.")
            }
            _ => Self::new("DB", format!("Database error: {e}")),
        }
    }
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        Self::new("IO", format!("File error: {e}"))
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("DATA", format!("Data error: {e}"))
    }
}

impl From<reqwest::Error> for CoreError {
    fn from(e: reqwest::Error) -> Self {
        // The message of a reqwest error can include the URL but never request headers, so keys stay out.
        Self::ai(format!("Could not reach the AI provider: {}", e.without_url()))
    }
}
