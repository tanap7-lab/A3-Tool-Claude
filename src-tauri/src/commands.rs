//! The interface's window into the app. Commands stay thin: rules live in `a3-core` and the database.

use crate::jobs::spawn_attachment;
use crate::state::*;
use a3_core::ai::{self, A3Summary, AiConfig, AskAnswer, ConnectionTest, ReviewOutcome, Suggestion, SummaryTone, WarningDraft};
use a3_core::connect::{self, ConnectionProfile};
use a3_core::domain::{self, LinkTarget};
use a3_core::email::{self, EmailDraft};
use a3_core::export::{self, ExportOptions};
use a3_core::extract::{self, ImageData};
use a3_core::model::*;
use a3_core::repo::*;
use a3_core::store::{AttachmentStore, PgAttachmentStore, MAX_FILE_BYTES};
use a3_core::{CoreError, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

type S<'a> = State<'a, AppState>;

// ---------------------------------------------------------------- connection and sign-in

#[tauri::command]
pub fn profile_get(app: AppHandle) -> Result<Option<ConnectionProfile>> {
    load_profile(&app)
}

#[tauri::command]
pub fn profile_save(app: AppHandle, profile: ConnectionProfile) -> Result<()> {
    save_profile(&app, &profile)
}

/// Section 15: connection details can come from a small profile file supplied by the admin.
#[tauri::command]
pub fn profile_import(app: AppHandle, path: PathBuf) -> Result<ConnectionProfile> {
    let bytes = std::fs::read(path)?;
    let p: ConnectionProfile =
        serde_json::from_slice(&bytes).map_err(|_| CoreError::invalid("That is not an A3 Sheet connection profile."))?;
    save_profile(&app, &p)?;
    Ok(p)
}

#[tauri::command]
pub fn remembered_login(app: AppHandle) -> Result<Option<String>> {
    Ok(std::fs::read_to_string(config_dir(&app)?.join("last-login")).ok().map(|s| s.trim().to_string()))
}

async fn open_session(app: &AppHandle, state: &AppState, login: &str, password: &str) -> Result<AppUser> {
    let profile = load_profile(app)?.ok_or_else(|| CoreError::new("NO_PROFILE", "Set up the database connection first."))?;
    let pool = connect::sign_in(&profile, login, password).await?;
    let store = Arc::new(PgAttachmentStore::new(pool.clone()));
    let repo = Arc::new(PgRepository::open(pool).await?);
    let me = repo.me().clone();
    repo.audit("app_user", Some(me.id), None, "signed_in", None, json!({})).await?;
    let session = Session { repo, store, login: login.to_string() };
    *state.session.write().expect("session lock") = Some(session.clone());
    tauri::async_runtime::spawn(crate::jobs::resume_after_sign_in(app.clone(), session));
    Ok(me)
}

#[tauri::command]
pub async fn sign_in(app: AppHandle, state: S<'_>, login: String, password: String, remember: bool) -> Result<AppUser> {
    let login = login.trim().to_string();
    let me = open_session(&app, &state, &login, &password).await?;
    std::fs::write(config_dir(&app)?.join("last-login"), &login)?;
    if remember {
        secret_set("db", &login, &password)?;
    } else {
        secret_delete("db", &login)?;
    }
    Ok(me)
}

/// Signs in from Credential Manager when the user chose to be remembered. None means show the form.
#[tauri::command]
pub async fn auto_sign_in(app: AppHandle, state: S<'_>) -> Result<Option<AppUser>> {
    let Some(login) = remembered_login(app.clone())? else { return Ok(None) };
    let Some(password) = secret_get("db", &login)? else { return Ok(None) };
    match open_session(&app, &state, &login, &password).await {
        Ok(me) => Ok(Some(me)),
        Err(e) if e.code == "BAD_LOGIN" => {
            secret_delete("db", &login)?;
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub fn sign_out(state: S<'_>) -> Result<()> {
    if let Ok(s) = state.session() {
        let _ = secret_delete("db", &s.login);
    }
    *state.session.write().expect("session lock") = None;
    Ok(())
}

#[tauri::command]
pub async fn connection_ping(state: S<'_>) -> Result<bool> {
    Ok(state.repo()?.schema_version().await.is_ok())
}

// ---------------------------------------------------------------- account and admin

#[tauri::command]
pub async fn me(state: S<'_>) -> Result<AppUser> {
    let repo = state.repo()?;
    Ok(repo.list_users().await?.into_iter().find(|u| u.id == repo.me().id).unwrap_or_else(|| repo.me().clone()))
}
#[tauri::command]
pub async fn users_list(state: S<'_>) -> Result<Vec<AppUser>> {
    state.repo()?.list_users().await
}
#[tauri::command]
pub async fn account_update(state: S<'_>, display_name: String, email: String, default_project_id: Option<Uuid>) -> Result<AppUser> {
    state.repo()?.update_my_account(&display_name, &email, default_project_id).await
}
#[tauri::command]
pub async fn account_change_password(app: AppHandle, state: S<'_>, old_password: String, new_password: String) -> Result<()> {
    let session = state.session()?;
    // The server cannot check the old password inside a statement, so a fresh sign-in checks it.
    let profile = load_profile(&app)?.ok_or_else(|| CoreError::new("NO_PROFILE", "No connection profile."))?;
    let check = connect::sign_in(&profile, &session.login, &old_password)
        .await
        .map_err(|e| if e.code == "BAD_LOGIN" { CoreError::new("BAD_LOGIN", "Your current password is not right.") } else { e })?;
    check.close().await;
    session.repo.change_my_password(&new_password).await?;
    if secret_get("db", &session.login)?.is_some() {
        secret_set("db", &session.login, &new_password)?;
    }
    Ok(())
}
#[tauri::command]
pub async fn admin_create_user(state: S<'_>, login: String, name: String, email: String, password: String, is_admin: bool) -> Result<Uuid> {
    state.repo()?.admin_create_user(&login, &name, &email, &password, is_admin).await
}
#[tauri::command]
pub async fn admin_set_active(state: S<'_>, user: Uuid, active: bool) -> Result<()> {
    state.repo()?.admin_set_active(user, active).await
}
#[tauri::command]
pub async fn admin_set_admin(state: S<'_>, user: Uuid, is_admin: bool) -> Result<()> {
    state.repo()?.admin_set_admin(user, is_admin).await
}
#[tauri::command]
pub async fn admin_reset_password(state: S<'_>, user: Uuid, password: String) -> Result<()> {
    state.repo()?.admin_reset_password(user, &password).await
}
#[tauri::command]
pub async fn settings_get(state: S<'_>) -> Result<Settings> {
    state.repo()?.get_settings().await
}
#[tauri::command]
pub async fn settings_set(state: S<'_>, settings: Settings) -> Result<()> {
    state.repo()?.set_settings(&settings).await
}

// ---------------------------------------------------------------- projects

#[tauri::command]
pub async fn projects_list(state: S<'_>, only_mine: bool) -> Result<Vec<Project>> {
    state.repo()?.list_projects(only_mine).await
}
#[tauri::command]
pub async fn project_create(state: S<'_>, name: String, code: String, description: String) -> Result<Project> {
    state.repo()?.create_project(&name, &code, &description).await
}
#[tauri::command]
pub async fn project_set_ai(state: S<'_>, project: Uuid, enabled: bool) -> Result<()> {
    state.repo()?.set_project_ai(project, enabled).await
}
#[tauri::command]
pub async fn project_set_archived(state: S<'_>, project: Uuid, archived: bool) -> Result<()> {
    state.repo()?.set_project_archived(project, archived).await
}
#[tauri::command]
pub async fn project_members(state: S<'_>, project: Uuid) -> Result<Vec<AppUser>> {
    state.repo()?.project_members(project).await
}
#[tauri::command]
pub async fn project_add_member(state: S<'_>, project: Uuid, user: Uuid) -> Result<()> {
    state.repo()?.add_project_member(project, user).await
}
#[tauri::command]
pub async fn project_remove_member(state: S<'_>, project: Uuid, user: Uuid) -> Result<()> {
    state.repo()?.remove_project_member(project, user).await
}
#[tauri::command]
pub async fn pick_values(state: S<'_>, project: Option<Uuid>) -> Result<Vec<PickValue>> {
    state.repo()?.pick_values(project).await
}
#[tauri::command]
pub async fn pick_add(state: S<'_>, list_name: String, value: String, project: Option<Uuid>) -> Result<()> {
    state.repo()?.add_pick_value(&list_name, &value, project).await
}
#[tauri::command]
pub async fn pick_remove(state: S<'_>, id: Uuid) -> Result<()> {
    state.repo()?.remove_pick_value(id).await
}

// ---------------------------------------------------------------- A3 sheets and steps

#[tauri::command]
pub async fn sheets_list(state: S<'_>, filter: SheetFilter) -> Result<SheetPage> {
    state.repo()?.list_sheets(&filter).await
}
#[tauri::command]
pub async fn sheet_detail(state: S<'_>, id: Uuid) -> Result<SheetDetail> {
    state.repo()?.sheet_detail(id).await
}
#[tauri::command]
pub async fn sheet_create(state: S<'_>, new: NewSheet) -> Result<Uuid> {
    state.repo()?.create_sheet(&new).await
}
#[tauri::command]
pub async fn sheet_update(state: S<'_>, id: Uuid, patch: SheetPatch) -> Result<()> {
    state.repo()?.update_sheet(id, &patch).await
}
#[tauri::command]
pub async fn sheet_withdraw(state: S<'_>, id: Uuid, reason: String) -> Result<()> {
    state.repo()?.withdraw_sheet(id, &reason).await
}
#[tauri::command]
pub async fn step_save(state: S<'_>, step: Uuid, expected_version: i32, content: Value) -> Result<Step> {
    state.repo()?.save_step(step, expected_version, &content).await
}
#[tauri::command]
pub async fn step_lock(state: S<'_>, step: Uuid) -> Result<Step> {
    state.repo()?.acquire_lock(step).await
}
#[tauri::command]
pub async fn step_unlock(state: S<'_>, step: Uuid) -> Result<()> {
    state.repo()?.release_lock(step).await
}
#[tauri::command]
pub async fn step_close(state: S<'_>, step: Uuid) -> Result<()> {
    state.repo()?.close_step(step).await
}
#[tauri::command]
pub async fn step_reopen(state: S<'_>, step: Uuid, reason: String) -> Result<()> {
    state.repo()?.reopen_step(step, &reason).await
}
#[tauri::command]
pub async fn sheet_snapshots(state: S<'_>, id: Uuid) -> Result<Vec<Snapshot>> {
    state.repo()?.snapshots(id).await
}
#[tauri::command]
pub async fn snapshot_restore(state: S<'_>, snapshot: Uuid) -> Result<()> {
    state.repo()?.restore_snapshot(snapshot).await
}
#[tauri::command]
pub async fn sheet_history(state: S<'_>, id: Uuid) -> Result<Vec<AuditRow>> {
    state.repo()?.history(id).await
}

// ---------------------------------------------------------------- unsaved text recovery (section 15)

fn recovery_path(app: &AppHandle, step: Uuid) -> Result<PathBuf> {
    Ok(recovery_dir(app)?.join(format!("{step}.json")))
}
#[tauri::command]
pub fn recovery_save(app: AppHandle, step: Uuid, content: Value, version: i32) -> Result<()> {
    // Write then rename, so a crash mid-write cannot destroy the text this file exists to protect.
    let path = recovery_path(&app, step)?;
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, serde_json::to_vec(&json!({"content": content, "version": version}))?)?;
    std::fs::rename(&temp, &path)?;
    Ok(())
}
#[tauri::command]
pub fn recovery_load(app: AppHandle, step: Uuid) -> Result<Option<Value>> {
    let p = recovery_path(&app, step)?;
    Ok(std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()))
}
#[tauri::command]
pub fn recovery_clear(app: AppHandle, step: Uuid) -> Result<()> {
    let _ = std::fs::remove_file(recovery_path(&app, step)?);
    Ok(())
}

// ---------------------------------------------------------------- attachments

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddResult {
    pub added: Vec<Attachment>,
    pub duplicates: Vec<String>,
    pub errors: Vec<String>,
}

async fn register_attachment(app: &AppHandle, session: &Session, sheet: Uuid, name: &str, hash: &str) -> Result<AddOutcome> {
    let kind = extract::classify(name);
    let out = session.repo.add_attachment(sheet, name, hash, &extract::file_type_label(name)).await?;
    if !out.duplicate {
        let initial = extract::initial_read_state(kind);
        if initial != "pending" {
            session.repo.set_read_state(out.attachment.id, initial, None).await?;
        } else {
            if provider_for(app, &session.login).unwrap_or(None).is_some() {
                session.repo.set_ai_state(out.attachment.id, "pending", None, None).await?;
            }
            spawn_attachment(app, session.clone(), out.attachment.id);
        }
    }
    Ok(out)
}

/// FR-ATT-01: up to 50 MB per file, 20 files at once. Files that fail report one by one.
#[tauri::command]
pub async fn attachments_add(app: AppHandle, state: S<'_>, sheet: Uuid, paths: Vec<PathBuf>) -> Result<AddResult> {
    let session = state.session()?;
    if paths.len() > 20 {
        return Err(CoreError::invalid("You can add up to 20 files at once."));
    }
    let mut result = AddResult { added: vec![], duplicates: vec![], errors: vec![] };
    for path in paths {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
        let outcome = async {
            let size = std::fs::metadata(&path)?.len();
            if size > MAX_FILE_BYTES {
                return Err(CoreError::new("TOO_LARGE", "over the 50 MB limit"));
            }
            let blob = session.store.put_file(&path).await?;
            register_attachment(&app, &session, sheet, &name, &blob.hash).await
        }
        .await;
        match outcome {
            Ok(o) if o.duplicate => result.duplicates.push(name),
            Ok(o) => result.added.push(o.attachment),
            Err(e) => result.errors.push(format!("{name}: {}", e.message)),
        }
    }
    Ok(result)
}

/// A file pasted from the clipboard.
#[tauri::command]
pub async fn attachment_add_bytes(app: AppHandle, state: S<'_>, sheet: Uuid, name: String, base64: String) -> Result<AddOutcome> {
    let session = state.session()?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(base64).map_err(|_| CoreError::invalid("That data could not be read."))?;
    let blob = session.store.put_bytes(&bytes).await?;
    register_attachment(&app, &session, sheet, &name, &blob.hash).await
}

#[tauri::command]
pub async fn attachments_list(state: S<'_>, sheet: Uuid) -> Result<Vec<Attachment>> {
    state.repo()?.list_attachments(sheet).await
}
#[tauri::command]
pub async fn attachment_delete(state: S<'_>, id: Uuid) -> Result<()> {
    state.repo()?.delete_attachment(id).await
}
#[tauri::command]
pub async fn attachment_restore(state: S<'_>, id: Uuid) -> Result<()> {
    state.repo()?.restore_attachment(id).await
}
#[tauri::command]
pub async fn attachment_set_sheets(state: S<'_>, id: Uuid, excluded: Vec<String>) -> Result<()> {
    state.repo()?.set_excluded_sheets(id, &excluded).await
}
/// Reads the file again, or asks AI again after a failure.
#[tauri::command]
pub async fn attachment_retry(app: AppHandle, state: S<'_>, id: Uuid) -> Result<()> {
    let s = state.session()?;
    let a = s.repo.attachment(id).await?;
    if a.read_state == "unreadable" {
        s.repo.set_read_state(id, "pending", None).await?;
    }
    if provider_for(&app, &s.login)?.is_some() {
        s.repo.set_ai_state(id, "pending", None, None).await?;
    }
    spawn_attachment(&app, s, id);
    Ok(())
}
#[tauri::command]
pub async fn segments_list(state: S<'_>, attachment: Uuid) -> Result<Vec<Segment>> {
    state.repo()?.segments(attachment).await
}
#[tauri::command]
pub async fn source_view(state: S<'_>, segment: Uuid) -> Result<SourceView> {
    state.repo()?.source_view(segment).await
}
#[tauri::command]
pub async fn step_add_passage(state: S<'_>, step: Uuid, expected_version: i32, segment: Uuid, text: String) -> Result<Step> {
    state.repo()?.add_passage_to_step(step, expected_version, segment, &text).await
}

async fn fetch_to_temp(session: &Session, a: &Attachment, sub: &str) -> Result<PathBuf> {
    let dir = temp_dir(sub)?.join(a.id.to_string());
    std::fs::create_dir_all(&dir)?;
    // Only the last path part of a stored name, so it cannot point outside this folder.
    let safe = Path::new(&a.file_name).file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let path = dir.join(safe);
    session.store.get_to_file(&a.blob_hash, &path).await?;
    Ok(path)
}

/// A page an Ask answer cites from the web, opened in the user's browser. Only web addresses are opened,
/// so a link can never start a program or open a local file.
#[tauri::command]
pub fn open_web_link(app: AppHandle, url: String) -> Result<()> {
    let ok = url.len() <= 2048 && (url.starts_with("https://") || url.starts_with("http://"));
    if !ok {
        return Err(CoreError::new("OPEN", "That is not a web address."));
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| CoreError::new("OPEN", format!("Could not open the web page: {e}")))
}

/// FR-ATT-02: opened in its default app.
#[tauri::command]
pub async fn attachment_open(app: AppHandle, state: S<'_>, id: Uuid) -> Result<()> {
    let s = state.session()?;
    let a = s.repo.attachment(id).await?;
    let path = fetch_to_temp(&s, &a, "open").await?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| CoreError::new("OPEN", format!("Could not open this file: {e}")))
}

/// FR-ATT-02: saved anywhere.
#[tauri::command]
pub async fn attachment_save(state: S<'_>, id: Uuid, dest: PathBuf) -> Result<()> {
    let s = state.session()?;
    let a = s.repo.attachment(id).await?;
    s.store.get_to_file(&a.blob_hash, &dest).await
}

/// The picture behind a segment, for the source pane: a PDF page, a slide's image, or an image file.
#[tauri::command]
pub async fn segment_preview(state: S<'_>, segment: Uuid) -> Result<Vec<ImageData>> {
    let s = state.session()?;
    let seg = s.repo.segment(segment).await?;
    let a = s.repo.attachment(seg.attachment_id).await?;
    let path = fetch_to_temp(&s, &a, "work").await?;
    let exe = std::env::current_exe()?;
    let images = extract::images_isolated(&exe, &path, &a.file_name, &seg.location).await;
    let _ = std::fs::remove_file(&path);
    images
}

// ---------------------------------------------------------------- proposals and findings

#[tauri::command]
pub async fn proposals_list(state: S<'_>, sheet: Uuid, pending_only: bool) -> Result<Vec<Proposal>> {
    state.repo()?.proposals(sheet, pending_only).await
}
#[tauri::command]
pub async fn proposal_decide(state: S<'_>, id: Uuid, decision: ProposalDecision) -> Result<()> {
    state.repo()?.decide_proposal(id, &decision).await
}
#[tauri::command]
pub async fn findings_list(state: S<'_>, sheet: Uuid) -> Result<Vec<Finding>> {
    state.repo()?.findings(sheet, true).await
}
#[tauri::command]
pub async fn finding_dismiss(state: S<'_>, id: Uuid) -> Result<()> {
    state.repo()?.dismiss_finding(id).await
}
#[tauri::command]
pub async fn messages_list(state: S<'_>, sheet: Uuid) -> Result<Vec<AiMessage>> {
    Ok(state.repo()?.messages(sheet).await?.into_iter().filter(|m| m.role != "summary").collect())
}

// ---------------------------------------------------------------- AI jobs and settings

#[tauri::command]
pub async fn ai_review(app: AppHandle, state: S<'_>, sheet: Uuid) -> Result<ReviewOutcome> {
    ai_service(&app, &state.session()?)?.review(sheet).await
}
#[tauri::command]
pub async fn ai_improve(app: AppHandle, state: S<'_>, sheet: Uuid, letter: String, selection: Option<String>) -> Result<Vec<Suggestion>> {
    ai_service(&app, &state.session()?)?.improve(sheet, &letter, selection.as_deref()).await
}
#[tauri::command]
pub async fn ai_summarize(app: AppHandle, state: S<'_>, sheet: Uuid, tone: Option<SummaryTone>) -> Result<A3Summary> {
    ai_service(&app, &state.session()?)?.summarize_with_tone(sheet, tone.unwrap_or_default()).await
}
#[tauri::command]
pub async fn ai_apply_summary(state: S<'_>, sheet: Uuid, draft: A3Summary) -> Result<Vec<Step>> {
    state.repo()?.apply_summary(sheet, &draft.steps).await
}
#[tauri::command]
pub async fn ai_condense(app: AppHandle, state: S<'_>, sheet: Uuid, letter: String, target_chars: usize) -> Result<Vec<Suggestion>> {
    ai_service(&app, &state.session()?)?.condense(sheet, &letter, target_chars).await
}
#[tauri::command]
pub async fn ai_draft_warning(app: AppHandle, state: S<'_>, sheet: Uuid) -> Result<WarningDraft> {
    ai_service(&app, &state.session()?)?.draft_warning(sheet).await
}
#[tauri::command]
pub async fn ai_ask(app: AppHandle, state: S<'_>, sheet: Uuid, question: String, more: bool, options: Option<ai::AskOptions>) -> Result<AskAnswer> {
    ai_service(&app, &state.session()?)?.ask_with(sheet, &question, more, &options.unwrap_or_default()).await
}
#[tauri::command]
pub async fn ai_suggest_tasks(app: AppHandle, state: S<'_>, warning: Uuid) -> Result<Vec<String>> {
    ai_service(&app, &state.session()?)?.suggest_tasks(warning).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsView {
    pub config: Option<AiConfig>,
    pub has_key: bool,
    pub ready: bool,
}

#[tauri::command]
pub fn ai_settings_get(app: AppHandle, state: S<'_>) -> Result<AiSettingsView> {
    let login = state.session()?.login;
    Ok(AiSettingsView {
        config: load_ai_config(&app, &login)?,
        has_key: secret_get("ai", &login)?.is_some(),
        ready: provider_for(&app, &login)?.is_some(),
    })
}

/// FR-AI-02: the key goes only to Windows Credential Manager. Leaving it empty keeps the stored one.
#[tauri::command]
pub fn ai_settings_save(app: AppHandle, state: S<'_>, config: AiConfig, key: Option<String>) -> Result<AiSettingsView> {
    let login = state.session()?.login;
    save_ai_config(&app, &login, &config)?;
    if let Some(k) = key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        secret_set("ai", &login, k)?;
    }
    ai_settings_get(app, state)
}

#[tauri::command]
pub fn ai_key_clear(app: AppHandle, state: S<'_>) -> Result<AiSettingsView> {
    secret_delete("ai", &state.session()?.login)?;
    ai_settings_get(app, state)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDraftSettings {
    pub config: AiConfig,
    pub key: Option<String>,
}

fn draft_provider(login: &str, draft: &AiDraftSettings) -> Result<Box<dyn ai::AiProvider>> {
    let key = match draft.key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => k.to_string(),
        None => secret_get("ai", login)?.unwrap_or_default(),
    };
    ai::make_provider(&draft.config, &key)
}

/// FR-AI-03: the list comes live from the provider, so new models appear without an app update.
#[tauri::command]
pub async fn ai_list_models(state: S<'_>, draft: AiDraftSettings) -> Result<Vec<String>> {
    let login = state.session()?.login;
    let mut models = draft_provider(&login, &draft)?.list_models().await?;
    models.sort();
    Ok(models)
}

/// FR-AI-04: key works, structured output works, image reading works. The result is remembered.
#[tauri::command]
pub async fn ai_test(app: AppHandle, state: S<'_>, draft: AiDraftSettings) -> Result<ConnectionTest> {
    let login = state.session()?.login;
    let provider = draft_provider(&login, &draft)?;
    let result = ai::test_connection(provider.as_ref()).await;
    if let Some(mut saved) = load_ai_config(&app, &login)? {
        if saved.provider == draft.config.provider && saved.model == draft.config.model {
            saved.vision_ok = result.image_reading_works;
            save_ai_config(&app, &login, &saved)?;
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------- wall

#[tauri::command]
pub async fn wall_list(state: S<'_>, filter: WallFilter) -> Result<Vec<Warning>> {
    state.repo()?.list_wall(&filter).await
}
#[tauri::command]
pub async fn warning_get(state: S<'_>, id: Uuid) -> Result<Warning> {
    state.repo()?.warning(id).await
}
#[tauri::command]
pub async fn warnings_for_sheet(state: S<'_>, sheet: Uuid) -> Result<Vec<Warning>> {
    state.repo()?.warnings_for_sheet(sheet).await
}
#[tauri::command]
pub async fn warning_updates(state: S<'_>, warning: Uuid) -> Result<Vec<WarningUpdate>> {
    state.repo()?.warning_updates(warning).await
}
#[tauri::command]
pub async fn warning_post(state: S<'_>, new: NewWarning) -> Result<Uuid> {
    state.repo()?.post_warning(&new).await
}
#[tauri::command]
pub async fn warning_add_update(state: S<'_>, warning: Uuid, text: String) -> Result<()> {
    state.repo()?.add_warning_update(warning, &text).await
}
#[tauri::command]
pub async fn warning_withdraw(state: S<'_>, warning: Uuid, reason: String) -> Result<()> {
    state.repo()?.withdraw_warning(warning, &reason).await
}
#[tauri::command]
pub async fn warning_set_read(state: S<'_>, warning: Uuid, read: bool) -> Result<()> {
    state.repo()?.set_warning_read(warning, read).await
}
#[tauri::command]
pub async fn task_create(state: S<'_>, new: NewTask) -> Result<Uuid> {
    state.repo()?.create_task(&new).await
}
#[tauri::command]
pub async fn tasks_for_warning(state: S<'_>, warning: Uuid) -> Result<Vec<Task>> {
    state.repo()?.tasks_for_warning(warning).await
}
#[tauri::command]
pub async fn task_close(state: S<'_>, task: Uuid, result: String) -> Result<()> {
    state.repo()?.close_task(task, &result).await
}
#[tauri::command]
pub async fn task_create_linked_a3(state: S<'_>, task: Uuid, project: Uuid) -> Result<Uuid> {
    state.repo()?.create_linked_a3(task, project).await
}

async fn email_for(repo: &PgRepository, warning: Uuid) -> Result<EmailDraft> {
    let w = repo.warning(warning).await?;
    let base = repo.get_settings().await?.link_base;
    let wl = domain::make_link(&base, "warning", &w.id.to_string());
    let al = domain::make_link(&base, "a3", &w.sheet_number);
    Ok(email::build_draft(&w, &wl, &al))
}

/// FR-EML-03: subject and formatted body for the clipboard, which the interface writes.
#[tauri::command]
pub async fn warning_email_draft(state: S<'_>, warning: Uuid) -> Result<EmailDraft> {
    email_for(state.repo()?.as_ref(), warning).await
}

/// FR-EML-02: the app never sends. It opens an unsent draft in the mail app.
#[tauri::command]
pub async fn warning_email_open(app: AppHandle, state: S<'_>, warning: Uuid) -> Result<()> {
    let draft = email_for(state.repo()?.as_ref(), warning).await?;
    let dir = temp_dir("email")?;
    let path = dir.join(format!("early-warning-{warning}.eml"));
    std::fs::write(&path, email::to_eml(&draft))?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| CoreError::new("OPEN", format!("Could not open the mail app: {e}")))
}

// ---------------------------------------------------------------- dashboard

#[tauri::command]
pub async fn my_work(state: S<'_>) -> Result<MyWork> {
    state.repo()?.my_work().await
}
#[tauri::command]
pub async fn management(state: S<'_>, filter: MgmtFilter) -> Result<Management> {
    state.repo()?.management(&filter).await
}

// ---------------------------------------------------------------- export

#[tauri::command]
pub async fn export_default_name(state: S<'_>, sheet: Uuid, layout: String, format: String) -> Result<String> {
    let d = state.repo()?.sheet_detail(sheet).await?;
    Ok(domain::export_file_name(&d.sheet.number, &d.sheet.title, &layout, &format))
}

/// Writes the export page to a temp file the preview frame can load. Returns its path.
#[tauri::command]
pub async fn export_preview(state: S<'_>, sheet: Uuid, options: ExportOptions) -> Result<PathBuf> {
    let repo = state.repo()?;
    let data = export::gather(repo.as_ref(), sheet).await?;
    let html = export::render_html(&data, &options);
    let dir = temp_dir("preview")?;
    // One fresh file per preview, and old ones cleared, so stale A3 text does not pile up.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let path = dir.join(format!("{}.html", Uuid::new_v4()));
    std::fs::write(&path, html)?;
    Ok(path)
}

#[tauri::command]
pub async fn export_save(app: AppHandle, state: S<'_>, sheet: Uuid, options: ExportOptions, format: String, dest: PathBuf) -> Result<()> {
    let repo = state.repo()?;
    let data = export::gather(repo.as_ref(), sheet).await?;
    match format.as_str() {
        "html" => std::fs::write(&dest, export::render_html(&data, &options))?,
        "docx" => std::fs::write(&dest, export::render_docx(&data, &options)?)?,
        "pdf" => crate::pdf::render_pdf(&app, &export::render_html(&data, &options), options.layout, &data, &dest).await?,
        other => return Err(CoreError::invalid(format!("Unknown export format: {other}"))),
    }
    repo.audit("a3_sheet", Some(sheet), Some(sheet), "a3_exported", None, json!({"format": format, "layout": options.layout})).await
}

#[tauri::command]
pub fn path_open(app: AppHandle, path: PathBuf) -> Result<()> {
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| CoreError::new("OPEN", format!("Could not open this file: {e}")))
}

// ---------------------------------------------------------------- links

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ResolvedLink {
    A3 { id: Uuid },
    Warning { id: Uuid },
}

/// FR-LNK-04: anything that fails the strict pattern resolves to nothing and is ignored.
#[tauri::command]
pub async fn link_resolve(state: S<'_>, text: String) -> Result<Option<ResolvedLink>> {
    let repo = state.repo()?;
    let base = repo.get_settings().await?.link_base;
    Ok(match domain::parse_link(&text, &base) {
        Some(LinkTarget::A3(number)) => repo.sheet_id_by_number(&number).await?.map(|id| ResolvedLink::A3 { id }),
        Some(LinkTarget::Warning(id)) => Uuid::parse_str(&id).ok().map(|id| ResolvedLink::Warning { id }),
        None => None,
    })
}

#[tauri::command]
pub async fn link_make(state: S<'_>, kind: String, id: String) -> Result<String> {
    let base = state.repo()?.get_settings().await?.link_base;
    Ok(domain::make_link(&base, &kind, &id))
}

#[tauri::command]
pub fn link_take_pending(state: S<'_>) -> Option<String> {
    state.pending_link.lock().expect("link lock").take()
}
