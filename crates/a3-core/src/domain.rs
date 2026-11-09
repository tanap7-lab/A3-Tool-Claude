//! Small pure rules that are not database state: links, cause status, file names, text matching.

use once_cell::sync::Lazy;
use regex::Regex;
use sha2::{Digest, Sha256};

pub const LETTERS: [&str; 4] = ["A", "B", "C", "D"];

pub fn step_name(letter: &str) -> &'static str {
    match letter {
        "A" => "Define the problem",
        "B" => "Analyze root cause",
        "C" => "Implement solution",
        "D" => "Validate and standardize",
        _ => "",
    }
}

/// FR-LNK-04: strict patterns. Anything else is ignored.
static A3_NUMBER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^A3-\d{4}-\d{4,6}$").unwrap());
static WARNING_ID: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$").unwrap());

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum LinkTarget {
    A3(String),
    Warning(String),
}

/// Reads an `a3sheet://` link, an `https://` link under the base address, or a bare A3 number.
pub fn parse_link(input: &str, link_base: &str) -> Option<LinkTarget> {
    let input = input.trim();
    if A3_NUMBER.is_match(input) {
        return Some(LinkTarget::A3(input.to_string()));
    }
    let rest = if let Some(r) = input.strip_prefix("a3sheet://") {
        r
    } else {
        let base = link_base.trim().trim_end_matches('/');
        if base.is_empty() || !base.starts_with("https://") {
            return None;
        }
        input.strip_prefix(base)?.strip_prefix('/')?
    };
    let rest = rest.trim_end_matches('/');
    let (kind, id) = rest.split_once('/')?;
    match kind {
        "a3" if A3_NUMBER.is_match(id) => Some(LinkTarget::A3(id.to_string())),
        "warning" if WARNING_ID.is_match(id) => Some(LinkTarget::Warning(id.to_string())),
        _ => None,
    }
}

pub fn make_link(link_base: &str, kind: &str, id: &str) -> String {
    let base = link_base.trim().trim_end_matches('/');
    if base.starts_with("https://") {
        format!("{base}/{kind}/{id}")
    } else {
        format!("a3sheet://{kind}/{id}")
    }
}

/// Section 10: cause status comes from step states alone.
pub fn cause_status(b_closed: bool, d_closed: bool) -> &'static str {
    if d_closed {
        "Solution verified"
    } else if b_closed {
        "Root cause confirmed"
    } else {
        "Cause not confirmed"
    }
}

/// FR-EXP-08: `A3-2026-0042_Short-title_quadrant.pdf`
pub fn export_file_name(number: &str, title: &str, layout: &str, ext: &str) -> String {
    let mut short = String::new();
    for word in title.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).take(5) {
        if !short.is_empty() {
            short.push('-');
        }
        short.push_str(word);
    }
    if short.is_empty() {
        short.push_str("Untitled");
    }
    format!("{number}_{short}_{layout}.{ext}")
}

/// Text used for "the same point". Ignores case, spacing, and typographic quote and dash variants.
pub fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{201B}' | '`' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            c => c,
        })
        .flat_map(|c| c.to_lowercase())
        .collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    out.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn norm_key(text: &str) -> String {
    sha256_hex(normalize(text).as_bytes())
}

/// Splits step text into sentences so AI can cite them as `A.1`, `A.2` and so on.
pub fn split_sentences(text: &str) -> Vec<String> {
    static BOUNDARY: Lazy<Regex> = Lazy::new(|| Regex::new(r"[.!?]+(?:\s+|$)").unwrap());
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut start = 0;
        for m in BOUNDARY.find_iter(line) {
            let prev = line[..m.start()].chars().last();
            let next = line[m.end()..].chars().next();
            // Keep "3.5" whole, and keep abbreviations whose next word starts lower-case.
            if next.is_some_and(|c| c.is_lowercase()) || prev.is_none() {
                continue;
            }
            let s = line[start..m.end()].trim();
            if !s.is_empty() {
                out.push(s.to_string());
            }
            start = m.end();
        }
        let rest = line[start..].trim();
        if !rest.is_empty() {
            out.push(rest.to_string());
        }
    }
    out
}

pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_strict() {
        assert_eq!(parse_link("a3sheet://a3/A3-2026-0042", ""), Some(LinkTarget::A3("A3-2026-0042".into())));
        assert_eq!(parse_link("A3-2026-0042", ""), Some(LinkTarget::A3("A3-2026-0042".into())));
        assert_eq!(
            parse_link("https://links.example.com/a3/A3-2026-0042", "https://links.example.com/"),
            Some(LinkTarget::A3("A3-2026-0042".into()))
        );
        assert_eq!(parse_link("https://evil.example.com/a3/A3-2026-0042", "https://links.example.com"), None);
        assert_eq!(parse_link("a3sheet://a3/A3-2026-0042;drop table", ""), None);
        assert_eq!(parse_link("a3sheet://a3/../../etc", ""), None);
        assert_eq!(parse_link("a3sheet://warning/not-a-uuid", ""), None);
        assert_eq!(
            parse_link("a3sheet://warning/123e4567-e89b-12d3-a456-426614174000", ""),
            Some(LinkTarget::Warning("123e4567-e89b-12d3-a456-426614174000".into()))
        );
    }

    #[test]
    fn links_follow_base_address() {
        assert_eq!(make_link("", "a3", "A3-2026-0042"), "a3sheet://a3/A3-2026-0042");
        assert_eq!(make_link("https://x.example/l/", "a3", "A3-2026-0042"), "https://x.example/l/a3/A3-2026-0042");
    }

    #[test]
    fn cause_status_follows_steps() {
        assert_eq!(cause_status(false, false), "Cause not confirmed");
        assert_eq!(cause_status(true, false), "Root cause confirmed");
        assert_eq!(cause_status(true, true), "Solution verified");
    }

    #[test]
    fn file_name_matches_spec_shape() {
        assert_eq!(
            export_file_name("A3-2026-0042", "Short title", "quadrant", "pdf"),
            "A3-2026-0042_Short-title_quadrant.pdf"
        );
        assert_eq!(export_file_name("A3-2026-0042", "  ", "accordion", "docx"), "A3-2026-0042_Untitled_accordion.docx");
    }

    #[test]
    fn sentences_keep_decimals_whole() {
        let s = split_sentences("Scrap rose to 3.5% in March. Target is 1.0%.\nLine 4 stopped");
        assert_eq!(s, vec!["Scrap rose to 3.5% in March.", "Target is 1.0%.", "Line 4 stopped"]);
    }

    #[test]
    fn normalize_ignores_spacing_case_and_dash_variants() {
        assert_eq!(normalize("Line  4\u{2013}B  Stopped"), normalize("line 4-b stopped"));
    }
}
