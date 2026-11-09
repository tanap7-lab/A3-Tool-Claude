//! Records passed between the database, the jobs and the interface.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub type Ts = DateTime<Utc>;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AppUser {
    pub id: Uuid,
    pub role_name: String,
    pub display_name: String,
    pub email: String,
    pub is_admin: bool,
    pub active: bool,
    pub default_project_id: Option<Uuid>,
    pub last_project_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub code: String,
    pub description: String,
    pub owner_id: Uuid,
    pub owner_name: String,
    pub ai_enabled: bool,
    pub archived_at: Option<Ts>,
    pub is_member: bool,
    pub is_owner: bool,
    pub member_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PickValue {
    pub id: Uuid,
    pub list_name: String,
    pub value: String,
    pub project_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub stalled_days: i32,
    pub link_base: String,
}

/// One row of the A3 list (FR-LST-01).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SheetRow {
    pub id: Uuid,
    pub number: String,
    pub title: String,
    pub project_id: Uuid,
    pub project_code: String,
    pub owner_id: Uuid,
    pub owner_name: String,
    pub step_strip: String,
    pub status: String,
    pub current_step: Option<String>,
    pub overdue: bool,
    pub stalled: bool,
    pub reopened: bool,
    pub withdrawn: bool,
    pub severity: Option<String>,
    pub category: Option<String>,
    pub area: Option<String>,
    pub registered_on: NaiveDate,
    pub target_close_date: Option<NaiveDate>,
    pub closed_at: Option<Ts>,
    pub updated_at: Ts,
    pub pending_review: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SheetFilter {
    pub mine: bool,
    /// `open`, `overdue`, `stalled`, `closed`, `withdrawn`
    pub quick: Option<String>,
    pub project_id: Option<Uuid>,
    pub owner_id: Option<Uuid>,
    pub category: Option<String>,
    pub area: Option<String>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub search: Option<String>,
    pub sort: Option<String>,
    pub descending: Option<bool>,
    pub page: Option<i64>,
    /// Links from the Management tab.
    pub current_step: Option<String>,
    pub age_band: Option<String>,
    pub closed_from: Option<NaiveDate>,
    pub reopened_from: Option<NaiveDate>,
    pub reopen_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetPage {
    pub rows: Vec<SheetRow>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    pub id: Uuid,
    pub number: String,
    pub title: String,
    pub project_id: Uuid,
    pub project_name: String,
    pub project_code: String,
    pub project_ai_enabled: bool,
    pub owner_id: Uuid,
    pub owner_name: String,
    pub team_member_ids: Vec<Uuid>,
    pub registered_on: NaiveDate,
    pub problem_date: Option<NaiveDate>,
    pub target_close_date: Option<NaiveDate>,
    pub category: Option<String>,
    pub severity: Option<String>,
    pub source: Option<String>,
    pub area: Option<String>,
    pub product: Option<String>,
    pub origin_warning_id: Option<Uuid>,
    pub origin_task_id: Option<Uuid>,
    pub withdrawn_at: Option<Ts>,
    pub withdrawn_reason: Option<String>,
    pub updated_at: Ts,
    pub status: String,
    pub current_step: Option<String>,
    pub closed_at: Option<Ts>,
    pub step_strip: String,
    pub overdue: bool,
    pub stalled: bool,
    pub reopened: bool,
    pub pending_review: i64,
    pub can_edit: bool,
    pub is_owner: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub id: Uuid,
    pub sheet_id: Uuid,
    pub letter: String,
    pub content: Value,
    pub plain_text: String,
    pub state: String,
    pub closed_at: Option<Ts>,
    pub version: i32,
    pub reopen_count: i32,
    pub locked_by: Option<Uuid>,
    pub locked_by_name: Option<String>,
    pub locked_at: Option<Ts>,
    pub updated_at: Ts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetDetail {
    pub sheet: Sheet,
    pub steps: Vec<Step>,
    pub link: String,
    pub open_conflicts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NewSheet {
    pub title: String,
    pub project_id: Uuid,
    pub problem_date: Option<NaiveDate>,
    pub target_close_date: Option<NaiveDate>,
    pub category: Option<String>,
    pub severity: Option<String>,
    pub source: Option<String>,
    pub area: Option<String>,
    pub product: Option<String>,
    pub origin_warning_id: Option<Uuid>,
    pub origin_task_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetPatch {
    pub title: String,
    pub project_id: Uuid,
    pub owner_id: Uuid,
    pub team_member_ids: Vec<Uuid>,
    pub problem_date: Option<NaiveDate>,
    pub target_close_date: Option<NaiveDate>,
    pub category: Option<String>,
    pub severity: Option<String>,
    pub source: Option<String>,
    pub area: Option<String>,
    pub product: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: Uuid,
    pub step_id: Uuid,
    pub letter: String,
    pub content: Value,
    pub plain_text: String,
    pub version: i32,
    pub author_name: Option<String>,
    pub reason: String,
    pub created_at: Ts,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditRow {
    pub id: i64,
    pub actor_name: Option<String>,
    pub object_type: String,
    pub action: String,
    pub reason: Option<String>,
    pub detail: Value,
    pub at: Ts,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: Uuid,
    pub sheet_id: Uuid,
    pub file_name: String,
    pub version: i32,
    pub blob_hash: String,
    pub size_bytes: i64,
    pub file_type: String,
    pub read_state: String,
    pub read_error: Option<String>,
    pub ai_state: String,
    pub ai_error: Option<String>,
    pub ai_note: Option<String>,
    pub summary: Option<String>,
    pub excluded_sheets: Vec<String>,
    pub confirms_count: i32,
    pub added_by: Uuid,
    pub added_by_name: String,
    pub added_at: Ts,
    pub deleted_at: Option<Ts>,
    /// The steps this file has fed, from accepted facts (FR-ATT-03).
    pub informs_steps: Vec<String>,
    pub sheet_names: Vec<String>,
    pub pending_images: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddOutcome {
    pub attachment: Attachment,
    /// FR-ATT-04: identical content already on this A3, so nothing new was stored.
    pub duplicate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: Uuid,
    pub attachment_id: Uuid,
    pub seq: i32,
    pub label: String,
    pub location: Value,
    pub text: String,
    pub from_image: bool,
    pub needs_image: bool,
    pub ai_visible: bool,
    pub group_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    pub segment_id: Uuid,
    pub attachment_id: Uuid,
    pub file_name: String,
    pub label: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Passage {
    pub id: Uuid,
    pub file_name: String,
    pub label: String,
    pub text: String,
    pub from_image: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub id: Uuid,
    pub letter: String,
    pub text: String,
    pub source_segment_id: Option<Uuid>,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub id: Uuid,
    pub sheet_id: Uuid,
    pub attachment_id: Option<Uuid>,
    pub letter: Option<String>,
    pub kind: String,
    pub field: Option<String>,
    pub proposed_text: String,
    pub current_text: Option<String>,
    pub source_segment_id: Option<Uuid>,
    pub source_label: Option<String>,
    pub quote: Option<String>,
    pub from_image: bool,
    pub why: Option<String>,
    pub state: String,
    pub step_closed: bool,
    pub created_at: Ts,
}

#[derive(Debug, Clone, Default)]
pub struct NewProposal {
    pub sheet_id: Uuid,
    pub attachment_id: Option<Uuid>,
    pub letter: Option<String>,
    pub kind: String,
    pub field: Option<String>,
    pub proposed_text: String,
    pub current_text: Option<String>,
    pub replaces_fact_id: Option<Uuid>,
    pub source_segment_id: Option<Uuid>,
    pub source_label: Option<String>,
    pub quote: Option<String>,
    pub from_image: bool,
    pub why: Option<String>,
    pub norm_key: String,
    pub source_blob_hash: Option<String>,
    pub run_id: Option<Uuid>,
}

/// What the user decided on one proposal card.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum ProposalDecision {
    /// Add or Update: write this text. `text` holds the user's edit when they changed it.
    Accept { text: Option<String> },
    Reject,
    /// Conflict card (FR-IMP-06).
    KeepCurrent,
    UseNew,
    KeepBoth,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: Uuid,
    pub check_code: String,
    pub letter: String,
    pub rank: i32,
    pub text: String,
    pub why: Option<String>,
    pub source_segment_id: Option<Uuid>,
    pub source_label: Option<String>,
    pub quote: Option<String>,
    pub state: String,
    pub created_at: Ts,
}

#[derive(Debug, Clone, Default)]
pub struct NewFinding {
    pub sheet_id: Uuid,
    pub check_code: String,
    pub letter: String,
    pub rank: i32,
    pub text: String,
    pub why: Option<String>,
    pub source_segment_id: Option<Uuid>,
    pub source_label: Option<String>,
    pub quote: Option<String>,
    pub step_text_hash: String,
    pub run_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AiMessage {
    pub id: Uuid,
    pub role: String,
    pub text: String,
    pub sources: Value,
    pub user_name: Option<String>,
    pub created_at: Ts,
}

#[derive(Debug, Clone, Default)]
pub struct NewRun {
    pub sheet_id: Option<Uuid>,
    pub job: String,
    pub provider: String,
    pub model: String,
    pub sources: Value,
    pub outcome: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub discarded_count: i32,
    pub discard_reasons: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    pub id: Uuid,
    pub sheet_id: Uuid,
    pub sheet_number: String,
    pub sheet_title: String,
    pub headline: String,
    pub what_happened: String,
    pub who_should_check: Vec<String>,
    pub quick_check: String,
    pub severity: String,
    /// The latest status told to readers: from the last update, otherwise from posting.
    pub cause_status: String,
    pub author_id: Uuid,
    pub author_name: String,
    pub posted_at: Ts,
    pub last_update_at: Ts,
    pub withdrawn_at: Option<Ts>,
    pub withdrawn_reason: Option<String>,
    pub archived: bool,
    pub unread: bool,
    pub updated: bool,
    pub checks_total: i64,
    pub checks_open: i64,
    pub not_affected: i64,
    pub affected: i64,
    pub not_applicable: i64,
    pub read_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WarningUpdate {
    pub id: Uuid,
    pub warning_id: Uuid,
    pub text: String,
    pub cause_status: String,
    pub author_name: String,
    pub created_at: Ts,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct WallFilter {
    pub unread: bool,
    pub severity: Option<String>,
    pub area: Option<String>,
    pub my_tasks: bool,
    pub archive: bool,
    pub search: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewWarning {
    pub sheet_id: Uuid,
    pub headline: String,
    pub what_happened: String,
    pub who_should_check: Vec<String>,
    pub quick_check: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Uuid,
    pub warning_id: Uuid,
    pub warning_headline: String,
    pub description: String,
    pub assignee_id: Uuid,
    pub assignee_name: String,
    pub target_area: String,
    pub due_on: Option<NaiveDate>,
    pub result: Option<String>,
    pub closed_at: Option<Ts>,
    pub linked_a3_id: Option<Uuid>,
    pub linked_a3_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTask {
    pub warning_id: Uuid,
    pub description: String,
    pub assignee_id: Option<Uuid>,
    pub target_area: String,
    pub due_on: Option<NaiveDate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MgmtFilter {
    pub project_id: Option<Uuid>,
    pub area: Option<String>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct LabelCount {
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Management {
    pub by_step: Vec<LabelCount>,
    pub by_age: Vec<LabelCount>,
    pub overdue: Vec<SheetRow>,
    pub stalled: Vec<SheetRow>,
    pub closed_count: i64,
    pub average_days_to_close: Option<f64>,
    pub reopened_count: i64,
    pub reopened_by_reason: Vec<LabelCount>,
    pub warning_reach: Vec<WarningReach>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarningReach {
    pub id: Uuid,
    pub headline: String,
    pub read_percent: i64,
    pub checks_open: i64,
    pub checks_done: i64,
    pub affected: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyWork {
    pub unread_warnings: Vec<Warning>,
    pub unread_total: i64,
    pub my_sheets: Vec<SheetRow>,
    pub my_tasks: Vec<Task>,
}
