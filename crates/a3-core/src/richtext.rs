//! Step content is stored as the editor's structured JSON. This module reads it for search, AI
//! and export, and builds it when a proposal is accepted.

use crate::domain::normalize;
use html_escape::{encode_double_quoted_attribute as attr, encode_text as esc};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub segment_id: String,
    pub label: String,
    pub edited: bool,
}

pub fn empty_doc() -> Value {
    json!({"type": "doc", "content": [{"type": "paragraph"}]})
}

pub fn plain_text(doc: &Value) -> String {
    let mut out = String::new();
    write_plain(doc, &mut out);
    out.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string()
}

fn write_plain(node: &Value, out: &mut String) {
    match node.get("type").and_then(Value::as_str).unwrap_or("") {
        "text" => out.push_str(node.get("text").and_then(Value::as_str).unwrap_or("")),
        "hardBreak" => out.push('\n'),
        "paragraph" | "heading" => {
            children(node).iter().for_each(|c| write_plain(c, out));
            out.push('\n');
        }
        "tableRow" => {
            let cells: Vec<String> = children(node)
                .iter()
                .map(|cell| {
                    let mut s = String::new();
                    write_plain(cell, &mut s);
                    s.trim().replace('\n', " ")
                })
                .collect();
            out.push_str(&cells.join(" | "));
            out.push('\n');
        }
        _ => children(node).iter().for_each(|c| write_plain(c, out)),
    }
}

fn children(node: &Value) -> &[Value] {
    node.get("content").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn source_mark(segment_id: Option<&str>, label: Option<&str>, original: &str, from_image: bool) -> Option<Value> {
    segment_id.map(|id| {
        json!({"type": "source", "attrs": {
            "segmentId": id,
            "label": label.unwrap_or("Source"),
            "original": original,
            "edited": false,
            "fromImage": from_image,
        }})
    })
}

/// A paragraph holding accepted text, carrying its source chip when it has one (FR-A3-05).
pub fn sourced_paragraph(text: &str, segment_id: Option<&str>, label: Option<&str>, from_image: bool) -> Value {
    let mut node = json!({"type": "text", "text": text});
    if let Some(mark) = source_mark(segment_id, label, text, from_image) {
        node["marks"] = json!([mark]);
    }
    json!({"type": "paragraph", "content": [node]})
}

/// Appends a block. A document holding only its starting empty paragraph is replaced, not extended.
pub fn append_block(doc: &mut Value, block: Value) {
    if !doc.get("content").is_some_and(Value::is_array) {
        *doc = json!({"type": "doc", "content": []});
    }
    let content = doc["content"].as_array_mut().expect("just ensured an array");
    let only_empty = content.len() == 1
        && content[0].get("type").and_then(Value::as_str) == Some("paragraph")
        && children(&content[0]).is_empty();
    if only_empty {
        content.clear();
    }
    content.push(block);
}

/// Replaces `old` with `block`: the whole paragraph when it equals `old`, or inside one text node
/// when `old` is part of it. Returns false when `old` is not found, so the caller can append.
pub fn replace_text(doc: &mut Value, old: &str, text: &str, segment_id: Option<&str>, label: Option<&str>, from_image: bool) -> bool {
    let target = normalize(old);
    if target.is_empty() {
        return false;
    }
    replace_in(doc, &target, old, text, segment_id, label, from_image)
}

fn replace_in(node: &mut Value, target: &str, old: &str, text: &str, seg: Option<&str>, label: Option<&str>, img: bool) -> bool {
    let Some(content) = node.get_mut("content").and_then(Value::as_array_mut) else {
        return false;
    };
    for i in 0..content.len() {
        let kind = content[i].get("type").and_then(Value::as_str).unwrap_or("").to_string();
        if kind == "paragraph" {
            if normalize(&plain_text(&content[i])) == target {
                content[i] = sourced_paragraph(text, seg, label, img);
                return true;
            }
            if let Some(parts) = content[i].get_mut("content").and_then(Value::as_array_mut) {
                for j in 0..parts.len() {
                    let Some(t) = parts[j].get("text").and_then(Value::as_str) else { continue };
                    let Some(at) = t.find(old) else { continue };
                    let (before, after) = (t[..at].to_string(), t[at + old.len()..].to_string());
                    let marks = parts[j].get("marks").cloned();
                    let plain = |s: String| {
                        let mut n = json!({"type": "text", "text": s});
                        if let Some(m) = &marks {
                            n["marks"] = m.clone();
                        }
                        n
                    };
                    let mut middle = json!({"type": "text", "text": text});
                    if let Some(m) = source_mark(seg, label, text, img) {
                        middle["marks"] = json!([m]);
                    }
                    let mut new_parts = Vec::new();
                    if !before.is_empty() {
                        new_parts.push(plain(before));
                    }
                    new_parts.push(middle);
                    if !after.is_empty() {
                        new_parts.push(plain(after));
                    }
                    parts.splice(j..=j, new_parts);
                    return true;
                }
            }
        } else if replace_in(&mut content[i], target, old, text, seg, label, img) {
            return true;
        }
    }
    false
}

/// Every source cited in a step, in order and without repeats. Used for export source notes.
pub fn source_refs(doc: &Value) -> Vec<SourceRef> {
    let mut out: Vec<SourceRef> = Vec::new();
    collect_refs(doc, &mut out);
    out
}

fn collect_refs(node: &Value, out: &mut Vec<SourceRef>) {
    if let Some(marks) = node.get("marks").and_then(Value::as_array) {
        for m in marks {
            if m.get("type").and_then(Value::as_str) == Some("source") {
                let a = &m["attrs"];
                let r = SourceRef {
                    segment_id: a["segmentId"].as_str().unwrap_or("").to_string(),
                    label: a["label"].as_str().unwrap_or("Source").to_string(),
                    edited: a["edited"].as_bool().unwrap_or(false),
                };
                if !out.iter().any(|x| x.segment_id == r.segment_id) {
                    out.push(r);
                }
            }
        }
    }
    children(node).iter().for_each(|c| collect_refs(c, out));
}

/// Only image sources the editor itself stores (data URIs) are written out, so an export cannot
/// reach out to a remote address and always works without a network (FR-EXP-07).
pub fn safe_image_src(src: &str) -> Option<&str> {
    let ok = ["data:image/png;base64,", "data:image/jpeg;base64,", "data:image/gif;base64,", "data:image/webp;base64,"];
    ok.iter().any(|p| src.starts_with(p)).then_some(src)
}

pub fn to_html(doc: &Value) -> String {
    let mut out = String::new();
    write_html(doc, &mut out);
    out
}

fn write_children(node: &Value, out: &mut String) {
    children(node).iter().for_each(|c| write_html(c, out));
}

fn wrap(node: &Value, tag: &str, out: &mut String) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    write_children(node, out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn write_html(node: &Value, out: &mut String) {
    match node.get("type").and_then(Value::as_str).unwrap_or("") {
        "doc" => write_children(node, out),
        "paragraph" => {
            if children(node).is_empty() {
                return;
            }
            wrap(node, "p", out)
        }
        "bulletList" => wrap(node, "ul", out),
        "orderedList" => wrap(node, "ol", out),
        "listItem" => wrap(node, "li", out),
        "table" => wrap(node, "table", out),
        "tableRow" => wrap(node, "tr", out),
        "tableHeader" => wrap(node, "th", out),
        "tableCell" => wrap(node, "td", out),
        "hardBreak" => out.push_str("<br>"),
        "image" => {
            if let Some(src) = node["attrs"]["src"].as_str().and_then(safe_image_src) {
                out.push_str(&format!("<img src=\"{}\" alt=\"\">", attr(src)));
            }
        }
        "text" => {
            let text = esc(node.get("text").and_then(Value::as_str).unwrap_or("")).to_string();
            let marks = node.get("marks").and_then(Value::as_array);
            let has = |t: &str| marks.is_some_and(|m| m.iter().any(|x| x.get("type").and_then(Value::as_str) == Some(t)));
            let mut s = text;
            if has("bold") {
                s = format!("<strong>{s}</strong>");
            }
            if has("italic") {
                s = format!("<em>{s}</em>");
            }
            out.push_str(&s);
        }
        _ => write_children(node, out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_covers_lists_and_tables() {
        let doc = json!({"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "One"}]},
            {"type": "bulletList", "content": [{"type": "listItem", "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Two"}]}]}]},
            {"type": "table", "content": [{"type": "tableRow", "content": [
                {"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "a"}]}]},
                {"type": "tableCell", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "b"}]}]}]}]}
        ]});
        assert_eq!(plain_text(&doc), "One\nTwo\na | b");
    }

    #[test]
    fn append_replaces_the_starting_empty_paragraph() {
        let mut doc = empty_doc();
        append_block(&mut doc, sourced_paragraph("Hello", Some("s1"), Some("p.1"), false));
        assert_eq!(doc["content"].as_array().unwrap().len(), 1);
        append_block(&mut doc, sourced_paragraph("World", None, None, false));
        assert_eq!(plain_text(&doc), "Hello\nWorld");
        assert_eq!(source_refs(&doc).len(), 1);
    }

    #[test]
    fn replace_handles_whole_paragraph_and_inner_text() {
        let mut doc = empty_doc();
        append_block(&mut doc, sourced_paragraph("Scrap was 3.5%.", None, None, false));
        append_block(&mut doc, sourced_paragraph("Line 4 stopped. Output fell.", None, None, false));
        assert!(replace_text(&mut doc, "Scrap was 3.5%.", "Scrap was 4.1%.", Some("s2"), Some("p.2"), false));
        assert!(replace_text(&mut doc, "Output fell.", "Output fell 20%.", None, None, false));
        assert_eq!(plain_text(&doc), "Scrap was 4.1%.\nLine 4 stopped. Output fell 20%.");
        assert!(!replace_text(&mut doc, "not present", "x", None, None, false));
    }

    #[test]
    fn html_is_escaped_and_blocks_remote_images() {
        let doc = json!({"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "<script>alert(1)</script>", "marks": [{"type": "bold"}]}]},
            {"type": "image", "attrs": {"src": "https://evil.example/x.png"}},
            {"type": "image", "attrs": {"src": "data:image/png;base64,AAAA"}}
        ]});
        let html = to_html(&doc);
        assert!(html.contains("<strong>&lt;script&gt;"));
        assert!(!html.contains("evil.example"));
        assert!(html.contains("data:image/png;base64,AAAA"));
    }
}
