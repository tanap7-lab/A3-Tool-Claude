//! Background reading of attachments (FR-ATT-06). The user keeps working while it runs.

use crate::state::{pipeline_for, AppState, Session};
use a3_core::pipeline::PipelineResult;
use a3_core::repo::{AttachmentRepo, Repository};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEvent {
    pub attachment_id: Uuid,
    pub sheet_id: Uuid,
    pub file_name: String,
    /// `running` or `done`
    pub phase: String,
    pub read_state: Option<String>,
    pub added: Option<usize>,
    pub confirms: Option<usize>,
    pub note: Option<String>,
    pub error: Option<String>,
}

pub fn spawn_attachment(app: &AppHandle, session: Session, attachment_id: Uuid) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let slots = app.state::<AppState>().job_slots.clone();
        let _permit = slots.acquire_owned().await;
        let Ok(att) = AttachmentRepo::attachment(session.repo.as_ref(), attachment_id).await else { return };
        let base = JobEvent {
            attachment_id,
            sheet_id: att.sheet_id,
            file_name: att.file_name.clone(),
            phase: "running".into(),
            read_state: None,
            added: None,
            confirms: None,
            note: None,
            error: None,
        };
        let _ = app.emit("job", base.clone());
        let started = std::time::Instant::now();
        let result: a3_core::Result<PipelineResult> = match pipeline_for(&app, &session) {
            Ok(p) => p.process(attachment_id).await,
            Err(e) => Err(e),
        };
        // Timings and states only, so no A3 or attachment text reaches the log.
        tracing::info!(attachment = %attachment_id, ms = started.elapsed().as_millis() as u64, ok = result.is_ok(), "attachment job finished");
        let done = match result {
            Ok(r) => JobEvent {
                phase: "done".into(),
                read_state: Some(r.read_state),
                added: r.import.as_ref().map(|i| i.added),
                confirms: r.import.as_ref().map(|i| i.confirms),
                note: r.import.and_then(|i| i.note),
                error: r.ai_error,
                ..base
            },
            Err(e) => JobEvent { phase: "done".into(), error: Some(e.message), ..base },
        };
        let _ = app.emit("job", done);
    });
}

/// FR-ATT-06: interrupted jobs resume at next launch. Also runs routine housekeeping.
pub async fn resume_after_sign_in(app: AppHandle, session: Session) {
    let repo: &dyn Repository = session.repo.as_ref();
    match repo.unfinished_attachments().await {
        Ok(list) => {
            for a in list {
                spawn_attachment(&app, session.clone(), a.id);
            }
        }
        Err(e) => tracing::warn!(error = %e.message, "could not list unfinished attachments"),
    }
    if let Err(e) = repo.purge_deleted().await {
        tracing::warn!(error = %e.message, "could not purge deleted attachments");
    }
}
