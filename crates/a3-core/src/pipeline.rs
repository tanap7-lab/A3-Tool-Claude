//! The background path of one attachment: fetch it, read it in a child process, read pictures if
//! vision is available, then let AI propose. Each stage records its state in the database, so an
//! interrupted job resumes at next launch (FR-ATT-06).

use crate::ai::{AiService, ImportOutcome};
use crate::error::{CoreError, Result};
use crate::extract::{self, ExtractedSegment, ImageData};
use crate::repo::Repository;
use crate::store::AttachmentStore;
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

/// How files are read. The app uses a child process. Tests read in-process.
#[async_trait]
pub trait FileReader: Send + Sync {
    async fn read(&self, path: &Path, file_name: &str) -> Result<Vec<ExtractedSegment>>;
    async fn images(&self, path: &Path, file_name: &str, location: &Value) -> Result<Vec<ImageData>>;
}

pub struct ChildProcessReader {
    pub exe: PathBuf,
}

#[async_trait]
impl FileReader for ChildProcessReader {
    async fn read(&self, path: &Path, file_name: &str) -> Result<Vec<ExtractedSegment>> {
        extract::extract_isolated(&self.exe, path, file_name).await
    }
    async fn images(&self, path: &Path, file_name: &str, location: &Value) -> Result<Vec<ImageData>> {
        extract::images_isolated(&self.exe, path, file_name, location).await
    }
}

pub struct InProcessReader;

#[async_trait]
impl FileReader for InProcessReader {
    async fn read(&self, path: &Path, file_name: &str) -> Result<Vec<ExtractedSegment>> {
        let (p, n) = (path.to_path_buf(), file_name.to_string());
        tokio::task::spawn_blocking(move || extract::extract_file(&p, &n))
            .await
            .map_err(|_| CoreError::new("UNREADABLE", "This file could not be read."))?
    }
    async fn images(&self, path: &Path, file_name: &str, location: &Value) -> Result<Vec<ImageData>> {
        extract::segment_images(path, file_name, location)
    }
}

pub struct Pipeline {
    pub repo: Arc<dyn Repository>,
    pub store: Arc<dyn AttachmentStore>,
    pub reader: Arc<dyn FileReader>,
    pub temp_dir: PathBuf,
    /// None when no API key is set: records still work, without proposals (principle 6).
    pub ai: Option<Arc<AiService>>,
    pub vision: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineResult {
    pub attachment_id: Uuid,
    pub read_state: String,
    pub import: Option<ImportOutcome>,
    pub ai_error: Option<String>,
}

impl Pipeline {
    pub async fn process(&self, attachment_id: Uuid) -> Result<PipelineResult> {
        let att = self.repo.attachment(attachment_id).await?;
        let mut read_state = att.read_state.clone();

        let needs_file = matches!(read_state.as_str(), "pending" | "reading");
        let needs_images = self.ai.is_some() && self.vision && att.pending_images > 0;
        if needs_file || needs_images {
            std::fs::create_dir_all(&self.temp_dir)?;
            let temp = self.temp_dir.join(format!("{attachment_id}-{}", sanitize(&att.file_name)));
            let fetched = self.store.get_to_file(&att.blob_hash, &temp).await;
            let result = match fetched {
                Ok(()) => self.read_stages(attachment_id, &att.file_name, att.sheet_id, &temp, needs_file).await,
                Err(e) => Err(e),
            };
            let _ = std::fs::remove_file(&temp);
            match result {
                Ok(state) => read_state = state,
                Err(e) => {
                    // FR ("Untrusted files"): a failing file is marked Unreadable and nothing else breaks.
                    self.repo.set_read_state(attachment_id, "unreadable", Some(&e.message)).await?;
                    self.repo.set_ai_state(attachment_id, "none", None, None).await?;
                    return Ok(PipelineResult { attachment_id, read_state: "unreadable".into(), import: None, ai_error: None });
                }
            }
        }

        if read_state != "read" {
            self.repo.set_ai_state(attachment_id, "none", None, None).await?;
            return Ok(PipelineResult { attachment_id, read_state, import: None, ai_error: None });
        }
        let current_ai_state = self.repo.attachment(attachment_id).await?.ai_state;
        let Some(ai) = self.ai.as_ref().filter(|_| current_ai_state != "done") else {
            return Ok(PipelineResult { attachment_id, read_state, import: None, ai_error: None });
        };
        self.repo.set_ai_state(attachment_id, "running", None, None).await?;
        match ai.import_attachment(attachment_id).await {
            Ok(outcome) => {
                self.repo.set_ai_state(attachment_id, "done", None, outcome.note.as_deref()).await?;
                Ok(PipelineResult { attachment_id, read_state, import: Some(outcome), ai_error: None })
            }
            Err(e) if e.code == "AI_OFF" => {
                self.repo.set_ai_state(attachment_id, "none", None, None).await?;
                Ok(PipelineResult { attachment_id, read_state, import: None, ai_error: None })
            }
            Err(e) => {
                self.repo.set_ai_state(attachment_id, "failed", Some(&e.message), None).await?;
                Ok(PipelineResult { attachment_id, read_state, import: None, ai_error: Some(e.message) })
            }
        }
    }

    async fn read_stages(&self, id: Uuid, file_name: &str, sheet: Uuid, path: &Path, read_text: bool) -> Result<String> {
        if read_text {
            self.repo.set_read_state(id, "reading", None).await?;
            let segments = self.reader.read(path, file_name).await?;
            self.repo.replace_segments(id, &segments).await?;
        }
        if let (Some(ai), true) = (&self.ai, self.vision) {
            let segs = self.repo.segments(id).await?;
            for seg in segs.into_iter().filter(|s| s.needs_image) {
                let images = self.reader.images(path, file_name, &seg.location).await?;
                // A picture that cannot be read leaves the page unread. It does not fail the file.
                match ai.read_image(sheet, images).await {
                    Ok(text) if !text.trim().is_empty() => self.repo.set_segment_from_image(seg.id, text.trim()).await?,
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e.message, "reading an image failed"),
                }
            }
        }
        self.repo.set_read_state(id, "read", None).await?;
        Ok("read".to_string())
    }
}

/// Keeps the extension, which the readers go by, and nothing that could leave the temp folder.
fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_names_cannot_escape() {
        assert_eq!(sanitize("..\\..\\evil name.pdf"), ".._.._evil_name.pdf");
        assert!(!sanitize("a/b:c.docx").contains('/'));
    }
}
