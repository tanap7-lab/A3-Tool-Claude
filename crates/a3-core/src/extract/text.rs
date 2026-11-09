use super::ExtractedSegment;
use crate::error::Result;
use serde_json::json;
use std::path::Path;

const LINES_PER_SEGMENT: usize = 40;

pub fn extract(path: &Path) -> Result<Vec<ExtractedSegment>> {
    let bytes = std::fs::read(path)?;
    Ok(split_lines(&String::from_utf8_lossy(&bytes)))
}

pub(super) fn split_lines(text: &str) -> Vec<ExtractedSegment> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .chunks(LINES_PER_SEGMENT)
        .enumerate()
        .filter(|(_, c)| c.iter().any(|l| !l.trim().is_empty()))
        .map(|(i, chunk)| {
            let start = i * LINES_PER_SEGMENT + 1;
            let end = start + chunk.len() - 1;
            ExtractedSegment::new(format!("Lines {start}-{end}"), json!({"line": start, "lineEnd": end}), chunk.join("\n"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_carry_line_numbers() {
        let text = (1..=45).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let segs = split_lines(&text);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].label, "Lines 1-40");
        assert_eq!(segs[1].location["line"], 41);
        assert!(segs[1].text.starts_with("line 41"));
    }
}
