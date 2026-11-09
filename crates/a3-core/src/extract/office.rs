//! Word and PowerPoint, parsed directly from OOXML so citations can point at a heading and
//! paragraph or a slide (section 5).

use super::{ExtractedSegment, ImageData};
use crate::error::{CoreError, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde_json::json;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use zip::ZipArchive;

/// Decompression limits against zip bombs.
const MAX_ENTRY_BYTES: u64 = 100 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 20_000;
const MAX_RATIO: u64 = 200;
/// A slide with fewer characters than this that also holds a picture gets read as an image.
const LOW_TEXT_CHARS: usize = 40;
/// Paragraphs are grouped under a heading into passages of about this size.
const PASSAGE_CHARS: usize = 1500;

type Zip = ZipArchive<BufReader<File>>;

fn open_zip(path: &Path) -> Result<Zip> {
    let f = File::open(path)?;
    let z = ZipArchive::new(BufReader::new(f)).map_err(|e| CoreError::new("UNREADABLE", format!("This file is damaged or is not a modern Office file: {e}")))?;
    if z.len() > MAX_ARCHIVE_ENTRIES {
        return Err(CoreError::new("UNREADABLE", "This file has too many parts to read safely."));
    }
    Ok(z)
}

fn read_entry(z: &mut Zip, name: &str) -> Result<Option<Vec<u8>>> {
    let mut e = match z.by_name(name) {
        Ok(e) => e,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(CoreError::new("UNREADABLE", format!("Part {name} could not be read: {err}"))),
    };
    let big = e.size() > MAX_ENTRY_BYTES;
    let bomb = e.compressed_size() > 0 && e.size() / e.compressed_size().max(1) > MAX_RATIO && e.size() > 10 * 1024 * 1024;
    if big || bomb {
        return Err(CoreError::new("UNREADABLE", "This file expands too far to read safely."));
    }
    let mut buf = Vec::with_capacity(e.size().min(MAX_ENTRY_BYTES) as usize);
    // `take` also guards against an entry whose declared size is a lie.
    e.by_ref().take(MAX_ENTRY_BYTES).read_to_end(&mut buf).map_err(|err| CoreError::new("UNREADABLE", format!("Part {name} could not be read: {err}")))?;
    Ok(Some(buf))
}

fn xml_reader(xml: &[u8]) -> Reader<&[u8]> {
    Reader::from_reader(xml)
}

fn local(name: &[u8]) -> &[u8] {
    name.rsplit(|b| *b == b':').next().unwrap_or(name)
}

fn attr(e: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    e.attributes().flatten().find(|a| local(a.key.as_ref()) == key).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

/// Text of one run, with the entities a document can legally hold decoded.
fn decode_text(e: &quick_xml::events::BytesText<'_>) -> String {
    let raw = String::from_utf8_lossy(e.as_ref()).into_owned();
    match quick_xml::escape::unescape(&raw) {
        Ok(s) => s.into_owned(),
        Err(_) => raw,
    }
}

fn entity_text(e: &quick_xml::events::BytesRef<'_>) -> String {
    let raw = String::from_utf8_lossy(e.as_ref()).into_owned();
    match raw.as_str() {
        "amp" => "&".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "quot" => "\"".into(),
        "apos" => "'".into(),
        other => quick_xml::escape::unescape(&format!("&{other};")).map(|s| s.into_owned()).unwrap_or_default(),
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct DocPara {
    pub text: String,
    pub heading: bool,
    pub table_row: bool,
}

pub(super) fn parse_document_xml(xml: &[u8]) -> Result<Vec<DocPara>> {
    let mut r = xml_reader(xml);
    let mut paras = Vec::new();
    let (mut text, mut heading, mut in_t, mut table_depth) = (String::new(), false, false, 0usize);
    let mut cells: Vec<String> = Vec::new();
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"t" => in_t = true,
                b"tbl" => table_depth += 1,
                b"tr" if table_depth == 1 => cells.clear(),
                _ => {}
            },
            Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"pStyle" => {
                    let v = attr(&e, b"val").unwrap_or_default().to_lowercase();
                    if v.starts_with("heading") || v == "title" {
                        heading = true;
                    }
                }
                b"tab" => text.push(' '),
                b"br" => text.push(' '),
                _ => {}
            },
            Ok(Event::Text(t)) if in_t => text.push_str(&decode_text(&t)),
            Ok(Event::GeneralRef(t)) if in_t => text.push_str(&entity_text(&t)),
            Ok(Event::End(e)) => match local(e.name().as_ref()) {
                b"t" => in_t = false,
                b"p" => {
                    let t = text.trim().to_string();
                    if table_depth > 0 {
                        if !t.is_empty() {
                            cells.push(t);
                        }
                    } else if !t.is_empty() {
                        paras.push(DocPara { text: t, heading, table_row: false });
                    }
                    text.clear();
                    heading = false;
                }
                b"tr" if table_depth == 1 => {
                    if !cells.is_empty() {
                        paras.push(DocPara { text: cells.join(" | "), heading: false, table_row: true });
                    }
                    cells.clear();
                }
                b"tbl" => table_depth = table_depth.saturating_sub(1),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(CoreError::new("UNREADABLE", format!("This document's text could not be read: {e}"))),
            _ => {}
        }
        buf.clear();
    }
    Ok(paras)
}

pub fn extract_docx(path: &Path) -> Result<Vec<ExtractedSegment>> {
    let mut z = open_zip(path)?;
    let xml = read_entry(&mut z, "word/document.xml")?.ok_or_else(|| CoreError::new("UNREADABLE", "This Word file has no document body."))?;
    Ok(group_paragraphs(parse_document_xml(&xml)?))
}

/// Passages grouped under their heading. The citation is the heading and the first paragraph number.
pub(super) fn group_paragraphs(paras: Vec<DocPara>) -> Vec<ExtractedSegment> {
    let mut out = Vec::new();
    let mut heading: Option<String> = None;
    let (mut buf, mut start) = (String::new(), 1usize);
    let flush = |out: &mut Vec<ExtractedSegment>, heading: &Option<String>, buf: &mut String, start: usize| {
        if buf.trim().is_empty() {
            buf.clear();
            return;
        }
        let label = match heading {
            Some(h) => format!("{h}, paragraph {start}"),
            None => format!("Paragraph {start}"),
        };
        out.push(ExtractedSegment::new(label, json!({"heading": heading, "paragraph": start}), buf.trim().to_string()));
        buf.clear();
    };
    for (i, p) in paras.into_iter().enumerate() {
        let n = i + 1;
        if p.heading {
            flush(&mut out, &heading, &mut buf, start);
            heading = Some(p.text.clone());
            start = n;
            buf.push_str(&p.text);
            buf.push('\n');
            continue;
        }
        if buf.len() + p.text.len() > PASSAGE_CHARS && !buf.trim().is_empty() {
            flush(&mut out, &heading, &mut buf, start);
            start = n;
        }
        if buf.is_empty() {
            start = n;
        }
        buf.push_str(&p.text);
        buf.push('\n');
    }
    flush(&mut out, &heading, &mut buf, start);
    out
}

fn rels(z: &mut Zip, name: &str) -> Result<HashMap<String, (String, String)>> {
    let mut map = HashMap::new();
    let Some(xml) = read_entry(z, name)? else { return Ok(map) };
    let mut r = xml_reader(&xml);
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if local(e.name().as_ref()) == b"Relationship" => {
                if let (Some(id), Some(ty), Some(t)) = (attr(&e, b"Id"), attr(&e, b"Type"), attr(&e, b"Target")) {
                    map.insert(id, (ty, t));
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(CoreError::new("UNREADABLE", format!("This file's structure could not be read: {e}"))),
            _ => {}
        }
        buf.clear();
    }
    Ok(map)
}

/// Slide part names in presentation order. File numbering does not always match it.
fn slide_order(z: &mut Zip) -> Result<Vec<String>> {
    let pres = read_entry(z, "ppt/presentation.xml")?.ok_or_else(|| CoreError::new("UNREADABLE", "This PowerPoint file has no presentation part."))?;
    let map = rels(z, "ppt/_rels/presentation.xml.rels")?;
    let mut r = xml_reader(&pres);
    let mut out = Vec::new();
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if local(e.name().as_ref()) == b"sldId" => {
                if let Some((_, target)) = attr(&e, b"id").filter(|_| false).or_else(|| attr(&e, b"r:id")).or_else(|| rid(&e)).and_then(|id| map.get(&id)) {
                    out.push(format!("ppt/{}", target.trim_start_matches('/').trim_start_matches("ppt/")));
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(CoreError::new("UNREADABLE", format!("This file's structure could not be read: {e}"))),
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn rid(e: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref().ends_with(b":id")).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

pub(super) fn slide_text(xml: &[u8]) -> Result<String> {
    let mut r = xml_reader(xml);
    let (mut paras, mut cur, mut in_t) = (Vec::new(), String::new(), false);
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"t" => in_t = true,
            Ok(Event::Text(t)) if in_t => cur.push_str(&decode_text(&t)),
            Ok(Event::GeneralRef(t)) if in_t => cur.push_str(&entity_text(&t)),
            Ok(Event::End(e)) => match local(e.name().as_ref()) {
                b"t" => in_t = false,
                b"p" => {
                    if !cur.trim().is_empty() {
                        paras.push(cur.trim().to_string());
                    }
                    cur.clear();
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(CoreError::new("UNREADABLE", format!("A slide's text could not be read: {e}"))),
            _ => {}
        }
        buf.clear();
    }
    Ok(paras.join("\n"))
}

fn rels_path(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

fn resolve_target(part: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = part.split('/').collect();
    parts.pop();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

pub fn extract_pptx(path: &Path) -> Result<Vec<ExtractedSegment>> {
    let mut z = open_zip(path)?;
    let mut out = Vec::new();
    for (i, part) in slide_order(&mut z)?.into_iter().enumerate() {
        let n = i + 1;
        let Some(xml) = read_entry(&mut z, &part)? else { continue };
        let body = slide_text(&xml)?;
        let links = rels(&mut z, &rels_path(&part))?;
        let has_image = links.values().any(|(ty, _)| ty.ends_with("/image"));
        let mut text = body.clone();
        if let Some((_, target)) = links.values().find(|(ty, _)| ty.ends_with("/notesSlide")) {
            if let Some(notes_xml) = read_entry(&mut z, &resolve_target(&part, target))? {
                let notes = slide_text(&notes_xml)?;
                // A notes page repeats the slide number as text, so bare digits are not notes.
                let notes: Vec<&str> = notes.lines().filter(|l| !l.trim().chars().all(|c| c.is_ascii_digit())).collect();
                if !notes.is_empty() {
                    text.push_str("\nNotes: ");
                    text.push_str(&notes.join("\n"));
                }
            }
        }
        let low_text = body.chars().filter(|c| !c.is_whitespace()).count() < LOW_TEXT_CHARS;
        out.push(ExtractedSegment {
            needs_image: low_text && has_image,
            ..ExtractedSegment::new(format!("Slide {n}"), json!({"slide": n}), text.trim().to_string())
        });
    }
    Ok(out)
}

pub fn slide_images(path: &Path, slide: usize) -> Result<Vec<ImageData>> {
    let mut z = open_zip(path)?;
    let order = slide_order(&mut z)?;
    let part = order.get(slide.saturating_sub(1)).ok_or_else(|| CoreError::new("UNREADABLE", "That slide does not exist."))?.clone();
    let links = rels(&mut z, &rels_path(&part))?;
    let mut out = Vec::new();
    for (ty, target) in links.values() {
        if ty.ends_with("/image") {
            if let Some(bytes) = read_entry(&mut z, &resolve_target(&part, target))? {
                // Vector formats such as EMF cannot be decoded here, so they are skipped.
                if let Ok(img) = super::pdf::image_from_bytes(&bytes) {
                    out.push(img);
                }
            }
        }
        if out.len() >= 3 {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_xml_keeps_headings_tables_and_entities() {
        let xml = br#"<w:document xmlns:w="x"><w:body>
          <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Problem</w:t></w:r></w:p>
          <w:p><w:r><w:t>Scrap &amp; rework </w:t></w:r><w:r><w:t>rose to 3.5%</w:t></w:r></w:p>
          <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Line</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>4</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
        </w:body></w:document>"#;
        let paras = parse_document_xml(xml).unwrap();
        assert_eq!(paras.len(), 3);
        assert!(paras[0].heading);
        assert_eq!(paras[1].text, "Scrap & rework rose to 3.5%");
        assert_eq!(paras[2].text, "Line | 4");
        assert!(paras[2].table_row);
        let segs = group_paragraphs(paras);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].label, "Problem, paragraph 1");
        assert!(segs[0].text.contains("Scrap & rework"));
    }

    #[test]
    fn slide_text_splits_paragraphs() {
        let xml = br#"<p:sld xmlns:a="a" xmlns:p="p"><a:p><a:r><a:t>Title</a:t></a:r></a:p><a:p><a:r><a:t>Point </a:t></a:r><a:r><a:t>one</a:t></a:r></a:p></p:sld>"#;
        assert_eq!(slide_text(xml).unwrap(), "Title\nPoint one");
    }

    #[test]
    fn relative_targets_resolve() {
        assert_eq!(resolve_target("ppt/slides/slide1.xml", "../media/image1.png"), "ppt/media/image1.png");
        assert_eq!(rels_path("ppt/slides/slide1.xml"), "ppt/slides/_rels/slide1.xml.rels");
    }
}
