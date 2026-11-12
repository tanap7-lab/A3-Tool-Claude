mod commands;
mod jobs;
mod pdf;
mod state;

use state::AppState;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

/// A link has arrived, from a second launch or from the first one. Hold it until sign-in if needed.
fn handle_link(app: &AppHandle, url: String) {
    if !(url.starts_with("a3sheet://") || url.starts_with("https://")) {
        return;
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    let state = app.state::<AppState>();
    let signed_in = state.session().is_ok();
    if signed_in {
        let _ = app.emit("open-link", url);
    } else {
        *state.pending_link.lock().expect("link lock") = Some(url);
    }
}

fn init_logging(app: &AppHandle) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let dir = app.path().app_log_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    let files = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .max_log_files(14)
        .filename_prefix("a3-sheet")
        .filename_suffix("log")
        .build(dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(files);
    let filter = tracing_subscriber::EnvFilter::new("warn,a3_core=info,a3_sheet_lib=info,sqlx=error");
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false).with_writer(writer).try_init();
    Some(guard)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // On Windows a link starts a new process with the URL as an argument, so single-instance routes
        // it to the open window (section 5). This plugin has to register first.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            match args.iter().skip(1).find(|a| a.contains("://")) {
                Some(url) => handle_link(app, url.clone()),
                None => {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.unminimize();
                        let _ = w.set_focus();
                    }
                }
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .setup(|app| {
            if let Some(guard) = init_logging(app.handle()) {
                app.manage(guard);
            }
            // A3 text copied out to open or preview a file does not stay in the temp folder between runs.
            let _ = std::fs::remove_dir_all(std::env::temp_dir().join("A3Sheet"));
            // The installer writes the scheme to HKCU. In a dev build there is no installer, so register here.
            #[cfg(debug_assertions)]
            {
                let _ = app.deep_link().register("a3sheet");
            }
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                if let Some(u) = urls.first() {
                    handle_link(app.handle(), u.to_string());
                }
            }
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                if let Some(u) = event.urls().first() {
                    handle_link(&handle, u.to_string());
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::profile_get,
            commands::profile_save,
            commands::profile_import,
            commands::remembered_login,
            commands::sign_in,
            commands::auto_sign_in,
            commands::sign_out,
            commands::connection_ping,
            commands::me,
            commands::users_list,
            commands::account_update,
            commands::account_change_password,
            commands::admin_create_user,
            commands::admin_set_active,
            commands::admin_set_admin,
            commands::admin_reset_password,
            commands::settings_get,
            commands::settings_set,
            commands::projects_list,
            commands::project_create,
            commands::project_set_ai,
            commands::project_set_archived,
            commands::project_members,
            commands::project_add_member,
            commands::project_remove_member,
            commands::pick_values,
            commands::pick_add,
            commands::pick_remove,
            commands::sheets_list,
            commands::sheet_detail,
            commands::sheet_create,
            commands::sheet_update,
            commands::sheet_withdraw,
            commands::step_save,
            commands::step_lock,
            commands::step_unlock,
            commands::step_close,
            commands::step_reopen,
            commands::sheet_snapshots,
            commands::snapshot_restore,
            commands::sheet_history,
            commands::recovery_save,
            commands::recovery_load,
            commands::recovery_clear,
            commands::attachments_add,
            commands::attachment_add_bytes,
            commands::attachments_list,
            commands::attachment_delete,
            commands::attachment_restore,
            commands::attachment_set_sheets,
            commands::attachment_retry,
            commands::segments_list,
            commands::source_view,
            commands::step_add_passage,
            commands::attachment_open,
            commands::attachment_save,
            commands::segment_preview,
            commands::proposals_list,
            commands::proposal_decide,
            commands::findings_list,
            commands::finding_dismiss,
            commands::messages_list,
            commands::ai_review,
            commands::ai_improve,
            commands::ai_summarize,
            commands::ai_apply_summary,
            commands::ai_condense,
            commands::ai_draft_warning,
            commands::ai_ask,
            commands::open_web_link,
            commands::ai_suggest_tasks,
            commands::ai_settings_get,
            commands::ai_settings_save,
            commands::ai_key_clear,
            commands::ai_list_models,
            commands::ai_test,
            commands::wall_list,
            commands::warning_get,
            commands::warnings_for_sheet,
            commands::warning_updates,
            commands::warning_post,
            commands::warning_add_update,
            commands::warning_withdraw,
            commands::warning_set_read,
            commands::task_create,
            commands::tasks_for_warning,
            commands::task_close,
            commands::task_create_linked_a3,
            commands::warning_email_draft,
            commands::warning_email_open,
            commands::my_work,
            commands::management,
            commands::export_default_name,
            commands::export_preview,
            commands::export_save,
            commands::path_open,
            commands::link_resolve,
            commands::link_make,
            commands::link_take_pending,
        ])
        .run(tauri::generate_context!())
        .expect("the app failed to start");
}
