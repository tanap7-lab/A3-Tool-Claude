use super::ExtractedSegment;
use crate::error::{CoreError, Result};
use serde_json::json;
use std::io::Read;
use std::path::Path;

fn message_segments(subject: &str, from: &str, to: &str, date: &str, body: &str) -> Vec<ExtractedSegment> {
    let mut out = vec![ExtractedSegment::new(
        "Message header",
        json!({"part": "header"}),
        format!("Subject: {subject}\nFrom: {from}\nTo: {to}\nDate: {date}"),
    )];
    for (i, seg) in super::text::split_lines(body).into_iter().enumerate() {
        let label = if i == 0 { "Message body".to_string() } else { format!("Message body, part {}", i + 1) };
        out.push(ExtractedSegment::new(label, json!({"part": "body", "index": i}), seg.text));
    }
    out
}

pub fn extract_eml(path: &Path) -> Result<Vec<ExtractedSegment>> {
    let bytes = std::fs::read(path)?;
    let msg = mail_parser::MessageParser::default()
        .parse(&bytes)
        .ok_or_else(|| CoreError::new("UNREADABLE", "This email file could not be read."))?;
    let addr = |a: Option<&mail_parser::Address<'_>>| {
        a.map(|x| {
            x.iter()
                .map(|m| m.address().or(m.name()).unwrap_or("").to_string())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
    };
    let body = msg.body_text(0).map(|b| b.into_owned()).unwrap_or_default();
    let date = msg.date().map(|d| d.to_rfc3339()).unwrap_or_default();
    Ok(message_segments(msg.subject().unwrap_or(""), &addr(msg.from()), &addr(msg.to()), &date, &body))
}

/// Outlook .msg is a compound file. These streams hold subject, sender, recipients and body text.
pub fn extract_msg(path: &Path) -> Result<Vec<ExtractedSegment>> {
    let mut comp = cfb::open(path).map_err(|e| CoreError::new("UNREADABLE", format!("This Outlook file could not be read: {e}")))?;
    let mut read = |prop: &str| -> String {
        for (suffix, utf16) in [("001F", true), ("001E", false)] {
            let name = format!("/__substg1.0_{prop}{suffix}");
            if let Ok(mut s) = comp.open_stream(&name) {
                let mut buf = Vec::new();
                if s.by_ref().take(20 * 1024 * 1024).read_to_end(&mut buf).is_ok() {
                    return if utf16 {
                        let units: Vec<u16> = buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                        String::from_utf16_lossy(&units)
                    } else {
                        String::from_utf8_lossy(&buf).into_owned()
                    };
                }
            }
        }
        String::new()
    };
    let (subject, from, to, body) = (read("0037"), read("0C1A"), read("0E04"), read("1000"));
    if subject.is_empty() && body.is_empty() {
        return Err(CoreError::new("UNREADABLE", "This Outlook file has no readable subject or body."));
    }
    Ok(message_segments(&subject, &from, &to, "", &body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eml_gives_header_and_body() {
        let dir = std::env::temp_dir().join(format!("a3-eml-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("m.eml");
        std::fs::write(&p, "From: a@example.com\r\nTo: b@example.com\r\nSubject: Line 4 stop\r\nContent-Type: text/plain\r\n\r\nScrap rose to 3.5%.\r\n").unwrap();
        let segs = extract_eml(&p).unwrap();
        assert!(segs[0].text.contains("Subject: Line 4 stop"));
        assert!(segs[1].text.contains("Scrap rose to 3.5%."));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
