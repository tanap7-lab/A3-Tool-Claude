use super::users::require_row;
use super::{PgRepository, SheetRepo, UserRepo};
use crate::domain::make_link;
use crate::error::{CoreError, Result};
use crate::model::*;
use crate::richtext;
use async_trait::async_trait;
use serde_json::{json, Value};
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

pub(super) const PAGE_SIZE: i64 = 50;

pub(super) const ROW_SELECT: &str = "SELECT sh.id, sh.number, sh.title, sh.project_id, p.code AS project_code, sh.owner_id, u.display_name AS owner_name,
    v.step_strip, v.status, v.current_step, v.overdue, v.stalled, v.reopened, v.withdrawn,
    sh.severity, sh.category, sh.area, sh.registered_on, sh.target_close_date, v.closed_at, sh.updated_at,
    (SELECT count(*) FROM ai_proposal pr WHERE pr.sheet_id = sh.id AND pr.state = 'pending') AS pending_review
    FROM a3_sheet sh
    JOIN v_a3_status v ON v.sheet_id = sh.id
    JOIN project p ON p.id = sh.project_id
    JOIN app_user u ON u.id = sh.owner_id";

fn push_filters(q: &mut QueryBuilder<Postgres>, f: &SheetFilter, me: Uuid) {
    q.push(" WHERE true");
    match f.quick.as_deref() {
        Some("open") => { q.push(" AND v.status = 'Open' AND NOT v.withdrawn"); }
        Some("overdue") => { q.push(" AND v.overdue"); }
        Some("stalled") => { q.push(" AND v.stalled"); }
        Some("closed") => { q.push(" AND v.status = 'Closed' AND NOT v.withdrawn"); }
        Some("withdrawn") => { q.push(" AND v.withdrawn"); }
        // A withdrawn A3 leaves all counts but stays retrievable through its own filter.
        _ => { q.push(" AND NOT v.withdrawn"); }
    }
    if f.mine {
        q.push(" AND (sh.owner_id = ").push_bind(me);
        q.push(" OR ").push_bind(me).push(" = ANY(sh.team_member_ids))");
    }
    if let Some(v) = f.project_id { q.push(" AND sh.project_id = ").push_bind(v); }
    if let Some(v) = f.owner_id { q.push(" AND sh.owner_id = ").push_bind(v); }
    if let Some(v) = &f.category { q.push(" AND sh.category = ").push_bind(v.clone()); }
    if let Some(v) = &f.area { q.push(" AND sh.area = ").push_bind(v.clone()); }
    if let Some(v) = f.from { q.push(" AND sh.registered_on >= ").push_bind(v); }
    if let Some(v) = f.to { q.push(" AND sh.registered_on <= ").push_bind(v); }
    if let Some(v) = &f.current_step { q.push(" AND v.current_step = ").push_bind(v.clone()); }
    if let Some(v) = f.closed_from { q.push(" AND v.closed_at >= ").push_bind(v); }
    match f.age_band.as_deref() {
        Some("0-30") => { q.push(" AND CURRENT_DATE - sh.registered_on BETWEEN 0 AND 30"); }
        Some("31-60") => { q.push(" AND CURRENT_DATE - sh.registered_on BETWEEN 31 AND 60"); }
        Some("61-90") => { q.push(" AND CURRENT_DATE - sh.registered_on BETWEEN 61 AND 90"); }
        Some("over 90") => { q.push(" AND CURRENT_DATE - sh.registered_on > 90"); }
        _ => {}
    }
    if f.reopened_from.is_some() || f.reopen_reason.is_some() {
        q.push(" AND EXISTS (SELECT 1 FROM audit_event ae WHERE ae.sheet_id = sh.id AND ae.action = 'step_reopened'");
        if let Some(v) = f.reopened_from { q.push(" AND ae.at >= ").push_bind(v); }
        if let Some(v) = &f.reopen_reason { q.push(" AND ae.reason = ").push_bind(v.clone()); }
        q.push(")");
    }
    if let Some(s) = f.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        // FR-LST-03: title, step text and attachment text.
        let like = format!("%{}%", s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        q.push(" AND (sh.title ILIKE ").push_bind(like.clone());
        q.push(" OR sh.number ILIKE ").push_bind(like);
        q.push(" OR sh.search @@ websearch_to_tsquery('english', ").push_bind(s.to_string()).push(")");
        q.push(" OR EXISTS (SELECT 1 FROM a3_step st WHERE st.sheet_id = sh.id AND st.search @@ websearch_to_tsquery('english', ").push_bind(s.to_string()).push("))");
        q.push(" OR EXISTS (SELECT 1 FROM source_segment seg JOIN attachment a ON a.id = seg.attachment_id WHERE a.sheet_id = sh.id AND a.deleted_at IS NULL AND seg.search @@ websearch_to_tsquery('english', ").push_bind(s.to_string()).push(")))");
    }
}

/// FR-LST-04: sorting on any column. Only these names ever reach the SQL text.
fn sort_column(name: Option<&str>) -> &'static str {
    match name {
        Some("number") => "sh.number",
        Some("title") => "sh.title",
        Some("project") => "p.code",
        Some("owner") => "u.display_name",
        Some("step") => "v.step_strip",
        Some("status") => "v.status",
        Some("health") => "(v.overdue::int + v.stalled::int + v.reopened::int)",
        Some("severity") => "CASE sh.severity WHEN 'High' THEN 1 WHEN 'Medium' THEN 2 WHEN 'Low' THEN 3 ELSE 4 END",
        Some("target") => "sh.target_close_date",
        _ => "sh.updated_at",
    }
}

#[async_trait]
impl SheetRepo for PgRepository {
    async fn list_sheets(&self, f: &SheetFilter) -> Result<SheetPage> {
        let page = f.page.unwrap_or(0).max(0);
        let mut count: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT count(*) FROM a3_sheet sh JOIN v_a3_status v ON v.sheet_id = sh.id
             JOIN project p ON p.id = sh.project_id JOIN app_user u ON u.id = sh.owner_id",
        );
        push_filters(&mut count, f, self.me.id);
        let total: i64 = count.build_query_scalar().fetch_one(&self.pool).await?;

        let mut q: QueryBuilder<Postgres> = QueryBuilder::new(ROW_SELECT);
        push_filters(&mut q, f, self.me.id);
        q.push(" ORDER BY ");
        q.push(sort_column(f.sort.as_deref()));
        q.push(if f.descending.unwrap_or(true) { " DESC NULLS LAST" } else { " ASC NULLS LAST" });
        q.push(", sh.number DESC LIMIT ").push_bind(PAGE_SIZE);
        q.push(" OFFSET ").push_bind(page * PAGE_SIZE);
        let rows: Vec<SheetRow> = q.build_query_as().fetch_all(&self.pool).await?;
        Ok(SheetPage { rows, total, page, page_size: PAGE_SIZE })
    }

    async fn sheet_detail(&self, id: Uuid) -> Result<SheetDetail> {
        let sheet = sqlx::query_as!(
            Sheet,
            r#"SELECT sh.id, sh.number, sh.title, sh.project_id, p.name AS project_name, p.code AS project_code,
                      p.ai_enabled AS project_ai_enabled, sh.owner_id, u.display_name AS owner_name, sh.team_member_ids,
                      sh.registered_on, sh.problem_date, sh.target_close_date, sh.category, sh.severity, sh.source, sh.area, sh.product,
                      sh.origin_warning_id, sh.origin_task_id, sh.withdrawn_at, sh.withdrawn_reason, sh.updated_at,
                      v.status AS "status!", v.current_step, v.closed_at, v.step_strip AS "step_strip!",
                      v.overdue AS "overdue!", v.stalled AS "stalled!", v.reopened AS "reopened!",
                      (SELECT count(*) FROM ai_proposal pr WHERE pr.sheet_id = sh.id AND pr.state = 'pending') AS "pending_review!",
                      EXISTS (SELECT 1 FROM project_member m WHERE m.project_id = sh.project_id AND m.user_id = $2) AS "can_edit!",
                      sh.owner_id = $2 AS "is_owner!"
               FROM a3_sheet sh
               JOIN v_a3_status v ON v.sheet_id = sh.id
               JOIN project p ON p.id = sh.project_id
               JOIN app_user u ON u.id = sh.owner_id
               WHERE sh.id = $1"#,
            id, self.me.id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| CoreError::not_found("A3"))?;
        let steps = sqlx::query_as!(
            Step,
            r#"SELECT st.id AS "id!", st.sheet_id AS "sheet_id!", st.letter AS "letter!", st.content AS "content!", st.plain_text AS "plain_text!", st.state AS "state!", st.closed_at, st.version AS "version!", st.reopen_count AS "reopen_count!",
                      CASE WHEN st.locked_at > now() - interval '2 minutes' THEN st.locked_by END AS locked_by,
                      CASE WHEN st.locked_at > now() - interval '2 minutes' THEN u.display_name END AS locked_by_name,
                      st.locked_at, st.updated_at AS "updated_at!"
               FROM a3_step st LEFT JOIN app_user u ON u.id = st.locked_by
               WHERE st.sheet_id = $1 ORDER BY st.letter"#,
            id
        )
        .fetch_all(&self.pool)
        .await?;
        let open_conflicts = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM ai_proposal WHERE sheet_id = $1 AND state = 'pending' AND kind = 'conflict'"#,
            id
        )
        .fetch_one(&self.pool)
        .await?;
        let link_base = self.get_settings().await?.link_base;
        let link = make_link(&link_base, "a3", &sheet.number);
        Ok(SheetDetail { sheet, steps, link, open_conflicts })
    }

    async fn sheet_id_by_number(&self, number: &str) -> Result<Option<Uuid>> {
        Ok(sqlx::query_scalar!("SELECT id FROM a3_sheet WHERE number = $1", number).fetch_optional(&self.pool).await?)
    }

    async fn create_sheet(&self, n: &NewSheet) -> Result<Uuid> {
        check_title(&n.title)?;
        let id = sqlx::query_scalar!(
            "INSERT INTO a3_sheet (title, project_id, owner_id, problem_date, target_close_date, category, severity, source, area, product, origin_warning_id, origin_task_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) RETURNING id",
            n.title.trim(), n.project_id, self.me.id, n.problem_date, n.target_close_date,
            n.category, n.severity, n.source, n.area, n.product, n.origin_warning_id, n.origin_task_id
        )
        .fetch_one(&self.pool)
        .await?;
        self.set_last_project(n.project_id).await?;
        self.audit("a3_sheet", Some(id), Some(id), "a3_created", None, json!({})).await?;
        Ok(id)
    }

    async fn update_sheet(&self, id: Uuid, p: &SheetPatch) -> Result<()> {
        check_title(&p.title)?;
        let r = sqlx::query!(
            "UPDATE a3_sheet SET title = $2, project_id = $3, owner_id = $4, team_member_ids = $5, problem_date = $6, target_close_date = $7,
                category = $8, severity = $9, source = $10, area = $11, product = $12
             WHERE id = $1",
            id, p.title.trim(), p.project_id, p.owner_id, &p.team_member_ids, p.problem_date, p.target_close_date,
            p.category, p.severity, p.source, p.area, p.product
        )
        .execute(&self.pool)
        .await?;
        require_row(r.rows_affected(), "Only project members can change this A3.")
    }

    async fn withdraw_sheet(&self, id: Uuid, reason: &str) -> Result<()> {
        if reason.trim().is_empty() {
            return Err(CoreError::invalid("Give a reason for withdrawing this A3."));
        }
        let r = sqlx::query!(
            "UPDATE a3_sheet SET withdrawn_at = now(), withdrawn_by = $2, withdrawn_reason = $3 WHERE id = $1 AND withdrawn_at IS NULL",
            id, self.me.id, reason.trim()
        )
        .execute(&self.pool)
        .await?;
        require_row(r.rows_affected(), "Only the A3 owner can withdraw it.")?;
        self.audit("a3_sheet", Some(id), Some(id), "a3_withdrawn", Some(reason.trim()), json!({})).await
    }

    async fn save_step(&self, step: Uuid, expected_version: i32, content: &Value) -> Result<Step> {
        let plain = richtext::plain_text(content);
        let mut tx = self.pool.begin().await?;
        // FR-A3-08: a stale save cannot overwrite newer text.
        let r = sqlx::query!(
            "UPDATE a3_step SET content = $2, plain_text = $3, locked_at = CASE WHEN locked_by = $5 THEN now() ELSE locked_at END
             WHERE id = $1 AND version = $4",
            step, content, plain, expected_version, self.me.id
        )
        .execute(&mut *tx)
        .await?;
        if r.rows_affected() == 0 {
            let exists = sqlx::query_scalar!("SELECT version FROM a3_step WHERE id = $1", step).fetch_optional(&mut *tx).await?;
            return Err(match exists {
                Some(_) if !self.can_edit_step(step).await? => CoreError::new("NO_PERMISSION", "Only project members can edit this A3."),
                Some(_) => CoreError::conflict("Someone else saved this step first. Your text is kept so you can compare."),
                None => CoreError::not_found("Step"),
            });
        }
        // FR-A3-09: at most one autosave snapshot every 10 minutes.
        sqlx::query!(
            "INSERT INTO a3_step_snapshot (step_id, sheet_id, letter, content, plain_text, version, author_id, reason)
             SELECT st.id, st.sheet_id, st.letter, st.content, st.plain_text, st.version, $2, 'autosave'
             FROM a3_step st
             WHERE st.id = $1 AND NOT EXISTS (
               SELECT 1 FROM a3_step_snapshot sn WHERE sn.step_id = st.id AND sn.reason = 'autosave'
                 AND sn.created_at > now() - interval '10 minutes')",
            step, self.me.id
        )
        .execute(&mut *tx)
        .await?;
        let saved = fetch_step(&mut *tx, step).await?;
        tx.commit().await?;
        Ok(saved)
    }

    async fn acquire_lock(&self, step: Uuid) -> Result<Step> {
        sqlx::query!("SELECT acquire_step_lock($1)", step).fetch_one(&self.pool).await?;
        fetch_step(&self.pool, step).await
    }

    async fn release_lock(&self, step: Uuid) -> Result<()> {
        sqlx::query!("SELECT release_step_lock($1)", step).execute(&self.pool).await?;
        Ok(())
    }

    async fn close_step(&self, step: Uuid) -> Result<()> {
        sqlx::query!("SELECT close_step($1)", step).execute(&self.pool).await?;
        Ok(())
    }

    async fn reopen_step(&self, step: Uuid, reason: &str) -> Result<()> {
        sqlx::query!("SELECT reopen_step($1, $2)", step, reason).execute(&self.pool).await?;
        Ok(())
    }

    async fn snapshots(&self, sheet: Uuid) -> Result<Vec<Snapshot>> {
        Ok(sqlx::query_as!(
            Snapshot,
            r#"SELECT sn.id AS "id!", sn.step_id AS "step_id!", sn.letter AS "letter!", sn.content AS "content!", sn.plain_text AS "plain_text!", sn.version AS "version!", u.display_name AS "author_name?", sn.reason AS "reason!", sn.created_at AS "created_at!"
               FROM a3_step_snapshot sn LEFT JOIN app_user u ON u.id = sn.author_id
               WHERE sn.sheet_id = $1 ORDER BY sn.created_at DESC LIMIT 200"#,
            sheet
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn restore_snapshot(&self, snapshot: Uuid) -> Result<()> {
        sqlx::query!("SELECT restore_step_snapshot($1)", snapshot).execute(&self.pool).await?;
        Ok(())
    }

    async fn history(&self, sheet: Uuid) -> Result<Vec<AuditRow>> {
        Ok(sqlx::query_as!(
            AuditRow,
            r#"SELECT e.id AS "id!", u.display_name AS "actor_name?", e.object_type AS "object_type!", e.action AS "action!", e.reason, e.detail AS "detail!", e.at AS "at!"
               FROM audit_event e LEFT JOIN app_user u ON u.id = e.actor_id
               WHERE e.sheet_id = $1 ORDER BY e.at DESC, e.id DESC LIMIT 500"#,
            sheet
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn snapshot_count(&self, sheet: Uuid) -> Result<i64> {
        Ok(sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM a3_step_snapshot WHERE sheet_id = $1"#, sheet)
            .fetch_one(&self.pool)
            .await?)
    }
}

impl PgRepository {
    async fn can_edit_step(&self, step: Uuid) -> Result<bool> {
        Ok(sqlx::query_scalar!(
            r#"SELECT EXISTS (
                 SELECT 1 FROM a3_step st JOIN a3_sheet sh ON sh.id = st.sheet_id
                 JOIN project_member m ON m.project_id = sh.project_id
                 WHERE st.id = $1 AND m.user_id = $2) AS "ok!""#,
            step, self.me.id
        )
        .fetch_one(&self.pool)
        .await?)
    }
}

pub(super) async fn fetch_step<'c, E>(exec: E, step: Uuid) -> Result<Step>
where
    E: sqlx::Executor<'c, Database = Postgres>,
{
    Ok(sqlx::query_as!(
        Step,
        r#"SELECT st.id AS "id!", st.sheet_id AS "sheet_id!", st.letter AS "letter!", st.content AS "content!", st.plain_text AS "plain_text!", st.state AS "state!", st.closed_at, st.version AS "version!", st.reopen_count AS "reopen_count!",
                  CASE WHEN st.locked_at > now() - interval '2 minutes' THEN st.locked_by END AS locked_by,
                  CASE WHEN st.locked_at > now() - interval '2 minutes' THEN u.display_name END AS locked_by_name,
                  st.locked_at, st.updated_at AS "updated_at!"
           FROM a3_step st LEFT JOIN app_user u ON u.id = st.locked_by
           WHERE st.id = $1"#,
        step
    )
    .fetch_one(exec)
    .await?)
}

fn check_title(title: &str) -> Result<()> {
    if title.chars().count() > 90 {
        return Err(CoreError::invalid("Titles can be up to 90 characters."));
    }
    Ok(())
}
