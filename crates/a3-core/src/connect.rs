//! Connection profiles and sign-in. Each user signs in with their own PostgreSQL login (D1).

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};
use std::path::PathBuf;
use std::time::Duration;

/// The small profile file an admin hands out. It holds no credentials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    pub host: String,
    pub port: u16,
    pub database: String,
    /// `verify-full` in the cloud. `disable` is accepted only for a localhost server.
    pub ssl_mode: String,
    #[serde(default)]
    pub ssl_root_cert: Option<PathBuf>,
}

impl ConnectionProfile {
    pub fn validate(&self) -> Result<()> {
        let local = matches!(self.host.as_str(), "localhost" | "127.0.0.1" | "::1");
        match self.ssl_mode.as_str() {
            "verify-full" => Ok(()),
            "disable" if local => Ok(()),
            "disable" => Err(CoreError::invalid("A remote database must use TLS with verify-full.")),
            other => Err(CoreError::invalid(format!("Unknown TLS mode: {other}"))),
        }
    }
}

pub async fn sign_in(profile: &ConnectionProfile, login: &str, password: &str) -> Result<PgPool> {
    profile.validate()?;
    let mut opts = PgConnectOptions::new()
        .host(&profile.host)
        .port(profile.port)
        .database(&profile.database)
        .username(login)
        .password(password)
        .application_name("a3-sheet")
        .ssl_mode(if profile.ssl_mode == "verify-full" { PgSslMode::VerifyFull } else { PgSslMode::Disable });
    if let Some(cert) = &profile.ssl_root_cert {
        opts = opts.ssl_root_cert(cert);
    }
    // Section 5: each client holds at most 3 connections, released when idle.
    PgPoolOptions::new()
        .max_connections(3)
        .min_connections(0)
        .idle_timeout(Duration::from_secs(60))
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(opts)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db) if matches!(db.code().as_deref(), Some("28P01") | Some("28000")) => {
                CoreError::new("BAD_LOGIN", "That login name or password is not right.")
            }
            sqlx::Error::Database(_) => e.into(),
            sqlx::Error::Tls(t) => CoreError::new("OFFLINE", format!("Secure connection failed: {t}")),
            _ => CoreError::new("OFFLINE", "Could not reach the database. Check the connection and try again."),
        })
}
