#![allow(dead_code)]
//! Test harness: real PostgreSQL, a real login per user. Needs the dev database (scripts\dev-db.ps1).

use a3_core::ai::{AiProvider, Completion, CompletionRequest};
use a3_core::connect::{sign_in, ConnectionProfile};
use a3_core::model::*;
use a3_core::repo::{PgRepository, Repository};
use a3_core::{scram, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub const PASSWORD: &str = "test-password-1";

pub fn database_url() -> String {
    if let Ok(u) = std::env::var("DATABASE_URL") {
        return u;
    }
    let env = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.env")).expect("run scripts/dev-db.ps1 and create .env");
    env.lines().find_map(|l| l.strip_prefix("DATABASE_URL=")).expect("DATABASE_URL in .env").trim().to_string()
}

pub fn profile() -> ConnectionProfile {
    ConnectionProfile { host: "localhost".into(), port: 5433, database: "a3sheet".into(), ssl_mode: "disable".into(), ssl_root_cert: None }
}

pub async fn owner_pool() -> PgPool {
    PgPool::connect(&database_url()).await.expect("connect as schema owner")
}

/// Creates a real login and signs in as it.
pub async fn new_user(is_admin: bool) -> Arc<PgRepository> {
    let owner = owner_pool().await;
    let login = format!("t_{}", Uuid::new_v4().simple());
    let verifier = scram::verifier(PASSWORD);
    sqlx::query("SELECT admin_create_user($1, $2, $3, $4, $5)")
        .bind(&login)
        .bind(format!("User {}", &login[2..8]))
        .bind("t@example.com")
        .bind(&verifier)
        .bind(is_admin)
        .execute(&owner)
        .await
        .expect("create user");
    owner.close().await;
    let pool = sign_in(&profile(), &login, PASSWORD).await.expect("sign in");
    Arc::new(PgRepository::open(pool).await.expect("open repo"))
}

/// An owner with a project and a member of it.
pub struct Team {
    pub owner: Arc<PgRepository>,
    pub member: Arc<PgRepository>,
    pub outsider: Arc<PgRepository>,
    pub project: Project,
}

pub async fn team() -> Team {
    use a3_core::repo::UserRepo;
    let owner = new_user(false).await;
    let member = new_user(false).await;
    let outsider = new_user(false).await;
    let code = format!("T{}", &Uuid::new_v4().simple().to_string()[..8]);
    let project = owner.create_project("Test project", &code, "").await.unwrap();
    owner.add_project_member(project.id, member.me().id).await.unwrap();
    Team { owner, member, outsider, project }
}

pub async fn new_sheet(repo: &PgRepository, project: Uuid) -> SheetDetail {
    use a3_core::repo::SheetRepo;
    let id = repo.create_sheet(&NewSheet { title: "Line 4 scrap".into(), project_id: project, ..Default::default() }).await.unwrap();
    repo.sheet_detail(id).await.unwrap()
}

pub fn doc(text: &str) -> Value {
    json!({"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": text}]}]})
}

pub async fn fill_step(repo: &PgRepository, sheet: Uuid, letter: &str, text: &str) -> Step {
    use a3_core::repo::SheetRepo;
    let d = repo.sheet_detail(sheet).await.unwrap();
    let s = d.steps.iter().find(|s| s.letter == letter).unwrap();
    repo.save_step(s.id, s.version, &doc(text)).await.unwrap()
}

pub fn step_of<'a>(d: &'a SheetDetail, letter: &str) -> &'a Step {
    d.steps.iter().find(|s| s.letter == letter).unwrap()
}

/// Adds a text attachment and reads it in-process, so its passages exist.
pub async fn add_text_file(repo: Arc<PgRepository>, sheet: Uuid, name: &str, text: &str) -> Attachment {
    use a3_core::repo::AttachmentRepo;
    use a3_core::store::{AttachmentStore, PgAttachmentStore};
    let store = PgAttachmentStore::new(repo.pool().clone());
    let blob = store.put_bytes(text.as_bytes()).await.unwrap();
    let out = repo.add_attachment(sheet, name, &blob.hash, "txt").await.unwrap();
    let segs = a3_core::extract::ExtractedSegment_for_tests(text);
    repo.replace_segments(out.attachment.id, &segs).await.unwrap();
    repo.set_read_state(out.attachment.id, "read", None).await.unwrap();
    repo.attachment(out.attachment.id).await.unwrap()
}

/// Plays back scripted replies and records every request it receives.
pub struct MockProvider {
    pub replies: Mutex<VecDeque<String>>,
    pub seen: Mutex<Vec<CompletionRequest>>,
    /// Pages a search would have returned, handed back with every reply.
    pub web_sources: Vec<a3_core::ai::WebSource>,
}

impl MockProvider {
    pub fn new(replies: Vec<Value>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into_iter().map(|v| v.to_string()).collect()),
            seen: Mutex::new(Vec::new()),
            web_sources: Vec::new(),
        })
    }
    pub fn with_web(replies: Vec<Value>, web_sources: Vec<a3_core::ai::WebSource>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into_iter().map(|v| v.to_string()).collect()),
            seen: Mutex::new(Vec::new()),
            web_sources,
        })
    }
    pub fn raw(replies: Vec<&str>) -> Arc<Self> {
        Arc::new(Self { replies: Mutex::new(replies.into_iter().map(str::to_string).collect()), seen: Mutex::new(Vec::new()), web_sources: Vec::new() })
    }
    pub fn requests(&self) -> Vec<CompletionRequest> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait]
impl AiProvider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }
    fn model(&self) -> &str {
        "mock-1"
    }
    async fn complete(&self, req: &CompletionRequest) -> Result<Completion> {
        self.seen.lock().unwrap().push(req.clone());
        let text = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| a3_core::CoreError::ai("mock ran out of replies"))?;
        Ok(Completion { text, input_tokens: 10, output_tokens: 5, web_sources: self.web_sources.clone() })
    }
    fn supports_web(&self) -> bool {
        true
    }
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec!["mock-1".into()])
    }
}

pub fn service(repo: Arc<PgRepository>, provider: Arc<MockProvider>) -> a3_core::ai::AiService {
    let r: Arc<dyn Repository> = repo;
    let p: Arc<dyn AiProvider> = provider;
    a3_core::ai::AiService::new(r, p)
}
