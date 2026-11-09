use super::users::require_row;
use super::{PgRepository, SheetRepo, UserRepo, WallRepo};
use crate::domain::{cause_status, word_count};
use crate::error::{CoreError, Result};
use crate::model::*;
use async_trait::async_trait;
use serde_json::json;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

/// The card as a given reader sees it. `$me` is bound first in every query built from this.
const WARNING_COLUMNS: &str = "w.id, w.sheet_id, sh.number AS sheet_number, sh.title AS sheet_title, w.headline, w.what_happened, w.who_should_check,
    w.quick_check, w.severity,
    COALESCE((SELECT eu.cause_status FROM early_warning_update eu WHERE eu.warning_id = w.id ORDER BY eu.created_at DESC LIMIT 1), w.cause_status) AS cause_status,
    w.author_id, u.display_name AS author_name, w.posted_at, w.last_update_at, w.withdrawn_at, w.withdrawn_reason,
    w.last_update_at < now() - interval '90 days' AS archived,
    (rs.warning_id IS NULL OR rs.unread) AS unread,
    COALESCE(rs.updated, false) AS updated,
    (SELECT count(*) FROM follow_up_task t WHERE t.warning_id = w.id) AS checks_total,
    (SELECT count(*) FROM follow_up_task t WHERE t.warning_id = w.id AND t.closed_at IS NULL) AS checks_open,
    (SELECT count(*) FROM follow_up_task t WHERE t.warning_id = w.id AND t.result = 'Not affected') AS not_affected,
    (SELECT count(*) FROM follow_up_task t WHERE t.warning_id = w.id AND t.result = 'Affected') AS affected,
    (SELECT count(*) FROM follow_up_task t WHERE t.warning_id = w.id AND t.result = 'Not applicable') AS not_applicable,
    (SELECT count(*) FROM warning_read_state r WHERE r.warning_id = w.id AND NOT r.unread AND r.read_at IS NOT NULL) AS read_count";

fn base<'a>(me: Uuid) -> QueryBuilder<'a, Postgres> {
    let mut q: QueryBuilder<Postgres> = QueryBuilder::new(format!("SELECT {WARNING_COLUMNS} FROM early_warning w
        JOIN a3_sheet sh ON sh.id = w.sheet_id
        JOIN app_user u ON u.id = w.author_id
        LEFT JOIN warning_read_state rs ON rs.warning_id = w.id AND rs.user_id = ").as_str().to_owned());
    q.push_bind(me);
    q
}

#[async_trait]
impl WallRepo for PgRepository {
    async fn list_wall(&self, f: &WallFilter) -> Result<Vec<Warning>> {
        let mut q = base(self.me.id);
        q.push(" WHERE true");
        // FR-EW-05: cards move to Archive 90 days after the last update. Search still finds them.
        let searching = f.search.as_deref().is_some_and(|s| !s.trim().is_empty());
        if f.archive {
            q.push(" AND w.last_update_at < now() - interval '90 days'");
        } else if !searching {
            q.push(" AND w.last_update_at >= now() - interval '90 days'");
        }
        if f.unread {
            q.push(" AND (rs.warning_id IS NULL OR rs.unread)");
        }
        if let Some(s) = &f.severity {
            q.push(" AND w.severity = ").push_bind(s.clone());
        }
        if let Some(a) = &f.area {
            q.push(" AND ").push_bind(a.clone()).push(" = ANY(w.who_should_check)");
        }
        if f.my_tasks {
            q.push(" AND EXISTS (SELECT 1 FROM follow_up_task t WHERE t.warning_id = w.id AND t.assignee_id = ").push_bind(self.me.id).push(")");
        }
        if let Some(s) = f.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let like = format!("%{}%", s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            q.push(" AND (w.headline ILIKE ").push_bind(like.clone());
            q.push(" OR w.what_happened ILIKE ").push_bind(like.clone());
            q.push(" OR sh.number ILIKE ").push_bind(like).push(")");
        }
        q.push(" ORDER BY w.posted_at DESC LIMIT ").push_bind(f.limit.unwrap_or(200));
        Ok(q.build_query_as().fetch_all(&self.pool).await?)
    }

    async fn warning(&self, id: Uuid) -> Result<Warning> {
        let mut q = base(self.me.id);
        q.push(" WHERE w.id = ").push_bind(id);
        q.build_query_as().fetch_optional(&self.pool).await?.ok_or_else(|| CoreError::not_found("Warning"))
    }

    async fn warnings_for_sheet(&self, sheet: Uuid) -> Result<Vec<Warning>> {
        let mut q = base(self.me.id);
        q.push(" WHERE w.sheet_id = ").push_bind(sheet);
        q.push(" ORDER BY w.posted_at DESC");
        Ok(q.build_query_as().fetch_all(&self.pool).await?)
    }

    async fn warning_updates(&self, warning: Uuid) -> Result<Vec<WarningUpdate>> {
        Ok(sqlx::query_as!(
            WarningUpdate,
            "SELECT eu.id, eu.warning_id, eu.text, eu.cause_status, u.display_name AS author_name, eu.created_at
             FROM early_warning_update eu JOIN app_user u ON u.id = eu.author_id
             WHERE eu.warning_id = $1 ORDER BY eu.created_at",
            warning
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn post_warning(&self, n: &NewWarning) -> Result<Uuid> {
        let headline = n.headline.trim();
        if headline.is_empty() || headline.chars().count() > 80 {
            return Err(CoreError::invalid("The headline needs 1 to 80 characters."));
        }
        if n.what_happened.trim().is_empty() || word_count(&n.what_happened) > 40 {
            return Err(CoreError::invalid("What happened needs 1 to 40 words."));
        }
        if n.quick_check.lines().count() > 2 {
            return Err(CoreError::invalid("Quick check can be up to 2 lines."));
        }
        let status = self.current_cause_status(n.sheet_id).await?;
        let id = sqlx::query_scalar!(
            "INSERT INTO early_warning (sheet_id, headline, what_happened, who_should_check, quick_check, severity, cause_status, author_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
            n.sheet_id, headline, n.what_happened.trim(), &n.who_should_check, n.quick_check.trim(), n.severity, status, self.me.id
        )
        .fetch_one(&self.pool)
        .await?;
        self.audit("early_warning", Some(id), Some(n.sheet_id), "warning_posted", None, json!({"headline": headline})).await?;
        Ok(id)
    }

    async fn add_warning_update(&self, warning: Uuid, text: &str) -> Result<()> {
        let w = self.warning(warning).await?;
        let status = self.current_cause_status(w.sheet_id).await?;
        sqlx::query!(
            "INSERT INTO early_warning_update (warning_id, text, cause_status, author_id) VALUES ($1, $2, $3, $4)",
            warning, text.trim(), status, self.me.id
        )
        .execute(&self.pool)
        .await?;
        self.audit("early_warning", Some(warning), Some(w.sheet_id), "warning_updated", None, json!({})).await
    }

    async fn withdraw_warning(&self, warning: Uuid, reason: &str) -> Result<()> {
        if reason.trim().is_empty() {
            return Err(CoreError::invalid("Give a reason for withdrawing this warning."));
        }
        let w = self.warning(warning).await?;
        let r = sqlx::query!(
            "UPDATE early_warning SET withdrawn_at = now(), withdrawn_reason = $2 WHERE id = $1 AND withdrawn_at IS NULL",
            warning, reason.trim()
        )
        .execute(&self.pool)
        .await?;
        require_row(r.rows_affected(), "Only the author can withdraw this warning.")?;
        self.audit("early_warning", Some(warning), Some(w.sheet_id), "warning_withdrawn", Some(reason.trim()), json!({})).await
    }

    async fn set_warning_read(&self, warning: Uuid, read: bool) -> Result<()> {
        sqlx::query!(
            "INSERT INTO warning_read_state (warning_id, user_id, read_at, unread, updated)
             VALUES ($1, $2, CASE WHEN $3 THEN now() END, NOT $3, false)
             ON CONFLICT (warning_id, user_id) DO UPDATE
               SET read_at = CASE WHEN $3 THEN now() ELSE warning_read_state.read_at END,
                   unread = NOT $3,
                   updated = CASE WHEN $3 THEN false ELSE warning_read_state.updated END",
            warning, self.me.id, read
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn create_task(&self, n: &NewTask) -> Result<Uuid> {
        if n.description.trim().is_empty() {
            return Err(CoreError::invalid("Describe the task."));
        }
        Ok(sqlx::query_scalar!(
            "INSERT INTO follow_up_task (warning_id, description, assignee_id, target_area, due_on, created_by)
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
            n.warning_id, n.description.trim(), n.assignee_id.unwrap_or(self.me.id), n.target_area.trim(), n.due_on, self.me.id
        )
        .fetch_one(&self.pool)
        .await?)
    }

    async fn tasks_for_warning(&self, warning: Uuid) -> Result<Vec<Task>> {
        Ok(sqlx::query_as!(
            Task,
            r#"SELECT t.id, t.warning_id, w.headline AS warning_headline, t.description, t.assignee_id, u.display_name AS assignee_name,
                      t.target_area, t.due_on, t.result, t.closed_at, t.linked_a3_id, sh.number AS "linked_a3_number?"
               FROM follow_up_task t
               JOIN early_warning w ON w.id = t.warning_id
               JOIN app_user u ON u.id = t.assignee_id
               LEFT JOIN a3_sheet sh ON sh.id = t.linked_a3_id
               WHERE t.warning_id = $1 ORDER BY t.closed_at IS NOT NULL, t.due_on NULLS LAST, t.created_at"#,
            warning
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn close_task(&self, task: Uuid, result: &str) -> Result<()> {
        let r = sqlx::query!(
            "UPDATE follow_up_task SET result = $2, closed_at = now() WHERE id = $1",
            task, result
        )
        .execute(&self.pool)
        .await?;
        require_row(r.rows_affected(), "Only the assignee or creator can record a result.")
    }

    async fn create_linked_a3(&self, task: Uuid, project: Uuid) -> Result<Uuid> {
        let t = sqlx::query!(
            "SELECT t.warning_id, t.linked_a3_id, t.target_area, w.headline, w.severity, w.what_happened, sh.number, sh.category, sh.product
             FROM follow_up_task t JOIN early_warning w ON w.id = t.warning_id JOIN a3_sheet sh ON sh.id = w.sheet_id
             WHERE t.id = $1",
            task
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| CoreError::not_found("Task"))?;
        if let Some(existing) = t.linked_a3_id {
            return Ok(existing);
        }
        let title: String = t.headline.chars().take(90).collect();
        let id = self
            .create_sheet(&NewSheet {
                title,
                project_id: project,
                severity: Some(t.severity),
                category: t.category,
                source: Some("Internal".into()),
                area: Some(t.target_area).filter(|a| !a.is_empty()),
                product: t.product,
                origin_warning_id: Some(t.warning_id),
                origin_task_id: Some(task),
                ..Default::default()
            })
            .await?;
        // Pre-filled with the warning's approved text, in the problem step (FR-TSK-04).
        let step = sqlx::query!("SELECT id, version FROM a3_step WHERE sheet_id = $1 AND letter = 'A'", id).fetch_one(&self.pool).await?;
        let mut doc = crate::richtext::empty_doc();
        let intro = format!("From early warning on {}: {}", t.number, t.what_happened);
        crate::richtext::append_block(&mut doc, crate::richtext::sourced_paragraph(&intro, None, None, false));
        self.save_step(step.id, step.version, &doc).await?;
        let r = sqlx::query!("UPDATE follow_up_task SET linked_a3_id = $2 WHERE id = $1", task, id).execute(&self.pool).await?;
        require_row(r.rows_affected(), "Only the assignee or creator can link an A3 to this task.")?;
        Ok(id)
    }
}

impl PgRepository {
    async fn current_cause_status(&self, sheet: Uuid) -> Result<String> {
        let detail = self.sheet_detail(sheet).await?;
        let closed = |l: &str| detail.steps.iter().any(|s| s.letter == l && s.state == "closed");
        Ok(cause_status(closed("B"), closed("D")).to_string())
    }
}
