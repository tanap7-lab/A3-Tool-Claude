//! Large spreadsheets (section 8): AI gets column names, the row count and the first 50 rows of each
//! sheet. Later rows stay searchable but are not sent.

use super::ExtractedSegment;
use crate::error::{CoreError, Result};
use calamine::{open_workbook_auto, Data, Reader};
use serde_json::json;
use std::path::Path;

const AI_ROWS: usize = 50;
const TAIL_CHUNK: usize = 200;
const MAX_ROWS: usize = 200_000;

pub fn extract_workbook(path: &Path) -> Result<Vec<ExtractedSegment>> {
    let mut wb = open_workbook_auto(path).map_err(|e| CoreError::new("UNREADABLE", format!("This workbook could not be read: {e}")))?;
    let mut out = Vec::new();
    for name in wb.sheet_names().to_vec() {
        let range = wb
            .worksheet_range(&name)
            .map_err(|e| CoreError::new("UNREADABLE", format!("Sheet {name} could not be read: {e}")))?;
        let (first_row, first_col) = range.start().map(|(r, c)| (r as usize + 1, c as usize)).unwrap_or((1, 0));
        let rows: Vec<Vec<String>> = range.rows().take(MAX_ROWS).map(|r| r.iter().map(cell_text).collect()).collect();
        sheet_segments(&name, &rows, first_row, first_col, &mut out);
    }
    Ok(out)
}

pub fn extract_csv(path: &Path) -> Result<Vec<ExtractedSegment>> {
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("CSV").to_string();
    let bytes = std::fs::read(path)?;
    let mut reader = csv::ReaderBuilder::new().has_headers(false).flexible(true).from_reader(bytes.as_slice());
    let mut rows = Vec::new();
    for rec in reader.records().take(MAX_ROWS) {
        let rec = rec.map_err(|e| CoreError::new("UNREADABLE", format!("This CSV could not be read: {e}")))?;
        rows.push(rec.iter().map(str::to_string).collect::<Vec<_>>());
    }
    let mut out = Vec::new();
    sheet_segments(&name, &rows, 1, 0, &mut out);
    Ok(out)
}

fn cell_text(c: &Data) -> String {
    match c {
        Data::Empty => String::new(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        other => other.to_string(),
    }
}

pub(super) fn column_name(mut i: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (i % 26) as u8) as char);
        if i < 26 {
            return s;
        }
        i = i / 26 - 1;
    }
}

fn row_lines(rows: &[Vec<String>], first_row_number: usize) -> String {
    rows.iter()
        .enumerate()
        .filter(|(_, r)| r.iter().any(|c| !c.trim().is_empty()))
        .map(|(i, r)| format!("Row {}: {}", first_row_number + i, r.join(" | ")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn sheet_segments(name: &str, rows: &[Vec<String>], first_row: usize, first_col: usize, out: &mut Vec<ExtractedSegment>) {
    if rows.iter().all(|r| r.iter().all(|c| c.trim().is_empty())) {
        return;
    }
    let width = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let col_a = column_name(first_col);
    let col_z = column_name(first_col + width - 1);
    let range_label = |from: usize, to: usize| format!("{name}!{col_a}{from}:{col_z}{to}");

    let head_len = rows.len().min(AI_ROWS);
    let last = first_row + head_len - 1;
    let header = rows[0].join(" | ");
    let text = format!(
        "Sheet: {name}\nColumns: {header}\nTotal rows: {}\n{}",
        rows.len(),
        row_lines(&rows[..head_len], first_row)
    );
    out.push(ExtractedSegment {
        group_name: Some(name.to_string()),
        ..ExtractedSegment::new(
            range_label(first_row, last),
            json!({"sheet": name, "range": format!("{col_a}{first_row}:{col_z}{last}")}),
            text,
        )
    });

    for (i, chunk) in rows[head_len..].chunks(TAIL_CHUNK).enumerate() {
        let from = first_row + head_len + i * TAIL_CHUNK;
        let to = from + chunk.len() - 1;
        out.push(ExtractedSegment {
            ai_visible: false,
            group_name: Some(name.to_string()),
            ..ExtractedSegment::new(
                range_label(from, to),
                json!({"sheet": name, "range": format!("{col_a}{from}:{col_z}{to}")}),
                row_lines(chunk, from),
            )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_names_roll_over() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(27), "AB");
    }

    #[test]
    fn only_first_fifty_rows_go_to_ai() {
        let rows: Vec<Vec<String>> = (0..130).map(|i| vec![format!("id{i}"), format!("{i}")]).collect();
        let mut out = Vec::new();
        sheet_segments("Data", &rows, 1, 0, &mut out);
        assert_eq!(out.len(), 2);
        assert!(out[0].ai_visible);
        assert_eq!(out[0].label, "Data!A1:B50");
        assert!(out[0].text.contains("Total rows: 130"));
        assert!(out[0].text.contains("id49"));
        assert!(!out[0].text.contains("id50"));
        assert!(!out[1].ai_visible);
        assert_eq!(out[1].label, "Data!A51:B130");
        assert!(out[1].text.contains("Row 51: id50 | 50"));
    }
}
