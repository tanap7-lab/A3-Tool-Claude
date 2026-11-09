//! The HTML template behind both HTML and PDF export (section 5). Output is one file with inline
//! styles and images that works without a network (FR-EXP-07).

use super::{ExportData, ExportOptions, Layout};
use crate::domain::{step_name, LETTERS};
use crate::richtext;
use html_escape::{encode_double_quoted_attribute as attr, encode_text as esc};

/// The printed size of a quadrant's text area, shared with the fit meter in the interface.
pub const QUAD_BODY_WIDTH_MM: f32 = 192.0;
pub const QUAD_BODY_HEIGHT_MM: f32 = 96.0;

const CSS_COMMON: &str = r#"
:root { color-scheme: light; }
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; background: #ffffff; color: #202124; }
body { font-family: Roboto, "Segoe UI", Arial, sans-serif; font-size: 11pt; line-height: 1.35; -webkit-print-color-adjust: exact; print-color-adjust: exact; }
h1 { font-size: 16pt; font-weight: 500; margin: 0 0 1mm; }
.meta { display: flex; flex-wrap: wrap; gap: 1mm 6mm; font-size: 9pt; color: #5f6368; }
.meta b { color: #202124; font-weight: 500; }
.meta a { color: #1a73e8; text-decoration: none; word-break: break-all; }
.strip { display: inline-flex; gap: 1mm; vertical-align: middle; }
.seg { display: inline-block; width: 7mm; line-height: 4.4mm; text-align: center; font-size: 8pt; font-weight: 500; border-radius: 1mm; border: 0.3mm solid #dadce0; color: #5f6368; background: #fff; }
.seg.closed { background: #188038; border-color: #188038; color: #fff; }
.pill { display: inline-block; padding: 0 2.5mm; border-radius: 3mm; font-size: 8.5pt; font-weight: 500; background: #e8f0fe; color: #1765cc; }
.pill.closed { background: #e6f4ea; color: #137333; }
.withdrawn { background: #fce8e6; color: #c5221f; padding: 1mm 3mm; font-size: 9pt; margin-top: 1mm; }
.head-name { font-weight: 500; font-size: 11pt; }
.step-state { font-size: 8.5pt; color: #5f6368; font-weight: 400; }
.step-state.closed { color: #137333; }
.body p { margin: 0 0 0.45em; }
.body ul, .body ol { margin: 0 0 0.45em; padding-left: 1.3em; }
.body li p { margin: 0; }
.body table { border-collapse: collapse; width: 100%; margin: 0 0 0.45em; font-size: 0.92em; }
.body th, .body td { border: 0.25mm solid #dadce0; padding: 0.4mm 1.2mm; text-align: left; vertical-align: top; }
.body th { background: #f8f9fa; font-weight: 500; }
.body img { display: block; max-width: 100%; height: auto; margin: 0 0 0.45em; }
.empty { color: #5f6368; font-style: italic; }
.extras { font-size: 8.5pt; color: #5f6368; }
.extras b { color: #202124; font-weight: 500; }
.extras ul { margin: 0.5mm 0 0; padding-left: 4mm; }
.page-footer { font-size: 8pt; color: #5f6368; margin-top: 2mm; }
@media print { .page-footer { display: none; } }
"#;

const CSS_QUADRANT: &str = r#"
@page { size: 420mm 297mm; margin: 8mm 10mm 14mm; }
.sheet { width: 400mm; height: 275mm; display: flex; flex-direction: column; gap: 2.5mm; overflow: hidden; }
.head { flex: none; }
.grid { flex: 1 1 auto; min-height: 0; display: grid; grid-template-columns: 1fr 1fr; grid-template-rows: minmax(0, 1fr) minmax(0, 1fr); grid-template-areas: "A C" "B D"; gap: 2.5mm; }
.quad { border: 0.3mm solid #dadce0; border-radius: 1.5mm; display: flex; flex-direction: column; min-height: 0; overflow: hidden; }
.quad-head { flex: none; display: flex; align-items: baseline; justify-content: space-between; gap: 3mm; padding: 1.2mm 3mm; background: #f8f9fa; border-bottom: 0.3mm solid #dadce0; }
.quad-body { flex: 1 1 auto; min-height: 0; overflow: hidden; padding: 2mm 3mm; font-size: 11pt; }
.quad-body img { max-height: 70mm; }
.more-note { font-size: 8pt; color: #5f6368; font-style: italic; margin: 1mm 0 0; }
.extras { flex: none; }
.continued { width: 400mm; break-before: page; padding-top: 2mm; }
.continued h2 { font-size: 12pt; font-weight: 500; margin: 0 0 1.5mm; }
.continued .body { font-size: 8pt; column-count: 2; column-gap: 6mm; margin-bottom: 4mm; }
.notes-page { width: 400mm; break-before: page; }
@media screen { html { background: #f8f9fa; } body { width: 420mm; margin: 4mm auto; padding: 8mm 10mm; box-shadow: 0 0 2mm rgba(60,64,67,.3); } }
"#;

const CSS_ACCORDION: &str = r#"
@page { size: 210mm 297mm; margin: 14mm 15mm 16mm; }
.sheet { max-width: 180mm; margin: 0 auto; }
.head { margin-bottom: 4mm; padding-bottom: 3mm; border-bottom: 0.3mm solid #dadce0; }
details { border: 0.3mm solid #dadce0; border-radius: 1.5mm; margin-bottom: 3mm; }
summary { cursor: pointer; padding: 2mm 3mm; background: #f8f9fa; list-style-position: inside; border-radius: 1.5mm; }
summary h2 { display: inline; font-size: 12pt; font-weight: 500; margin: 0; }
summary .step-state { margin-left: 3mm; }
.body { padding: 2.5mm 3mm; }
.extras { margin-top: 4mm; }
@media print { details { border: none; margin-bottom: 4mm; } summary { list-style: none; background: none; padding: 0 0 1mm; border-bottom: 0.3mm solid #dadce0; } .body { padding: 2mm 0; } details { break-inside: auto; } }
@media screen { html { background: #f8f9fa; } body { background: #fff; max-width: 210mm; margin: 4mm auto; padding: 8mm 12mm; box-shadow: 0 0 2mm rgba(60,64,67,.3); } }
"#;

/// Section 11, "when a quadrant overflows". Text scales from 11 pt to 8 pt, images shrink, then whole
/// blocks move to a continued page, because content must never be cut.
const FIT_SCRIPT: &str = r#"
(function () {
  var MAX = 11, MIN = 8, report = {};
  function over(b) { return b.scrollHeight > b.clientHeight + 1; }
  var sheet = document.querySelector('.sheet');
  var after = sheet;
  document.querySelectorAll('.quad').forEach(function (q) {
    var letter = q.getAttribute('data-letter');
    var b = q.querySelector('.quad-body');
    var size = MAX;
    b.style.fontSize = size + 'pt';
    while (over(b) && size > MIN) { size -= 0.5; b.style.fontSize = size + 'pt'; }
    if (over(b)) {
      [50, 35, 22].forEach(function (mm) {
        if (over(b)) b.querySelectorAll('img').forEach(function (i) { i.style.maxHeight = mm + 'mm'; });
      });
    }
    var moved = [];
    while (over(b) && b.lastElementChild) { moved.unshift(b.lastElementChild); b.removeChild(b.lastElementChild); }
    if (moved.length) {
      var sec = document.createElement('section');
      sec.className = 'continued';
      var h = document.createElement('h2');
      h.textContent = q.getAttribute('data-title') + ' (continued)';
      var body = document.createElement('div');
      body.className = 'body';
      moved.forEach(function (m) { body.appendChild(m); });
      sec.appendChild(h); sec.appendChild(body);
      after.parentNode.insertBefore(sec, after.nextSibling);
      after = sec;
      var note = document.createElement('p');
      note.className = 'more-note';
      note.textContent = 'Continued on a later page.';
      b.appendChild(note);
      while (over(b) && note.previousElementSibling) {
        var prev = note.previousElementSibling;
        body.insertBefore(prev, body.firstChild);
      }
    }
    report[letter] = { size: size, continued: moved.length > 0 };
  });
  window.__a3fit = report;
  try { if (window.parent !== window) window.parent.postMessage({ a3fit: report }, '*'); } catch (e) {}
})();
"#;

fn step_state_html(data: &ExportData, letter: &str) -> String {
    let Some(step) = data.steps.iter().find(|s| s.letter == letter) else { return String::new() };
    if step.state == "closed" {
        let date = step.closed_at.map(|d| d.format(", %Y-%m-%d").to_string()).unwrap_or_default();
        format!("<span class=\"step-state closed\">Closed{date}</span>")
    } else {
        "<span class=\"step-state\">Open</span>".to_string()
    }
}

fn step_body(data: &ExportData, letter: &str) -> String {
    let html = data.steps.iter().find(|s| s.letter == letter).map(|s| richtext::to_html(&s.content)).unwrap_or_default();
    if html.trim().is_empty() {
        "<p class=\"empty\">Nothing recorded yet.</p>".to_string()
    } else {
        html
    }
}

fn header(data: &ExportData) -> String {
    let s = &data.sheet;
    let strip: String = LETTERS
        .iter()
        .map(|l| {
            let closed = data.steps.iter().any(|st| st.letter == *l && st.state == "closed");
            format!("<span class=\"seg{}\">{l}</span>", if closed { " closed" } else { "" })
        })
        .collect();
    let title = if s.title.trim().is_empty() { "Untitled A3" } else { s.title.as_str() };
    let date = |d: Option<chrono::NaiveDate>| d.map(|x| x.to_string()).unwrap_or_else(|| "not set".into());
    let closed_pill = if s.status == "Closed" { " closed" } else { "" };
    let withdrawn = match (&s.withdrawn_at, &s.withdrawn_reason) {
        (Some(_), r) => format!("<div class=\"withdrawn\">Withdrawn: {}</div>", esc(r.as_deref().unwrap_or(""))),
        _ => String::new(),
    };
    format!(
        r#"<header class="head"><h1>{title}</h1><div class="meta">
<span><b>{number}</b></span><span>Owner <b>{owner}</b></span><span>Project <b>{project}</b></span>
<span>Registered <b>{registered}</b></span><span>Target <b>{target}</b></span>
<span><span class="pill{closed_pill}">{status}</span></span><span class="strip">{strip}</span>
<span><a href="{link_attr}">{link}</a></span></div>{withdrawn}</header>"#,
        title = esc(title),
        number = esc(&s.number),
        owner = esc(&s.owner_name),
        project = esc(&s.project_name),
        registered = s.registered_on,
        target = date(s.target_close_date),
        status = esc(&s.status),
        link_attr = attr(&data.link),
        link = esc(&data.link),
    )
}

fn attachments_html(data: &ExportData) -> String {
    if data.attachments.is_empty() {
        return String::new();
    }
    let names: Vec<String> = data
        .attachments
        .iter()
        .map(|a| if a.version > 1 { format!("{} (v{})", esc(&a.file_name), a.version) } else { esc(&a.file_name).to_string() })
        .collect();
    format!("<div class=\"extras\"><b>Attachments:</b> {}</div>", names.join(" · "))
}

fn notes_html(data: &ExportData) -> String {
    if data.source_notes.is_empty() {
        return "<div class=\"extras\"><b>Source notes:</b> no sourced text in this A3.</div>".to_string();
    }
    let items: String = data
        .source_notes
        .iter()
        .map(|n| {
            let file = n.file_name.as_deref().unwrap_or("source removed");
            let label = n.label.strip_prefix(&format!("{file}, ")).unwrap_or(&n.label);
            let edited = if n.edited { " (edited)" } else { "" };
            format!("<li>Step {}: {}, {}{edited}</li>", n.letter, esc(file), esc(label))
        })
        .collect();
    format!("<div class=\"extras\"><b>Source notes</b><ul>{items}</ul></div>")
}

fn page_title(data: &ExportData) -> String {
    format!("{} {}", data.sheet.number, data.sheet.title)
}

pub fn render_html(data: &ExportData, opts: &ExportOptions) -> String {
    let attachments = if opts.include_attachments { attachments_html(data) } else { String::new() };
    let notes = if opts.include_source_notes { notes_html(data) } else { String::new() };
    let footer = format!("<div class=\"page-footer\">{}</div>", esc(&data.footer_text()));
    let (css, body, script) = match opts.layout {
        Layout::Quadrant => {
            let quads: String = LETTERS
                .iter()
                .map(|l| {
                    let title = format!("{l}. {}", step_name(l));
                    format!(
                        r#"<section class="quad" data-letter="{l}" data-title="{title_attr}" style="grid-area:{l}"><div class="quad-head"><span class="head-name">{title_text}</span>{state}</div><div class="quad-body body">{body}</div></section>"#,
                        title_attr = attr(&title),
                        title_text = esc(&title),
                        state = step_state_html(data, l),
                        body = step_body(data, l),
                    )
                })
                .collect();
            let notes_page = if notes.is_empty() { String::new() } else { format!("<section class=\"notes-page\">{notes}</section>") };
            (
                CSS_QUADRANT,
                format!(
                    "<main class=\"sheet\">{}<div class=\"grid\">{quads}</div>{attachments}</main>{notes_page}{footer}",
                    header(data)
                ),
                FIT_SCRIPT,
            )
        }
        Layout::Accordion => {
            let sections: String = LETTERS
                .iter()
                .map(|l| {
                    format!(
                        r#"<details open><summary><h2>{l}. {name}</h2>{state}</summary><div class="body">{body}</div></details>"#,
                        name = esc(step_name(l)),
                        state = step_state_html(data, l),
                        body = step_body(data, l),
                    )
                })
                .collect();
            (CSS_ACCORDION, format!("<main class=\"sheet\">{}{sections}{attachments}{notes}{footer}</main>", header(data)), "")
        }
    };
    let script_tag = if script.is_empty() { String::new() } else { format!("<script>{script}</script>") };
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><style>{CSS_COMMON}{css}</style></head><body>{body}{script_tag}</body></html>",
        esc(&page_title(data))
    )
}

#[cfg(test)]
mod tests {
    use super::super::tests::sample;
    use super::*;

    fn opts(layout: Layout) -> ExportOptions {
        ExportOptions { layout, include_attachments: true, include_source_notes: false }
    }

    #[test]
    fn quadrant_places_steps_where_they_print() {
        let html = render_html(&sample(), &opts(Layout::Quadrant));
        assert!(html.contains(r#"grid-template-areas: "A C" "B D""#));
        for l in LETTERS {
            assert!(html.contains(&format!("grid-area:{l}")));
        }
        assert!(html.contains("A3-2026-0042"));
        assert!(html.contains("Closed, 2026-11-01"));
        assert!(html.contains("Snapshot 3"));
    }

    #[test]
    fn accordion_is_in_vertical_order_and_collapses() {
        let html = render_html(&sample(), &opts(Layout::Accordion));
        let pos: Vec<usize> = ["<h2>A.", "<h2>B.", "<h2>C.", "<h2>D."].iter().map(|n| html.find(n).unwrap()).collect();
        assert!(pos.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(html.matches("<details open>").count(), 4);
    }

    #[test]
    fn html_is_self_contained() {
        for layout in [Layout::Quadrant, Layout::Accordion] {
            let html = render_html(&sample(), &opts(layout));
            assert!(!html.contains("src=\"http"), "no remote resources");
            assert!(!html.contains("<link "), "no external stylesheets");
        }
    }

    #[test]
    fn user_text_cannot_inject_markup() {
        let mut d = sample();
        d.sheet.title = "<script>alert(1)</script>".into();
        d.sheet.owner_name = "<img src=x onerror=alert(1)>".into();
        let html = render_html(&d, &opts(Layout::Accordion));
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains("<img src=x"));
    }

    #[test]
    fn attachment_list_and_source_notes_are_optional() {
        let mut o = opts(Layout::Accordion);
        assert!(render_html(&sample(), &o).contains("report.pdf"));
        o.include_attachments = false;
        assert!(!render_html(&sample(), &o).contains("report.pdf"));
        o.include_source_notes = true;
        assert!(render_html(&sample(), &o).contains("Source notes"));
    }
}
