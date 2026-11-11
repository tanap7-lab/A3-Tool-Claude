//! `Repository` is the boundary around the database (section 5). Everything else depends on this
//! trait, so moving to an API server later (if D1 changes) replaces only `PgRepository`.

mod ai;
mod attachments;
mod dashboard;
mod sheets;
mod users;
mod wall;

use crate::error::Result;
use crate::model::*;
use async_trait::async_trait;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

#[async_trait]
pub trait UserRepo: Send + Sync {
    fn me(&self) -> &AppUser;
    async fn schema_version(&self) -> Result<i32>;
    async fn audit(&self, object_type: &str, object_id: Option<Uuid>, sheet_id: Option<Uuid>, action: &str, reason: Option<&str>, detail: Value) -> Result<()>;
    async fn list_users(&self) -> Result<Vec<AppUser>>;
    async fn update_my_account(&self, display_name: &str, email: &str, default_project_id: Option<Uuid>) -> Result<AppUser>;
    async fn set_last_project(&self, project_id: Uuid) -> Result<()>;
    async fn change_my_password(&self, new_password: &str) -> Result<()>;
    async fn admin_create_user(&self, login: &str, name: &str, email: &str, password: &str, is_admin: bool) -> Result<Uuid>;
    async fn admin_set_active(&self, user: Uuid, active: bool) -> Result<()>;
    async fn admin_set_admin(&self, user: Uuid, is_admin: bool) -> Result<()>;
    async fn admin_reset_password(&self, user: Uuid, password: &str) -> Result<()>;
    async fn get_settings(&self) -> Result<Settings>;
    async fn set_settings(&self, settings: &Settings) -> Result<()>;
    async fn list_projects(&self, only_mine: bool) -> Result<Vec<Project>>;
    async fn create_project(&self, name: &str, code: &str, description: &str) -> Result<Project>;
    async fn set_project_ai(&self, project: Uuid, enabled: bool) -> Result<()>;
    async fn set_project_archived(&self, project: Uuid, archived: bool) -> Result<()>;
    async fn project_members(&self, project: Uuid) -> Result<Vec<AppUser>>;
    async fn add_project_member(&self, project: Uuid, user: Uuid) -> Result<()>;
    async fn remove_project_member(&self, project: Uuid, user: Uuid) -> Result<()>;
    async fn pick_values(&self, project: Option<Uuid>) -> Result<Vec<PickValue>>;
    async fn add_pick_value(&self, list_name: &str, value: &str, project: Option<Uuid>) -> Result<()>;
    async fn remove_pick_value(&self, id: Uuid) -> Result<()>;
}

#[async_trait]
pub trait SheetRepo: Send + Sync {
    async fn list_sheets(&self, filter: &SheetFilter) -> Result<SheetPage>;
    async fn sheet_detail(&self, sheet: Uuid) -> Result<SheetDetail>;
    async fn sheet_id_by_number(&self, number: &str) -> Result<Option<Uuid>>;
    async fn create_sheet(&self, new: &NewSheet) -> Result<Uuid>;
    async fn update_sheet(&self, sheet: Uuid, patch: &SheetPatch) -> Result<()>;
    async fn withdraw_sheet(&self, sheet: Uuid, reason: &str) -> Result<()>;
    async fn save_step(&self, step: Uuid, expected_version: i32, content: &Value) -> Result<Step>;
    async fn apply_summary(&self, sheet: Uuid, steps: &[SummaryStep]) -> Result<Vec<Step>>;
    async fn acquire_lock(&self, step: Uuid) -> Result<Step>;
    async fn release_lock(&self, step: Uuid) -> Result<()>;
    async fn close_step(&self, step: Uuid) -> Result<()>;
    async fn reopen_step(&self, step: Uuid, reason: &str) -> Result<()>;
    async fn snapshots(&self, sheet: Uuid) -> Result<Vec<Snapshot>>;
    async fn restore_snapshot(&self, snapshot: Uuid) -> Result<()>;
    async fn history(&self, sheet: Uuid) -> Result<Vec<AuditRow>>;
    async fn snapshot_count(&self, sheet: Uuid) -> Result<i64>;
}

#[async_trait]
pub trait AttachmentRepo: Send + Sync {
    async fn add_attachment(&self, sheet: Uuid, file_name: &str, blob_hash: &str, file_type: &str) -> Result<AddOutcome>;
    async fn list_attachments(&self, sheet: Uuid) -> Result<Vec<Attachment>>;
    async fn attachment(&self, id: Uuid) -> Result<Attachment>;
    async fn set_read_state(&self, id: Uuid, state: &str, error: Option<&str>) -> Result<()>;
    async fn set_ai_state(&self, id: Uuid, state: &str, error: Option<&str>, note: Option<&str>) -> Result<()>;
    async fn set_attachment_summary(&self, id: Uuid, summary: &str, confirms: i32) -> Result<()>;
    async fn set_excluded_sheets(&self, id: Uuid, sheets: &[String]) -> Result<()>;
    async fn delete_attachment(&self, id: Uuid) -> Result<()>;
    async fn restore_attachment(&self, id: Uuid) -> Result<()>;
    async fn unfinished_attachments(&self) -> Result<Vec<Attachment>>;
    async fn replace_segments(&self, attachment: Uuid, segments: &[crate::extract::ExtractedSegment]) -> Result<()>;
    async fn segments(&self, attachment: Uuid) -> Result<Vec<Segment>>;
    async fn segment(&self, id: Uuid) -> Result<Segment>;
    async fn set_segment_from_image(&self, id: Uuid, text: &str) -> Result<()>;
    async fn source_view(&self, segment: Uuid) -> Result<SourceView>;
    /// Source passages ranked by relevance to `terms`. Returns the visible total as well.
    async fn search_passages(&self, sheet: Uuid, attachment: Option<Uuid>, terms: &str, limit: i64) -> Result<(Vec<Passage>, i64)>;
    async fn purge_deleted(&self) -> Result<i32>;
}

#[async_trait]
pub trait AiRepo: Send + Sync {
    async fn insert_run(&self, run: &NewRun) -> Result<Uuid>;
    async fn facts(&self, sheet: Uuid) -> Result<Vec<Fact>>;
    async fn insert_proposal(&self, p: &NewProposal) -> Result<Uuid>;
    async fn proposals(&self, sheet: Uuid, pending_only: bool) -> Result<Vec<Proposal>>;
    /// Rejected or already decided points from this source file, to avoid proposing them again (FR-IMP-07).
    async fn known_norm_keys(&self, sheet: Uuid, blob_hash: Option<&str>) -> Result<Vec<String>>;
    async fn decide_proposal(&self, id: Uuid, decision: &ProposalDecision) -> Result<()>;
    /// FR-IMP-10: the no-AI path, a passage placed in a step with its source chip intact.
    async fn add_passage_to_step(&self, step: Uuid, expected_version: i32, segment: Uuid, text: &str) -> Result<Step>;
    async fn replace_findings(&self, sheet: Uuid, findings: &[NewFinding]) -> Result<Vec<Finding>>;
    async fn findings(&self, sheet: Uuid, open_only: bool) -> Result<Vec<Finding>>;
    async fn dismiss_finding(&self, id: Uuid) -> Result<()>;
    async fn dismissed_finding_keys(&self, sheet: Uuid) -> Result<Vec<(String, String, String)>>;
    async fn messages(&self, sheet: Uuid) -> Result<Vec<AiMessage>>;
    async fn insert_message(&self, sheet: Uuid, role: &str, text: &str, sources: &Value) -> Result<Uuid>;
}

#[async_trait]
pub trait WallRepo: Send + Sync {
    async fn list_wall(&self, filter: &WallFilter) -> Result<Vec<Warning>>;
    async fn warning(&self, id: Uuid) -> Result<Warning>;
    async fn warnings_for_sheet(&self, sheet: Uuid) -> Result<Vec<Warning>>;
    async fn warning_updates(&self, warning: Uuid) -> Result<Vec<WarningUpdate>>;
    async fn post_warning(&self, new: &NewWarning) -> Result<Uuid>;
    async fn add_warning_update(&self, warning: Uuid, text: &str) -> Result<()>;
    async fn withdraw_warning(&self, warning: Uuid, reason: &str) -> Result<()>;
    async fn set_warning_read(&self, warning: Uuid, read: bool) -> Result<()>;
    async fn create_task(&self, new: &NewTask) -> Result<Uuid>;
    async fn tasks_for_warning(&self, warning: Uuid) -> Result<Vec<Task>>;
    async fn close_task(&self, task: Uuid, result: &str) -> Result<()>;
    async fn create_linked_a3(&self, task: Uuid, project: Uuid) -> Result<Uuid>;
}

#[async_trait]
pub trait DashboardRepo: Send + Sync {
    async fn my_work(&self) -> Result<MyWork>;
    async fn management(&self, filter: &MgmtFilter) -> Result<Management>;
}

pub trait Repository: UserRepo + SheetRepo + AttachmentRepo + AiRepo + WallRepo + DashboardRepo {}
impl<T: UserRepo + SheetRepo + AttachmentRepo + AiRepo + WallRepo + DashboardRepo> Repository for T {}

/// Direct PostgreSQL access under the signed-in user's own login.
pub struct PgRepository {
    pool: PgPool,
    me: AppUser,
}

impl PgRepository {
    /// Runs the checks that make a sign-in valid: an active account and a matching schema version.
    pub async fn open(pool: PgPool) -> Result<Self> {
        let version = sqlx::query_scalar!("SELECT max(version) FROM schema_meta").fetch_one(&pool).await?.unwrap_or(0);
        if version != crate::SCHEMA_VERSION {
            return Err(crate::CoreError::new(
                "SCHEMA_MISMATCH",
                format!(
                    "This app expects database version {} but the database is at {version}. Please update the app or ask your admin.",
                    crate::SCHEMA_VERSION
                ),
            ));
        }
        let me = sqlx::query_as!(
            AppUser,
            "SELECT id, role_name, display_name, email, is_admin, active, default_project_id, last_project_id
             FROM app_user WHERE role_name = session_user AND active"
        )
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| crate::CoreError::new("NO_ACCOUNT", "This login has no active A3 Sheet account. Ask your admin."))?;
        Ok(Self { pool, me })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
