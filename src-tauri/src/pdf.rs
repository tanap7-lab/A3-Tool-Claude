//! PDF export through WebView2 (section 5), from the same HTML template as the HTML export. It uses
//! the DevTools `Page.printToPDF` call rather than `PrintToPdf`, because only this call can produce
//! PDF bookmarks and "Page x of y" footers, which FR-EXP-06 and section 11 require.

use a3_core::export::{ExportData, Layout};
use a3_core::{CoreError, Result};
use base64::Engine;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

const MM_TO_IN: f64 = 1.0 / 25.4;

fn print_params(layout: Layout, footer_left: &str) -> serde_json::Value {
    // Margins match the @page rules in the export template.
    let (w, h, top, bottom, side) = match layout {
        Layout::Quadrant => (420.0, 297.0, 8.0, 14.0, 10.0),
        Layout::Accordion => (210.0, 297.0, 14.0, 16.0, 15.0),
    };
    let footer = format!(
        "<div style=\"font-family:Arial,sans-serif;font-size:8px;color:#5f6368;width:100%;padding:0 {side}mm;display:flex;justify-content:space-between\"><span>{}</span><span>Page <span class=\"pageNumber\"></span> of <span class=\"totalPages\"></span></span></div>",
        html_text(footer_left)
    );
    json!({
        "paperWidth": w * MM_TO_IN,
        "paperHeight": h * MM_TO_IN,
        "marginTop": top * MM_TO_IN,
        "marginBottom": bottom * MM_TO_IN,
        "marginLeft": side * MM_TO_IN,
        "marginRight": side * MM_TO_IN,
        "printBackground": true,
        "displayHeaderFooter": true,
        "headerTemplate": "<span></span>",
        "footerTemplate": footer,
        "generateTaggedPDF": true,
        "generateDocumentOutline": true,
        "transferMode": "ReturnAsBase64",
    })
}

fn html_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Loads the export in a hidden window, lets its fit script run, and prints it.
pub async fn render_pdf(app: &AppHandle, html: &str, layout: Layout, data: &ExportData, dest: &Path) -> Result<()> {
    let dir = crate::state::temp_dir("export")?;
    let file = dir.join(format!("{}.html", uuid::Uuid::new_v4()));
    std::fs::write(&file, html)?;
    let url = url::Url::from_file_path(&file).map_err(|_| CoreError::new("EXPORT", "Could not locate the export page."))?;
    let label = format!("export-{}", uuid::Uuid::new_v4().simple());

    let (loaded_tx, loaded_rx) = oneshot::channel::<()>();
    let loaded_tx = std::sync::Mutex::new(Some(loaded_tx));
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title("A3 Sheet export")
        .visible(false)
        .on_page_load(move |_, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                if let Some(tx) = loaded_tx.lock().expect("load lock").take() {
                    let _ = tx.send(());
                }
            }
        })
        .build()
        .map_err(|e| CoreError::new("EXPORT", format!("Could not start the PDF renderer: {e}")))?;

    let result = async {
        tokio::time::timeout(Duration::from_secs(30), loaded_rx)
            .await
            .map_err(|_| CoreError::new("EXPORT", "The PDF page took too long to load."))?
            .map_err(|_| CoreError::new("EXPORT", "The PDF page did not load."))?;
        let params = print_params(layout, &data.footer_text()).to_string();
        let bytes = call_print(&window, params).await?;
        std::fs::write(dest, bytes)?;
        Ok::<(), CoreError>(())
    }
    .await;

    let _ = window.destroy();
    let _ = std::fs::remove_file(&file);
    result
}

#[cfg(windows)]
async fn call_print(window: &tauri::WebviewWindow, params: String) -> Result<Vec<u8>> {
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows_core::HSTRING;

    let (tx, rx) = oneshot::channel::<std::result::Result<String, String>>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    window
        .with_webview(move |wv| {
            let send = move |r: std::result::Result<String, String>| {
                if let Some(t) = tx.lock().expect("print lock").take() {
                    let _ = t.send(r);
                }
            };
            let send = std::sync::Arc::new(send);
            let run = || -> std::result::Result<(), String> {
                unsafe {
                    let core = wv.controller().CoreWebView2().map_err(|e| e.to_string())?;
                    let method = HSTRING::from("Page.printToPDF");
                    let args = HSTRING::from(params.as_str());
                    let send2 = send.clone();
                    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |code, result| {
                        match code {
                            Ok(()) => send2(Ok(result)),
                            Err(e) => send2(Err(e.to_string())),
                        }
                        Ok(())
                    }));
                    core.CallDevToolsProtocolMethod(&method, &args, &handler).map_err(|e| e.to_string())
                }
            };
            if let Err(e) = run() {
                send(Err(e));
            }
        })
        .map_err(|e| CoreError::new("EXPORT", format!("The PDF renderer is not available: {e}")))?;

    let reply = tokio::time::timeout(Duration::from_secs(60), rx)
        .await
        .map_err(|_| CoreError::new("EXPORT", "Creating the PDF took too long."))?
        .map_err(|_| CoreError::new("EXPORT", "The PDF renderer stopped."))?
        .map_err(|e| CoreError::new("EXPORT", format!("The PDF could not be created: {e}")))?;
    let v: serde_json::Value = serde_json::from_str(&reply)?;
    let data = v["data"].as_str().ok_or_else(|| CoreError::new("EXPORT", "The PDF renderer returned no data."))?;
    base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| CoreError::new("EXPORT", format!("The PDF data was damaged: {e}")))
}

#[cfg(not(windows))]
async fn call_print(_window: &tauri::WebviewWindow, _params: String) -> Result<Vec<u8>> {
    Err(CoreError::new("EXPORT", "PDF export needs Windows."))
}
