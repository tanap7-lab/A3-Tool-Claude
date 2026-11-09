use super::{ExtractedSegment, ImageData};
use crate::error::{CoreError, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use once_cell::sync::Lazy;
use pdfium_render::prelude::*;
use serde_json::json;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// A page with fewer characters than this counts as scanned.
const MIN_PAGE_CHARS: usize = 20;
const MAX_IMAGE_EDGE: u32 = 1800;

fn pdfium_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(d) = std::env::var("A3_PDFIUM_DIR") {
        dirs.push(PathBuf::from(d));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            dirs.push(p.to_path_buf());
            dirs.push(p.join("resources"));
        }
    }
    dirs.push(PathBuf::from("."));
    dirs
}

/// PDFium's bindings can start only once per process, so all callers share one instance.
fn load_pdfium() -> Result<&'static Pdfium> {
    static PDFIUM: Lazy<std::result::Result<Pdfium, String>> = Lazy::new(|| {
        let mut last = String::from("not found");
        for dir in pdfium_dirs() {
            let path = Pdfium::pdfium_platform_library_name_at_path(&dir);
            if !Path::new(&path).exists() {
                continue;
            }
            match Pdfium::bind_to_library(path) {
                Ok(b) => return Ok(Pdfium::new(b)),
                Err(e) => last = e.to_string(),
            }
        }
        Err(last)
    });
    PDFIUM
        .as_ref()
        .map_err(|e| CoreError::new("UNREADABLE", format!("The PDF reader (pdfium.dll) could not start: {e}")))
}

pub fn extract(path: &Path) -> Result<Vec<ExtractedSegment>> {
    let pdfium = load_pdfium()?;
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| CoreError::new("UNREADABLE", format!("This PDF could not be opened: {e}")))?;
    let mut out = Vec::new();
    for (i, page) in doc.pages().iter().enumerate() {
        let n = i + 1;
        let text = page.text().map(|t| t.all()).unwrap_or_default();
        let text = text.replace("\r\n", "\n").replace('\r', "\n").trim().to_string();
        let scanned = text.chars().filter(|c| !c.is_whitespace()).count() < MIN_PAGE_CHARS;
        out.push(ExtractedSegment {
            needs_image: scanned,
            ..ExtractedSegment::new(format!("Page {n}"), json!({"page": n}), text)
        });
    }
    Ok(out)
}

pub fn render_page(path: &Path, page: usize) -> Result<Vec<ImageData>> {
    let pdfium = load_pdfium()?;
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| CoreError::new("UNREADABLE", format!("This PDF could not be opened: {e}")))?;
    let index = page.saturating_sub(1) as i32;
    let p = doc.pages().get(index).map_err(|_| CoreError::new("UNREADABLE", "That page does not exist."))?;
    let bitmap = p
        .render_with_config(&PdfRenderConfig::new().set_target_width(MAX_IMAGE_EDGE as i32))
        .map_err(|e| CoreError::new("UNREADABLE", format!("That page could not be drawn: {e}")))?;
    let img = bitmap.as_image().map_err(|e| CoreError::new("UNREADABLE", format!("That page could not be drawn: {e}")))?;
    Ok(vec![encode_png(&image::DynamicImage::ImageRgb8(img.to_rgb8()))?])
}

pub fn load_image(path: &Path) -> Result<ImageData> {
    let bytes = std::fs::read(path)?;
    image_from_bytes(&bytes)
}

pub(super) fn image_from_bytes(bytes: &[u8]) -> Result<ImageData> {
    let img = image::load_from_memory(bytes).map_err(|e| CoreError::new("UNREADABLE", format!("This image could not be read: {e}")))?;
    let img = if img.width().max(img.height()) > MAX_IMAGE_EDGE {
        img.resize(MAX_IMAGE_EDGE, MAX_IMAGE_EDGE, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    encode_png(&img)
}

fn encode_png(img: &image::DynamicImage) -> Result<ImageData> {
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| CoreError::new("UNREADABLE", format!("This image could not be encoded: {e}")))?;
    Ok(ImageData { mime: "image/png".into(), base64: B64.encode(buf.into_inner()) })
}
