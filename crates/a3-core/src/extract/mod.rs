//! Reads attachments locally (section 8). Files are untrusted, so reading runs in a child process
//! with a time limit: a bad file, even one that crashes a native parser, cannot take the app down.

mod email;
mod office;
mod pdf;
mod sheet;
mod text;

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedSegment {
    pub label: String,
    pub location: Value,
    pub text: String,
    /// A page or slide with little or no text, to be read by AI vision if available.
    pub needs_image: bool,
    /// Rows past the first 50 of a sheet are searchable but not sent to AI.
    pub ai_visible: bool,
    /// The sheet name, so the user can pick which sheets count.
    pub group_name: Option<String>,
}

impl ExtractedSegment {
    pub(crate) fn new(label: impl Into<String>, location: Value, text: impl Into<String>) -> Self {
        Self { label: label.into(), location, text: text.into(), needs_image: false, ai_visible: true, group_name: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Pdf,
    Image,
    Docx,
    Spreadsheet,
    Csv,
    Pptx,
    Text,
    Eml,
    Msg,
    LegacyOffice,
    Other,
}

pub fn classify(file_name: &str) -> FileKind {
    let ext = Path::new(file_name).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "pdf" => FileKind::Pdf,
        "png" | "jpg" | "jpeg" => FileKind::Image,
        "docx" => FileKind::Docx,
        "xlsx" | "xlsm" | "xls" => FileKind::Spreadsheet,
        "csv" => FileKind::Csv,
        "pptx" => FileKind::Pptx,
        "txt" | "md" => FileKind::Text,
        "eml" => FileKind::Eml,
        "msg" => FileKind::Msg,
        "doc" | "ppt" => FileKind::LegacyOffice,
        _ => FileKind::Other,
    }
}

pub fn file_type_label(file_name: &str) -> String {
    Path::new(file_name).extension().and_then(|e| e.to_str()).unwrap_or("file").to_lowercase()
}

/// The read state a new file starts in, before any reading happens.
pub fn initial_read_state(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Other => "stored_only",
        // Phase 0 has not proven a legacy reader reliable, so these are stored with a prompt.
        FileKind::LegacyOffice => "needs_modern_format",
        _ => "pending",
    }
}

/// Reads a file in this process. Used by the child process and by tests.
pub fn extract_file(path: &Path, file_name: &str) -> Result<Vec<ExtractedSegment>> {
    match classify(file_name) {
        FileKind::Pdf => pdf::extract(path),
        FileKind::Image => Ok(vec![ExtractedSegment {
            needs_image: true,
            ..ExtractedSegment::new("Image", serde_json::json!({"image": true}), "")
        }]),
        FileKind::Docx => office::extract_docx(path),
        FileKind::Pptx => office::extract_pptx(path),
        FileKind::Spreadsheet => sheet::extract_workbook(path),
        FileKind::Csv => sheet::extract_csv(path),
        FileKind::Text => text::extract(path),
        FileKind::Eml => email::extract_eml(path),
        FileKind::Msg => email::extract_msg(path),
        FileKind::LegacyOffice | FileKind::Other => Ok(vec![]),
    }
}

/// An image handed to a vision model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub mime: String,
    pub base64: String,
}

/// The picture(s) behind a `needs_image` segment.
pub fn segment_images(path: &Path, file_name: &str, location: &Value) -> Result<Vec<ImageData>> {
    match classify(file_name) {
        FileKind::Pdf => pdf::render_page(path, location["page"].as_u64().unwrap_or(1) as usize),
        FileKind::Image => pdf::load_image(path).map(|i| vec![i]),
        FileKind::Pptx => office::slide_images(path, location["slide"].as_u64().unwrap_or(1) as usize),
        _ => Ok(vec![]),
    }
}

/// What reading a plain text file produces. Tests use this to set up passages without a file.
#[allow(non_snake_case)]
#[doc(hidden)]
pub fn ExtractedSegment_for_tests(text_body: &str) -> Vec<ExtractedSegment> {
    text::split_lines(text_body)
}

const READ_LIMIT: Duration = Duration::from_secs(120);

/// Runs `exe --a3-extract <path> <name>` and reads the JSON it prints.
pub async fn extract_isolated(exe: &Path, path: &Path, file_name: &str) -> Result<Vec<ExtractedSegment>> {
    let out = run_child(exe, &["--a3-extract".as_ref(), path.as_os_str(), file_name.as_ref()]).await?;
    Ok(serde_json::from_slice(&out)?)
}

pub async fn images_isolated(exe: &Path, path: &Path, file_name: &str, location: &Value) -> Result<Vec<ImageData>> {
    let loc = location.to_string();
    let out = run_child(exe, &["--a3-images".as_ref(), path.as_os_str(), file_name.as_ref(), loc.as_ref()]).await?;
    Ok(serde_json::from_slice(&out)?)
}

async fn run_child(exe: &Path, args: &[&std::ffi::OsStr]) -> Result<Vec<u8>> {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.args(args).kill_on_drop(true).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = tokio::time::timeout(READ_LIMIT, cmd.output())
        .await
        .map_err(|_| CoreError::new("UNREADABLE", "Reading this file took too long."))??;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let msg = String::from_utf8_lossy(&out.stderr);
        let first = msg.lines().next().filter(|l| !l.is_empty()).unwrap_or("This file could not be read.");
        Err(CoreError::new("UNREADABLE", first.to_string()))
    }
}

/// The child-process entry point. Returns true when `args` asked for it, so `main` can exit before
/// any window starts.
pub fn run_child_mode(args: &[String]) -> bool {
    use std::io::Write;
    let finish = |result: Result<Vec<u8>>| match result {
        Ok(bytes) => {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(&bytes);
            let _ = out.flush();
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("{}", e.message);
            std::process::exit(2);
        }
    };
    match args.get(1).map(String::as_str) {
        Some("--a3-extract") if args.len() >= 4 => {
            finish(extract_file(Path::new(&args[2]), &args[3]).and_then(|s| Ok(serde_json::to_vec(&s)?)));
            true
        }
        Some("--a3-images") if args.len() >= 5 => {
            let loc: Value = serde_json::from_str(&args[4]).unwrap_or(Value::Null);
            finish(segment_images(Path::new(&args[2]), &args[3], &loc).and_then(|s| Ok(serde_json::to_vec(&s)?)));
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_spec_file_types() {
        assert_eq!(classify("a.PDF"), FileKind::Pdf);
        assert_eq!(classify("a.xlsm"), FileKind::Spreadsheet);
        assert_eq!(classify("a.ppt"), FileKind::LegacyOffice);
        assert_eq!(classify("a.zip"), FileKind::Other);
        assert_eq!(initial_read_state(FileKind::Other), "stored_only");
        assert_eq!(initial_read_state(FileKind::LegacyOffice), "needs_modern_format");
        assert_eq!(initial_read_state(FileKind::Pptx), "pending");
    }
}
