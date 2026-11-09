//! Reads real files of each type, built here so no binary fixtures live in the repo. Each must yield
//! passages that point to the right place (section 8).

use a3_core::extract::{classify, extract_file, segment_images, FileKind};
use std::io::Write;
use std::path::{Path, PathBuf};

fn temp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("a3-extract-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_zip(path: &Path, entries: &[(&str, &str)]) {
    let f = std::fs::File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    for (name, body) in entries {
        z.start_file(*name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
        z.write_all(body.as_bytes()).unwrap();
    }
    z.finish().unwrap();
}

/// A minimal one-font PDF. `pages` holds each page's single text line, or None for a blank page.
fn write_pdf(path: &Path, pages: &[Option<&str>]) {
    let mut objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        String::new(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ];
    let mut kids = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let page_obj = 4 + i * 2;
        kids.push(format!("{page_obj} 0 R"));
        let stream = match page {
            Some(t) => format!("BT /F1 12 Tf 50 700 Td ({t}) Tj ET"),
            None => String::new(),
        };
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>", page_obj + 1));
        objects.push(format!("<< /Length {} >>\nstream\n{}\nendstream", stream.len(), stream));
    }
    objects[1] = format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len());
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, o));
    }
    let xref = out.len();
    out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
    for off in offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1));
    std::fs::write(path, out).unwrap();
}

fn pdfium_ready() -> bool {
    if std::env::var("A3_PDFIUM_DIR").is_err() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../src-tauri/resources");
        std::env::set_var("A3_PDFIUM_DIR", dir);
    }
    Path::new(&std::env::var("A3_PDFIUM_DIR").unwrap()).join("pdfium.dll").exists()
}

#[test]
fn pdf_pages_become_passages_and_blank_pages_are_flagged() {
    assert!(pdfium_ready(), "pdfium.dll is needed for this test");
    let dir = temp_dir();
    let p = dir.join("report.pdf");
    write_pdf(&p, &[Some("Scrap rose to 3.5 percent on line 4"), None, Some("Target is 1.0 percent")]);
    let segs = extract_file(&p, "report.pdf").unwrap();
    assert_eq!(segs.len(), 3);
    assert_eq!(segs[0].label, "Page 1");
    assert_eq!(segs[0].location["page"], 1);
    assert!(segs[0].text.contains("Scrap rose to 3.5 percent"));
    assert!(!segs[0].needs_image);
    assert!(segs[1].needs_image, "a page with no text counts as scanned");
    assert_eq!(segs[2].location["page"], 3);
    assert!(segs[2].text.contains("Target"));

    // A scanned page can be drawn for a vision model.
    let imgs = segment_images(&p, "report.pdf", &segs[1].location).unwrap();
    assert_eq!(imgs.len(), 1);
    assert_eq!(imgs[0].mime, "image/png");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn docx_keeps_headings_paragraphs_and_tables() {
    let dir = temp_dir();
    let p = dir.join("report.docx");
    write_zip(&p, &[("word/document.xml", r#"<?xml version="1.0"?><w:document xmlns:w="w"><w:body>
        <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Problem</w:t></w:r></w:p>
        <w:p><w:r><w:t>Scrap rose to 3.5%.</w:t></w:r></w:p>
        <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Cause</w:t></w:r></w:p>
        <w:p><w:r><w:t>Die 4 wore out.</w:t></w:r></w:p>
        <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Die</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Worn</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
    </w:body></w:document>"#)]);
    let segs = extract_file(&p, "report.docx").unwrap();
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].label, "Problem, paragraph 1");
    assert_eq!(segs[0].location["heading"], "Problem");
    assert!(segs[0].text.contains("Scrap rose to 3.5%."));
    assert_eq!(segs[1].location["heading"], "Cause");
    assert!(segs[1].text.contains("Die | Worn"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pptx_follows_presentation_order_and_includes_notes() {
    let dir = temp_dir();
    let p = dir.join("deck.pptx");
    let slide = |t: &str| format!(r#"<p:sld xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:sld>"#);
    let (s1, s2, n1) = (slide("Second slide by file name"), slide("First slide by file name"), slide("A speaker note"));
    write_zip(&p, &[
        ("ppt/presentation.xml", r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="1" r:id="rId2"/><p:sldId id="2" r:id="rId1"/></p:sldIdLst></p:presentation>"#),
        ("ppt/_rels/presentation.xml.rels", r#"<Relationships><Relationship Id="rId1" Type="x/slide" Target="slides/slide1.xml"/><Relationship Id="rId2" Type="x/slide" Target="slides/slide2.xml"/></Relationships>"#),
        ("ppt/slides/slide1.xml", &s1),
        ("ppt/slides/slide2.xml", &s2),
        ("ppt/slides/_rels/slide2.xml.rels", r#"<Relationships><Relationship Id="rId1" Type="x/notesSlide" Target="../notesSlides/notesSlide1.xml"/></Relationships>"#),
        ("ppt/notesSlides/notesSlide1.xml", &n1),
    ]);
    let segs = extract_file(&p, "deck.pptx").unwrap();
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].label, "Slide 1");
    assert_eq!(segs[0].location["slide"], 1);
    assert!(segs[0].text.contains("First slide by file name"), "order comes from the presentation, not the file name");
    assert!(segs[0].text.contains("Notes: A speaker note"));
    assert!(segs[1].text.contains("Second slide by file name"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn xlsx_cites_sheet_and_range() {
    let dir = temp_dir();
    let p = dir.join("data.xlsx");
    write_zip(&p, &[
        ("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#),
        ("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#),
        ("xl/workbook.xml", r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Scrap" sheetId="1" r:id="rId1"/></sheets></workbook>"#),
        ("xl/_rels/workbook.xml.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#),
        ("xl/worksheets/sheet1.xml", r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Line</t></is></c><c r="B1" t="inlineStr"><is><t>Scrap</t></is></c></row><row r="2"><c r="A2"><v>4</v></c><c r="B2"><v>3.5</v></c></row></sheetData></worksheet>"#),
    ]);
    let segs = extract_file(&p, "data.xlsx").unwrap();
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].label, "Scrap!A1:B2");
    assert_eq!(segs[0].location["sheet"], "Scrap");
    assert_eq!(segs[0].group_name.as_deref(), Some("Scrap"));
    assert!(segs[0].text.contains("Columns: Line | Scrap"));
    assert!(segs[0].text.contains("Row 2: 4 | 3.5"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn csv_and_text_read_with_locations() {
    let dir = temp_dir();
    let c = dir.join("d.csv");
    std::fs::write(&c, "a,b\n1,2\n").unwrap();
    let segs = extract_file(&c, "d.csv").unwrap();
    assert_eq!(segs[0].label, "d!A1:B2");
    let t = dir.join("n.md");
    std::fs::write(&t, "# Title\nBody line\n").unwrap();
    assert_eq!(extract_file(&t, "n.md").unwrap()[0].label, "Lines 1-2");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn damaged_and_hostile_files_fail_cleanly() {
    let dir = temp_dir();
    let garbage = dir.join("x");
    std::fs::write(&garbage, b"\x00\x01 not a real file").unwrap();
    for name in ["x.docx", "x.pptx", "x.xlsx"] {
        assert_eq!(extract_file(&garbage, name).unwrap_err().code, "UNREADABLE", "{name}");
    }
    assert!(pdfium_ready());
    assert_eq!(extract_file(&garbage, "x.pdf").unwrap_err().code, "UNREADABLE");

    // A zip bomb: a highly compressible entry over the expansion limit is refused, not inflated.
    let bomb = dir.join("bomb.docx");
    let f = std::fs::File::create(&bomb).unwrap();
    let mut z = zip::ZipWriter::new(f);
    z.start_file("word/document.xml", zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
    let chunk = vec![b'a'; 1024 * 1024];
    for _ in 0..30 {
        z.write_all(&chunk).unwrap();
    }
    z.finish().unwrap();
    assert_eq!(extract_file(&bomb, "bomb.docx").unwrap_err().code, "UNREADABLE");

    // Malformed XML inside a valid zip.
    let badxml = dir.join("bad.docx");
    write_zip(&badxml, &[("word/document.xml", "<w:p><w:t>unclosed</w:wrong>")]);
    assert_eq!(extract_file(&badxml, "bad.docx").unwrap_err().code, "UNREADABLE");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unknown_types_are_stored_only() {
    assert_eq!(classify("drawing.dwg"), FileKind::Other);
    assert!(extract_file(Path::new("missing"), "drawing.dwg").unwrap().is_empty());
}
