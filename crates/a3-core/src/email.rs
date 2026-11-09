//! Email drafts built from a fixed template and the card's approved text, so no new AI wording is
//! introduced. The app prepares the email and the user sends it (FR-EML-04).

use crate::model::Warning;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailDraft {
    pub subject: String,
    pub text: String,
    pub html: String,
}

pub fn build_draft(w: &Warning, warning_link: &str, a3_link: &str) -> EmailDraft {
    let subject = format!("Early warning: {} ({})", w.headline, w.sheet_number);
    let quick = if w.quick_check.trim().is_empty() { "None stated yet." } else { w.quick_check.trim() };
    let text = format!(
        "What happened\r\n{}\r\n\r\nQuick check\r\n{}\r\n\r\nCause status\r\n{}\r\n\r\nWarning: {}\r\nA3: {}\r\nA3 number: {}\r\n",
        w.what_happened.trim(),
        quick.replace('\n', "\r\n"),
        w.cause_status,
        warning_link,
        a3_link,
        w.sheet_number
    );
    let e = |s: &str| html_escape::encode_text(s).to_string();
    let a = |s: &str| html_escape::encode_double_quoted_attribute(s).to_string();
    let html = format!(
        "<p><b>What happened</b><br>{}</p><p><b>Quick check</b><br>{}</p><p><b>Cause status</b><br>{}</p><p><a href=\"{}\">Open the warning</a><br><a href=\"{}\">Open the A3</a><br>A3 number: {}</p>",
        e(w.what_happened.trim()),
        e(quick).replace('\n', "<br>"),
        e(&w.cause_status),
        a(warning_link),
        a(a3_link),
        e(&w.sheet_number)
    );
    EmailDraft { subject, text, html }
}

/// FR-EML-02: an `.eml` with `X-Unsent: 1`, which mail apps open as an unsent draft.
pub fn to_eml(d: &EmailDraft) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let boundary = "=_a3sheet_boundary";
    let wrap = |s: &str| {
        let encoded = b64.encode(s.as_bytes());
        encoded.as_bytes().chunks(76).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join("\r\n")
    };
    let subject = format!("=?UTF-8?B?{}?=", b64.encode(d.subject.replace(['\r', '\n'], " ").as_bytes()));
    format!(
        "X-Unsent: 1\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\r\n\
         --{boundary}\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n\r\n\
         --{boundary}\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n\r\n--{boundary}--\r\n",
        wrap(&d.text),
        wrap(&d.html)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn warning() -> Warning {
        Warning {
            id: Uuid::nil(),
            sheet_id: Uuid::nil(),
            sheet_number: "A3-2026-0042".into(),
            sheet_title: "t".into(),
            headline: "Cracks on <PN-1>".into(),
            what_happened: "Scrap rose to 3.5%.".into(),
            who_should_check: vec![],
            quick_check: "Check die 4.".into(),
            severity: "High".into(),
            cause_status: "Cause not confirmed".into(),
            author_id: Uuid::nil(),
            author_name: "a".into(),
            posted_at: Utc::now(),
            last_update_at: Utc::now(),
            withdrawn_at: None,
            withdrawn_reason: None,
            archived: false,
            unread: true,
            updated: false,
            checks_total: 0,
            checks_open: 0,
            not_affected: 0,
            affected: 0,
            not_applicable: 0,
            read_count: 0,
        }
    }

    #[test]
    fn draft_has_spec_subject_and_both_links() {
        let d = build_draft(&warning(), "a3sheet://warning/x", "a3sheet://a3/A3-2026-0042");
        assert_eq!(d.subject, "Early warning: Cracks on <PN-1> (A3-2026-0042)");
        for needle in ["Scrap rose to 3.5%.", "Check die 4.", "Cause not confirmed", "a3sheet://warning/x", "a3sheet://a3/A3-2026-0042", "A3 number: A3-2026-0042"] {
            assert!(d.text.contains(needle), "{needle}");
        }
        assert!(d.html.contains("Scrap rose"));
    }

    #[test]
    fn eml_opens_as_unsent_and_cannot_be_header_injected() {
        let mut d = build_draft(&warning(), "l1", "l2");
        d.subject = "x\r\nBcc: attacker@example.com".into();
        let eml = to_eml(&d);
        assert!(eml.starts_with("X-Unsent: 1\r\n"));
        assert!(!eml.contains("Bcc: attacker"));
    }
}
