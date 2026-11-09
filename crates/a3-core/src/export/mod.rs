//! Any A3 exports in two layouts and three formats, using approved content only (section 11).
//! Export reads step text and metadata and nothing else, so pending proposals and AI findings
//! cannot appear in a file (FR-EXP-05).

mod docx;
mod html;

pub use docx::render_docx;
pub use html::{render_html, QUAD_BODY_HEIGHT_MM, QUAD_BODY_WIDTH_MM};

use crate::domain::make_link;
use crate::error::Result;
use crate::model::*;
use crate::repo::Repository;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    Quadrant,
    Accordion,
}

impl Layout {
    pub fn as_str(self) -> &'static str {
        match self {
            Layout::Quadrant => "quadrant",
            Layout::Accordion => "accordion",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    pub layout: Layout,
    pub include_attachments: bool,
    pub include_source_notes: bool,
}

pub struct SourceNote {
    pub letter: String,
    pub label: String,
    pub file_name: Option<String>,
    pub edited: bool,
}

pub struct ExportData {
    pub sheet: Sheet,
    pub steps: Vec<Step>,
    pub link: String,
    pub attachments: Vec<Attachment>,
    pub source_notes: Vec<SourceNote>,
    pub snapshot_number: i64,
    pub exported_at: DateTime<Local>,
}

impl ExportData {
    pub fn footer_text(&self) -> String {
        format!("Exported {}  ·  Snapshot {}", self.exported_at.format("%Y-%m-%d %H:%M"), self.snapshot_number)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone, Utc};
    use serde_json::json;

    fn step(letter: &str, closed: bool, text: &str) -> Step {
        Step {
            id: Uuid::new_v4(),
            sheet_id: Uuid::nil(),
            letter: letter.into(),
            content: json!({"type": "doc", "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": text}]},
                {"type": "bulletList", "content": [{"type": "listItem", "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "A list item"}]}]}]}
            ]}),
            plain_text: text.into(),
            state: if closed { "closed".into() } else { "open".into() },
            closed_at: closed.then(|| Utc.with_ymd_and_hms(2026, 11, 1, 4, 0, 0).unwrap()),
            version: 2,
            reopen_count: 0,
            locked_by: None,
            locked_by_name: None,
            locked_at: None,
            updated_at: Utc::now(),
        }
    }

    pub(crate) fn sample() -> ExportData {
        let now = Utc::now();
        let sheet = Sheet {
            id: Uuid::nil(),
            number: "A3-2026-0042".into(),
            title: "Scrap on line 4".into(),
            project_id: Uuid::nil(),
            project_name: "Press shop".into(),
            project_code: "PS".into(),
            project_ai_enabled: true,
            owner_id: Uuid::nil(),
            owner_name: "Sam".into(),
            team_member_ids: vec![],
            registered_on: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            problem_date: None,
            target_close_date: NaiveDate::from_ymd_opt(2026, 12, 1),
            category: None,
            severity: None,
            source: None,
            area: None,
            product: None,
            origin_warning_id: None,
            origin_task_id: None,
            withdrawn_at: None,
            withdrawn_reason: None,
            updated_at: now,
            status: "Open".into(),
            current_step: Some("B".into()),
            closed_at: None,
            step_strip: "COOO".into(),
            overdue: false,
            stalled: false,
            reopened: false,
            pending_review: 0,
            can_edit: true,
            is_owner: true,
        };
        let attachment = Attachment {
            id: Uuid::nil(),
            sheet_id: Uuid::nil(),
            file_name: "report.pdf".into(),
            version: 1,
            blob_hash: String::new(),
            size_bytes: 1,
            file_type: "pdf".into(),
            read_state: "read".into(),
            read_error: None,
            ai_state: "none".into(),
            ai_error: None,
            ai_note: None,
            summary: None,
            excluded_sheets: vec![],
            confirms_count: 0,
            added_by: Uuid::nil(),
            added_by_name: "Sam".into(),
            added_at: now,
            deleted_at: None,
            informs_steps: vec![],
            sheet_names: vec![],
            pending_images: 0,
        };
        ExportData {
            sheet,
            steps: vec![
                step("A", true, "Scrap rose to 3.5% on line 4."),
                step("B", false, "Die wear is suspected."),
                step("C", false, "Temporary: sort all parts."),
                step("D", false, ""),
            ],
            link: "a3sheet://a3/A3-2026-0042".into(),
            attachments: vec![attachment],
            source_notes: vec![],
            snapshot_number: 3,
            exported_at: Local::now(),
        }
    }
}

/// Reads what an export needs. Everything here is already approved content.
pub async fn gather(repo: &dyn Repository, sheet_id: Uuid) -> Result<ExportData> {
    let detail = repo.sheet_detail(sheet_id).await?;
    let settings = repo.get_settings().await?;
    let link = make_link(&settings.link_base, "a3", &detail.sheet.number);
    let attachments: Vec<Attachment> =
        repo.list_attachments(sheet_id).await?.into_iter().filter(|a| a.deleted_at.is_none()).collect();
    let mut source_notes = Vec::new();
    for step in &detail.steps {
        for r in crate::richtext::source_refs(&step.content) {
            let file_name = match Uuid::parse_str(&r.segment_id) {
                Ok(id) => repo.source_view(id).await.ok().filter(|v| !v.deleted).map(|v| v.file_name),
                Err(_) => None,
            };
            source_notes.push(SourceNote { letter: step.letter.clone(), label: r.label, file_name, edited: r.edited });
        }
    }
    Ok(ExportData {
        snapshot_number: repo.snapshot_count(sheet_id).await?,
        sheet: detail.sheet,
        steps: detail.steps,
        link,
        attachments,
        source_notes,
        exported_at: Local::now(),
    })
}
