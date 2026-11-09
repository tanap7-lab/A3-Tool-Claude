//! What the running app holds in memory: the signed-in session, local files, and the job limiter.

use a3_core::ai::{make_provider, AiConfig, AiProvider, AiService};
use a3_core::connect::ConnectionProfile;
use a3_core::pipeline::{ChildProcessReader, Pipeline};
use a3_core::repo::{PgRepository, Repository};
use a3_core::store::{AttachmentStore, PgAttachmentStore};
use a3_core::{CoreError, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Manager};
use tokio::sync::Semaphore;

pub const KEYRING_SERVICE: &str = "A3 Sheet";

#[derive(Clone)]
pub struct Session {
    pub repo: Arc<PgRepository>,
    pub store: Arc<PgAttachmentStore>,
    pub login: String,
}

pub struct AppState {
    pub session: RwLock<Option<Session>>,
    /// A link that arrived before sign-in, opened once sign-in finishes (FR-LNK-03).
    pub pending_link: Mutex<Option<String>>,
    /// Background reading stays light: at most two files at a time.
    pub job_slots: Arc<Semaphore>,
}

impl AppState {
    pub fn new() -> Self {
        Self { session: RwLock::new(None), pending_link: Mutex::new(None), job_slots: Arc::new(Semaphore::new(2)) }
    }

    pub fn session(&self) -> Result<Session> {
        self.session
            .read()
            .expect("session lock")
            .clone()
            .ok_or_else(|| CoreError::new("SIGNED_OUT", "Please sign in."))
    }

    pub fn repo(&self) -> Result<Arc<PgRepository>> {
        Ok(self.session()?.repo)
    }
}

pub fn config_dir(app: &AppHandle) -> Result<PathBuf> {
    let d = app.path().app_config_dir().map_err(|e| CoreError::new("IO", e.to_string()))?;
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn recovery_dir(app: &AppHandle) -> Result<PathBuf> {
    let d = app.path().app_local_data_dir().map_err(|e| CoreError::new("IO", e.to_string()))?.join("recovery");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn temp_dir(sub: &str) -> Result<PathBuf> {
    let d = std::env::temp_dir().join("A3Sheet").join(sub);
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub fn load_profile(app: &AppHandle) -> Result<Option<ConnectionProfile>> {
    Ok(read_json(&config_dir(app)?.join("profile.json")))
}

pub fn save_profile(app: &AppHandle, p: &ConnectionProfile) -> Result<()> {
    p.validate()?;
    std::fs::write(config_dir(app)?.join("profile.json"), serde_json::to_vec_pretty(p)?)?;
    Ok(())
}

pub fn load_ai_config(app: &AppHandle, login: &str) -> Result<Option<AiConfig>> {
    Ok(read_json(&config_dir(app)?.join(format!("ai-{}.json", file_safe(login)))))
}

pub fn save_ai_config(app: &AppHandle, login: &str, c: &AiConfig) -> Result<()> {
    std::fs::write(config_dir(app)?.join(format!("ai-{}.json", file_safe(login))), serde_json::to_vec_pretty(c)?)?;
    Ok(())
}

fn file_safe(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

// ---------------------------------------------------------------- Windows Credential Manager

fn entry(kind: &str, login: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("{kind}:{login}"))
        .map_err(|e| CoreError::new("KEYRING", format!("Windows Credential Manager is not available: {e}")))
}

pub fn secret_get(kind: &str, login: &str) -> Result<Option<String>> {
    match entry(kind, login)?.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(CoreError::new("KEYRING", format!("Could not read from Credential Manager: {e}"))),
    }
}

pub fn secret_set(kind: &str, login: &str, value: &str) -> Result<()> {
    entry(kind, login)?
        .set_password(value)
        .map_err(|e| CoreError::new("KEYRING", format!("Could not save to Credential Manager: {e}")))
}

pub fn secret_delete(kind: &str, login: &str) -> Result<()> {
    match entry(kind, login)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(CoreError::new("KEYRING", format!("Could not remove from Credential Manager: {e}"))),
    }
}

// ---------------------------------------------------------------- AI wiring

/// FR-AI-07: without a usable configuration, AI stays off and everything else works.
pub fn provider_for(app: &AppHandle, login: &str) -> Result<Option<(Arc<dyn AiProvider>, AiConfig)>> {
    let Some(cfg) = load_ai_config(app, login)? else { return Ok(None) };
    if cfg.provider.is_empty() || cfg.model.is_empty() {
        return Ok(None);
    }
    let key = secret_get("ai", login)?.unwrap_or_default();
    if key.is_empty() && cfg.provider != "custom" {
        return Ok(None);
    }
    let p: Arc<dyn AiProvider> = Arc::from(make_provider(&cfg, &key)?);
    Ok(Some((p, cfg)))
}

pub fn ai_service(app: &AppHandle, s: &Session) -> Result<Arc<AiService>> {
    let (p, _) = provider_for(app, &s.login)?.ok_or_else(|| CoreError::new("AI_NOT_SET", "Set up AI in Settings"))?;
    let repo: Arc<dyn Repository> = s.repo.clone();
    Ok(Arc::new(AiService::new(repo, p)))
}

pub fn pipeline_for(app: &AppHandle, s: &Session) -> Result<Pipeline> {
    let exe = std::env::current_exe()?;
    let provider = provider_for(app, &s.login).unwrap_or(None);
    let repo: Arc<dyn Repository> = s.repo.clone();
    let vision = provider.as_ref().is_some_and(|(_, c)| c.vision_ok);
    let ai = provider.map(|(p, _)| Arc::new(AiService::new(repo.clone(), p)));
    let store: Arc<dyn AttachmentStore> = s.store.clone();
    Ok(Pipeline {
        repo,
        store,
        reader: Arc::new(ChildProcessReader { exe }),
        temp_dir: temp_dir("work")?,
        ai,
        vision,
    })
}
