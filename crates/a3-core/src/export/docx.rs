//! DOCX export with native Word tables and heading styles (section 5).

use super::{ExportData, ExportOptions, Layout};
use crate::domain::{step_name, LETTERS};
use crate::error::{CoreError, Result};
use base64::Engine;
use docx_rs::*;
use serde_json::Value;
use std::io::Cursor;

// Page sizes in twips (1/20 pt).
const A3_LONG: u32 = 23811;
const A3_SHORT: u32 = 16838;
const A4_LONG: u32 = 16838;
const A4_SHORT: u32 = 11906;
const MARGIN: i32 = 567;
const EMU_PER_TWIP: u32 = 635;

enum Block {
    Para(Paragraph),
    Table(Table),
}

fn run(text: &str, size: usize, bold: bool, italic: bool, color: Option<&str>) -> Run {
    let mut r = Run::new().add_text(text).size(size);
    if bold {
        r = r.bold();
    }
    if italic {
        r = r.italic();
    }
    if let Some(c) = color {
        r = r.color(c);
    }
    r
}

fn children(node: &Value) -> &[Value] {
    node.get("content").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn image_run(src: &str, max_width_twips: u32) -> Option<Run> {
    let b64 = crate::richtext::safe_image_src(src)?.split_once(',')?.1;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let mut png = Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).ok()?;
    let (w, h) = (img.width().max(1), img.height().max(1));
    // 96 dpi is 15 twips per pixel. Images scale to the column.
    let width_twips = (w * 15).min(max_width_twips);
    let height_twips = (u64::from(width_twips) * u64::from(h) / u64::from(w)) as u32;
    let pic = Pic::new_with_dimensions(png.into_inner(), w, h).size(width_twips * EMU_PER_TWIP, height_twips * EMU_PER_TWIP);
    Some(Run::new().add_image(pic))
}

fn inline_runs(node: &Value, size: usize, max_width: u32, mut p: Paragraph) -> Paragraph {
    for child in children(node) {
        match child.get("type").and_then(Value::as_str).unwrap_or("") {
            "text" => {
                let marks = child.get("marks").and_then(Value::as_array);
                let has = |t: &str| marks.is_some_and(|m| m.iter().any(|x| x.get("type").and_then(Value::as_str) == Some(t)));
                p = p.add_run(run(child.get("text").and_then(Value::as_str).unwrap_or(""), size, has("bold"), has("italic"), None));
            }
            "hardBreak" => p = p.add_run(Run::new().add_break(BreakType::TextWrapping)),
            "image" => {
                if let Some(r) = child["attrs"]["src"].as_str().and_then(|s| image_run(s, max_width)) {
                    p = p.add_run(r);
                }
            }
            _ => {}
        }
    }
    p
}

fn list_blocks(list: &Value, ordered: bool, depth: i32, size: usize, max_width: u32, out: &mut Vec<Block>) {
    for (i, item) in children(list).iter().enumerate() {
        let marker = if ordered { format!("{}. ", i + 1) } else { "\u{2022} ".to_string() };
        let mut first = true;
        for inner in children(item) {
            match inner.get("type").and_then(Value::as_str).unwrap_or("") {
                "paragraph" => {
                    let mut p = Paragraph::new().indent(Some(360 * (depth + 1)), Some(SpecialIndentType::Hanging(240)), None, None);
                    if first {
                        p = p.add_run(run(&marker, size, false, false, None));
                        first = false;
                    }
                    out.push(Block::Para(inline_runs(inner, size, max_width, p)));
                }
                "bulletList" => list_blocks(inner, false, depth + 1, size, max_width, out),
                "orderedList" => list_blocks(inner, true, depth + 1, size, max_width, out),
                _ => {}
            }
        }
    }
}

fn table_block(table: &Value, size: usize, max_width: u32) -> Table {
    let rows: Vec<TableRow> = children(table)
        .iter()
        .map(|row| {
            let cells: Vec<TableCell> = children(row)
                .iter()
                .map(|cell| {
                    let header = cell.get("type").and_then(Value::as_str) == Some("tableHeader");
                    let mut c = TableCell::new();
                    if header {
                        c = c.shading(Shading::new().fill("F8F9FA"));
                    }
                    let mut any = false;
                    for inner in children(cell) {
                        if inner.get("type").and_then(Value::as_str) == Some("paragraph") {
                            c = c.add_paragraph(inline_runs(inner, size, max_width / 2, Paragraph::new()));
                            any = true;
                        }
                    }
                    if !any {
                        c = c.add_paragraph(Paragraph::new());
                    }
                    c
                })
                .collect();
            TableRow::new(cells)
        })
        .collect();
    Table::new(rows)
}

fn step_blocks(doc: &Value, size: usize, max_width: u32) -> Vec<Block> {
    let mut out = Vec::new();
    for node in children(doc) {
        match node.get("type").and_then(Value::as_str).unwrap_or("") {
            "paragraph" => {
                if !children(node).is_empty() {
                    out.push(Block::Para(inline_runs(node, size, max_width, Paragraph::new())));
                }
            }
            "bulletList" => list_blocks(node, false, 0, size, max_width, &mut out),
            "orderedList" => list_blocks(node, true, 0, size, max_width, &mut out),
            "table" => out.push(Block::Table(table_block(node, size, max_width))),
            "image" => {
                if let Some(r) = node["attrs"]["src"].as_str().and_then(|s| image_run(s, max_width)) {
                    out.push(Block::Para(Paragraph::new().add_run(r)));
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        out.push(Block::Para(Paragraph::new().add_run(run("Nothing recorded yet.", size, false, true, Some("5F6368")))));
    }
    out
}

fn state_text(data: &ExportData, letter: &str) -> String {
    match data.steps.iter().find(|s| s.letter == letter) {
        Some(s) if s.state == "closed" => {
            format!("Closed{}", s.closed_at.map(|d| d.format(", %Y-%m-%d").to_string()).unwrap_or_default())
        }
        _ => "Open".to_string(),
    }
}

fn step_doc<'a>(data: &'a ExportData, letter: &str) -> Option<&'a Value> {
    data.steps.iter().find(|s| s.letter == letter).map(|s| &s.content)
}

fn header_paragraphs(data: &ExportData) -> Vec<Paragraph> {
    let s = &data.sheet;
    let strip: String = LETTERS
        .iter()
        .map(|l| {
            let closed = data.steps.iter().any(|st| st.letter == *l && st.state == "closed");
            format!("{l}: {}", if closed { "Closed" } else { "Open" })
        })
        .collect::<Vec<_>>()
        .join("  ");
    let date = |d: Option<chrono::NaiveDate>| d.map(|x| x.to_string()).unwrap_or_else(|| "not set".into());
    let title = if s.title.trim().is_empty() { "Untitled A3" } else { s.title.as_str() };
    let line1 = format!(
        "{}  |  Owner {}  |  Project {}  |  Registered {}  |  Target {}  |  {}",
        s.number, s.owner_name, s.project_name, s.registered_on, date(s.target_close_date), s.status
    );
    let line2 = format!("{strip}  |  {}", data.link);
    let mut out = vec![
        Paragraph::new().style("Title").add_run(Run::new().add_text(title)),
        Paragraph::new().add_run(run(&line1, 18, false, false, Some("5F6368"))),
        Paragraph::new().add_run(run(&line2, 18, false, false, Some("5F6368"))),
    ];
    if s.withdrawn_at.is_some() {
        out.push(Paragraph::new().add_run(run(
            &format!("Withdrawn: {}", s.withdrawn_reason.as_deref().unwrap_or("")),
            18,
            true,
            false,
            Some("D93025"),
        )));
    }
    out
}

fn footer(data: &ExportData) -> Footer {
    Footer::new().add_paragraph(
        Paragraph::new()
            .add_run(run(&format!("{}  ·  Page ", data.footer_text()), 16, false, false, Some("5F6368")))
            .add_page_num(PageNum::new())
            .add_run(run(" of ", 16, false, false, Some("5F6368")))
            .add_num_pages(NumPages::new()),
    )
}

fn extras(data: &ExportData, opts: &ExportOptions) -> Vec<Paragraph> {
    let mut out = Vec::new();
    if opts.include_attachments && !data.attachments.is_empty() {
        let names: Vec<String> = data
            .attachments
            .iter()
            .map(|a| if a.version > 1 { format!("{} (v{})", a.file_name, a.version) } else { a.file_name.clone() })
            .collect();
        out.push(Paragraph::new().add_run(run(&format!("Attachments: {}", names.join(" · ")), 18, false, false, Some("5F6368"))));
    }
    if opts.include_source_notes {
        out.push(Paragraph::new().add_run(run("Source notes", 18, true, false, None)));
        for n in &data.source_notes {
            let file = n.file_name.as_deref().unwrap_or("source removed");
            let label = n.label.strip_prefix(&format!("{file}, ")).unwrap_or(&n.label);
            let edited = if n.edited { " (edited)" } else { "" };
            out.push(Paragraph::new().add_run(run(&format!("Step {}: {file}, {label}{edited}", n.letter), 18, false, false, Some("5F6368"))));
        }
    }
    out
}

fn styles(docx: Docx) -> Docx {
    docx.add_style(Style::new("Title", StyleType::Paragraph).name("Title").size(36).bold())
        .add_style(Style::new("Heading1", StyleType::Paragraph).name("Heading 1").size(26).bold().outline_lvl(0))
}

pub fn render_docx(data: &ExportData, opts: &ExportOptions) -> Result<Vec<u8>> {
    let mut docx = styles(Docx::new());
    let margin = PageMargin::new().top(MARGIN).bottom(MARGIN).left(MARGIN).right(MARGIN);
    match opts.layout {
        Layout::Quadrant => {
            docx = docx.page_size(A3_LONG, A3_SHORT).page_orient(PageOrientationType::Landscape).page_margin(margin);
            for p in header_paragraphs(data) {
                docx = docx.add_paragraph(p);
            }
            let content_width = A3_LONG - (MARGIN as u32) * 2;
            let col = content_width / 2;
            let cell = |letter: &str| {
                let mut c = TableCell::new().width(col as usize, WidthType::Dxa);
                c = c.add_paragraph(
                    Paragraph::new()
                        .add_run(run(&format!("{letter}. {}", step_name(letter)), 24, true, false, None))
                        .add_run(run(&format!("   {}", state_text(data, letter)), 18, false, false, Some("5F6368"))),
                );
                let empty = Value::Null;
                for b in step_blocks(step_doc(data, letter).unwrap_or(&empty), 20, col - 300) {
                    c = match b {
                        Block::Para(p) => c.add_paragraph(p),
                        // Word requires a paragraph after a nested table in a cell.
                        Block::Table(t) => c.add_table(t).add_paragraph(Paragraph::new()),
                    };
                }
                c
            };
            // A top-left, B bottom-left, C top-right, D bottom-right.
            let table = Table::new(vec![TableRow::new(vec![cell("A"), cell("C")]), TableRow::new(vec![cell("B"), cell("D")])])
                .set_grid(vec![col as usize, col as usize])
                .width(content_width as usize, WidthType::Dxa)
                .layout(TableLayoutType::Fixed);
            docx = docx.add_table(table);
        }
        Layout::Accordion => {
            docx = docx.page_size(A4_SHORT, A4_LONG).page_margin(margin);
            for p in header_paragraphs(data) {
                docx = docx.add_paragraph(p);
            }
            let width = A4_SHORT - (MARGIN as u32) * 2;
            for l in LETTERS {
                // Word headings, which Word can collapse.
                docx = docx.add_paragraph(Paragraph::new().style("Heading1").add_run(Run::new().add_text(format!("{l}. {}", step_name(l)))));
                docx = docx.add_paragraph(Paragraph::new().add_run(run(&state_text(data, l), 18, false, false, Some("5F6368"))));
                let empty = Value::Null;
                for b in step_blocks(step_doc(data, l).unwrap_or(&empty), 22, width) {
                    docx = match b {
                        Block::Para(p) => docx.add_paragraph(p),
                        Block::Table(t) => docx.add_table(t),
                    };
                }
            }
        }
    }
    for p in extras(data, opts) {
        docx = docx.add_paragraph(p);
    }
    docx = docx.footer(footer(data));
    let mut buf = Cursor::new(Vec::new());
    docx.build().pack(&mut buf).map_err(|e| CoreError::new("EXPORT", format!("The Word file could not be written: {e}")))?;
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::super::tests::sample;
    use super::*;
    use std::io::Read;

    fn document_xml(bytes: &[u8]) -> String {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut f = z.by_name("word/document.xml").unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        s
    }

    fn opts(layout: Layout) -> ExportOptions {
        ExportOptions { layout, include_attachments: true, include_source_notes: false }
    }

    #[test]
    fn quadrant_docx_is_a3_landscape_with_a_two_by_two_table() {
        let xml = document_xml(&render_docx(&sample(), &opts(Layout::Quadrant)).unwrap());
        assert!(xml.contains("w:w=\"23811\"") && xml.contains("w:h=\"16838\""), "A3 size");
        assert!(xml.contains("landscape"));
        let (a, c, b, d) = (
            xml.find("A. Define").unwrap(),
            xml.find("C. Implement").unwrap(),
            xml.find("B. Analyze").unwrap(),
            xml.find("D. Validate").unwrap(),
        );
        assert!(a < c && c < b && b < d, "row one holds A then C, row two holds B then D");
    }

    #[test]
    fn accordion_docx_uses_heading_styles_in_order() {
        let xml = document_xml(&render_docx(&sample(), &opts(Layout::Accordion)).unwrap());
        assert_eq!(xml.matches("w:val=\"Heading1\"").count(), 4);
        let (a, b, c, d) = (
            xml.find("A. Define").unwrap(),
            xml.find("B. Analyze").unwrap(),
            xml.find("C. Implement").unwrap(),
            xml.find("D. Validate").unwrap(),
        );
        assert!(a < b && b < c && c < d);
        assert!(xml.contains("Scrap rose"), "step text is present");
    }
}
