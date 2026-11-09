use super::sheets::ROW_SELECT;
use super::{DashboardRepo, PgRepository, WallRepo};
use crate::error::Result;
use crate::model::*;
use async_trait::async_trait;
use sqlx::{Postgres, QueryBuilder};

#[async_trait]
impl DashboardRepo for PgRepository {
    async fn my_work(&self) -> Result<MyWork> {
        let me = self.me.id;
        let unread = self.list_wall(&WallFilter { unread: true, ..Default::default() }).await?;
        let unread_total = unread.iter().filter(|w| w.withdrawn_at.is_none()).count() as i64;
        let unread_warnings = unread.into_iter().filter(|w| w.withdrawn_at.is_none()).take(5).collect();

        let mut q: QueryBuilder<Postgres> = QueryBuilder::new(ROW_SELECT);
        q.push(" WHERE v.status = 'Open' AND NOT v.withdrawn AND (sh.owner_id = ").push_bind(me);
        q.push(" OR ").push_bind(me).push(" = ANY(sh.team_member_ids)) ORDER BY sh.target_close_date NULLS LAST, sh.updated_at DESC");
        let my_sheets: Vec<SheetRow> = q.build_query_as().fetch_all(&self.pool).await?;

        let my_tasks = sqlx::query_as!(
            Task,
            r#"SELECT t.id, t.warning_id, w.headline AS warning_headline, t.description, t.assignee_id, u.display_name AS assignee_name,
                      t.target_area, t.due_on, t.result, t.closed_at, t.linked_a3_id, sh.number AS "linked_a3_number?"
               FROM follow_up_task t
               JOIN early_warning w ON w.id = t.warning_id
               JOIN app_user u ON u.id = t.assignee_id
               LEFT JOIN a3_sheet sh ON sh.id = t.linked_a3_id
               WHERE t.assignee_id = $1 AND t.closed_at IS NULL
               ORDER BY t.due_on NULLS LAST, t.created_at"#,
            me
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(MyWork { unread_warnings, unread_total, my_sheets, my_tasks })
    }

    async fn management(&self, f: &MgmtFilter) -> Result<Management> {
        let area = f.area.as_deref();
        let by_step = sqlx::query_as!(
            LabelCount,
            r#"SELECT l.letter AS "label!", count(v.sheet_id) AS "count!"
               FROM (VALUES ('A'), ('B'), ('C'), ('D')) AS l(letter)
               LEFT JOIN (
                 SELECT v.sheet_id, v.current_step FROM v_a3_status v JOIN a3_sheet sh ON sh.id = v.sheet_id
                 WHERE v.status = 'Open' AND NOT v.withdrawn
                   AND ($1::uuid IS NULL OR sh.project_id = $1) AND ($2::text IS NULL OR sh.area = $2)
               ) v ON v.current_step = l.letter
               GROUP BY l.letter ORDER BY l.letter"#,
            f.project_id, area
        )
        .fetch_all(&self.pool)
        .await?;

        let by_age = sqlx::query_as!(
            LabelCount,
            r#"SELECT b.band AS "label!", count(x.age) AS "count!"
               FROM (VALUES (1, '0-30', 0, 30), (2, '31-60', 31, 60), (3, '61-90', 61, 90), (4, 'over 90', 91, 2147483647)) AS b(ord, band, lo, hi)
               LEFT JOIN (
                 SELECT CURRENT_DATE - sh.registered_on AS age
                 FROM v_a3_status v JOIN a3_sheet sh ON sh.id = v.sheet_id
                 WHERE v.status = 'Open' AND NOT v.withdrawn
                   AND ($1::uuid IS NULL OR sh.project_id = $1) AND ($2::text IS NULL OR sh.area = $2)
               ) x ON x.age BETWEEN b.lo AND b.hi
               GROUP BY b.ord, b.band ORDER BY b.ord"#,
            f.project_id, area
        )
        .fetch_all(&self.pool)
        .await?;

        // Overdue and Stalled lists, oldest first.
        let list = |flag: &'static str| {
            let mut q: QueryBuilder<Postgres> = QueryBuilder::new(ROW_SELECT);
            q.push(" WHERE ").push(sqlx::AssertSqlSafe(flag));
            if let Some(p) = f.project_id {
                q.push(" AND sh.project_id = ").push_bind(p);
            }
            if let Some(a) = f.area.clone() {
                q.push(" AND sh.area = ").push_bind(a);
            }
            q.push(" ORDER BY sh.registered_on LIMIT 50");
            q
        };
        let mut oq = list("v.overdue");
        let overdue: Vec<SheetRow> = oq.build_query_as().fetch_all(&self.pool).await?;
        let mut sq = list("v.stalled");
        let stalled: Vec<SheetRow> = sq.build_query_as().fetch_all(&self.pool).await?;

        let closed = sqlx::query!(
            r#"SELECT count(*) AS "n!", avg(v.closed_at::date - sh.registered_on)::float8 AS avg_days
               FROM v_a3_status v JOIN a3_sheet sh ON sh.id = v.sheet_id
               WHERE v.status = 'Closed' AND NOT v.withdrawn
                 AND ($1::uuid IS NULL OR sh.project_id = $1) AND ($2::text IS NULL OR sh.area = $2)
                 AND ($3::date IS NULL OR v.closed_at::date >= $3) AND ($4::date IS NULL OR v.closed_at::date <= $4)"#,
            f.project_id, area, f.from, f.to
        )
        .fetch_one(&self.pool)
        .await?;

        let reopened_by_reason = sqlx::query_as!(
            LabelCount,
            r#"SELECT COALESCE(e.reason, 'Other') AS "label!", count(*) AS "count!"
               FROM audit_event e JOIN a3_sheet sh ON sh.id = e.sheet_id
               WHERE e.action = 'step_reopened' AND sh.withdrawn_at IS NULL
                 AND ($1::uuid IS NULL OR sh.project_id = $1) AND ($2::text IS NULL OR sh.area = $2)
                 AND ($3::date IS NULL OR e.at::date >= $3) AND ($4::date IS NULL OR e.at::date <= $4)
               GROUP BY 1 ORDER BY 2 DESC"#,
            f.project_id, area, f.from, f.to
        )
        .fetch_all(&self.pool)
        .await?;
        let reopened_count = reopened_by_reason.iter().map(|r| r.count).sum();

        let reach = sqlx::query!(
            r#"SELECT w.id, w.headline,
                      (SELECT count(*) FROM warning_read_state r WHERE r.warning_id = w.id AND NOT r.unread AND r.read_at IS NOT NULL) AS "read_n!",
                      (SELECT count(*) FROM app_user WHERE active) AS "users_n!",
                      (SELECT count(*) FROM follow_up_task t WHERE t.warning_id = w.id AND t.closed_at IS NULL) AS "open_n!",
                      (SELECT count(*) FROM follow_up_task t WHERE t.warning_id = w.id AND t.closed_at IS NOT NULL) AS "done_n!",
                      (SELECT count(*) FROM follow_up_task t WHERE t.warning_id = w.id AND t.result = 'Affected') AS "affected_n!"
               FROM early_warning w JOIN a3_sheet sh ON sh.id = w.sheet_id
               WHERE w.withdrawn_at IS NULL AND w.last_update_at >= now() - interval '90 days'
                 AND ($1::uuid IS NULL OR sh.project_id = $1) AND ($2::text IS NULL OR sh.area = $2)
               ORDER BY w.posted_at DESC LIMIT 50"#,
            f.project_id, area
        )
        .fetch_all(&self.pool)
        .await?;
        let warning_reach = reach
            .into_iter()
            .map(|r| WarningReach {
                id: r.id,
                headline: r.headline,
                read_percent: if r.users_n > 0 { (r.read_n * 100 + r.users_n / 2) / r.users_n } else { 0 },
                checks_open: r.open_n,
                checks_done: r.done_n,
                affected: r.affected_n,
            })
            .collect();

        Ok(Management {
            by_step,
            by_age,
            overdue,
            stalled,
            closed_count: closed.n,
            average_days_to_close: closed.avg_days,
            reopened_count,
            reopened_by_reason,
            warning_reach,
        })
    }
}
