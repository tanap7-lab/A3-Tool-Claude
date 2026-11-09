use super::{PgRepository, UserRepo};
use crate::error::{CoreError, Result};
use crate::model::*;
use crate::scram;
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

#[async_trait]
impl UserRepo for PgRepository {
    fn me(&self) -> &AppUser {
        &self.me
    }

    async fn schema_version(&self) -> Result<i32> {
        Ok(sqlx::query_scalar!("SELECT max(version) FROM schema_meta").fetch_one(&self.pool).await?.unwrap_or(0))
    }

    async fn audit(&self, object_type: &str, object_id: Option<Uuid>, sheet_id: Option<Uuid>, action: &str, reason: Option<&str>, detail: Value) -> Result<()> {
        sqlx::query!(
            "INSERT INTO audit_event (actor_id, object_type, object_id, sheet_id, action, reason, detail)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            self.me.id, object_type, object_id, sheet_id, action, reason, detail
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_users(&self) -> Result<Vec<AppUser>> {
        Ok(sqlx::query_as!(
            AppUser,
            "SELECT id, role_name, display_name, email, is_admin, active, default_project_id, last_project_id
             FROM app_user ORDER BY display_name"
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn update_my_account(&self, display_name: &str, email: &str, default_project_id: Option<Uuid>) -> Result<AppUser> {
        if display_name.trim().is_empty() {
            return Err(CoreError::invalid("Display name cannot be empty."));
        }
        Ok(sqlx::query_as!(
            AppUser,
            "UPDATE app_user SET display_name = $2, email = $3, default_project_id = $4 WHERE id = $1
             RETURNING id, role_name, display_name, email, is_admin, active, default_project_id, last_project_id",
            self.me.id, display_name.trim(), email.trim(), default_project_id
        )
        .fetch_one(&self.pool)
        .await?)
    }

    async fn set_last_project(&self, project_id: Uuid) -> Result<()> {
        sqlx::query!("UPDATE app_user SET last_project_id = $2 WHERE id = $1", self.me.id, project_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn change_my_password(&self, new_password: &str) -> Result<()> {
        check_password(new_password)?;
        let v = scram::verifier(new_password);
        sqlx::query!("SELECT change_my_password($1)", v).execute(&self.pool).await?;
        Ok(())
    }

    async fn admin_create_user(&self, login: &str, name: &str, email: &str, password: &str, is_admin: bool) -> Result<Uuid> {
        check_password(password)?;
        if name.trim().is_empty() {
            return Err(CoreError::invalid("Enter a display name."));
        }
        let v = scram::verifier(password);
        let id = sqlx::query_scalar!("SELECT admin_create_user($1, $2, $3, $4, $5)", login.trim(), name.trim(), email.trim(), v, is_admin)
            .fetch_one(&self.pool)
            .await?;
        id.ok_or_else(|| CoreError::new("DB", "The user was not created."))
    }

    async fn admin_set_active(&self, user: Uuid, active: bool) -> Result<()> {
        sqlx::query!("SELECT admin_set_user_active($1, $2)", user, active).execute(&self.pool).await?;
        Ok(())
    }

    async fn admin_set_admin(&self, user: Uuid, is_admin: bool) -> Result<()> {
        sqlx::query!("SELECT admin_set_user_admin($1, $2)", user, is_admin).execute(&self.pool).await?;
        Ok(())
    }

    async fn admin_reset_password(&self, user: Uuid, password: &str) -> Result<()> {
        check_password(password)?;
        let v = scram::verifier(password);
        sqlx::query!("SELECT admin_reset_password($1, $2)", user, v).execute(&self.pool).await?;
        Ok(())
    }

    async fn get_settings(&self) -> Result<Settings> {
        let rows = sqlx::query!("SELECT key, value FROM app_setting").fetch_all(&self.pool).await?;
        let get = |k: &str| rows.iter().find(|r| r.key == k).map(|r| r.value.clone()).unwrap_or_default();
        Ok(Settings { stalled_days: get("stalled_days").parse().unwrap_or(14), link_base: get("link_base") })
    }

    async fn set_settings(&self, s: &Settings) -> Result<()> {
        if s.stalled_days < 1 {
            return Err(CoreError::invalid("The stalled threshold must be at least 1 day."));
        }
        let base = s.link_base.trim();
        if !base.is_empty() && !base.starts_with("https://") {
            return Err(CoreError::invalid("The link base address must start with https://."));
        }
        let mut tx = self.pool.begin().await?;
        let days = s.stalled_days.to_string();
        let a = sqlx::query!("UPDATE app_setting SET value = $1 WHERE key = 'stalled_days'", days).execute(&mut *tx).await?;
        sqlx::query!("UPDATE app_setting SET value = $1 WHERE key = 'link_base'", base).execute(&mut *tx).await?;
        if a.rows_affected() == 0 {
            return Err(CoreError::new("NO_PERMISSION", "Only an admin can change company settings."));
        }
        tx.commit().await?;
        Ok(())
    }

    async fn list_projects(&self, only_mine: bool) -> Result<Vec<Project>> {
        Ok(sqlx::query_as!(
            Project,
            r#"SELECT p.id, p.name, p.code, p.description, p.owner_id, u.display_name AS owner_name, p.ai_enabled, p.archived_at,
                      EXISTS (SELECT 1 FROM project_member m WHERE m.project_id = p.id AND m.user_id = $1) AS "is_member!",
                      p.owner_id = $1 AS "is_owner!",
                      (SELECT count(*) FROM project_member m WHERE m.project_id = p.id) AS "member_count!"
               FROM project p JOIN app_user u ON u.id = p.owner_id
               WHERE NOT $2 OR EXISTS (SELECT 1 FROM project_member m WHERE m.project_id = p.id AND m.user_id = $1)
               ORDER BY p.archived_at IS NOT NULL, p.name"#,
            self.me.id, only_mine
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn create_project(&self, name: &str, code: &str, description: &str) -> Result<Project> {
        let (name, code) = (name.trim(), code.trim().to_uppercase());
        if name.is_empty() || code.is_empty() {
            return Err(CoreError::invalid("A project needs a name and a short code."));
        }
        let id = sqlx::query_scalar!(
            "INSERT INTO project (name, code, description, owner_id) VALUES ($1, $2, $3, $4) RETURNING id",
            name, code, description.trim(), self.me.id
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            let c: CoreError = e.into();
            if c.code == "DUPLICATE" { CoreError::new("DUPLICATE", "That project code is already in use.") } else { c }
        })?;
        self.list_projects(true)
            .await?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| CoreError::not_found("Project"))
    }

    async fn set_project_ai(&self, project: Uuid, enabled: bool) -> Result<()> {
        let r = sqlx::query!("UPDATE project SET ai_enabled = $2 WHERE id = $1", project, enabled).execute(&self.pool).await?;
        require_row(r.rows_affected(), "Only the project owner can change this.")?;
        self.audit("project", Some(project), None, "project_ai_changed", None, serde_json::json!({"enabled": enabled})).await
    }

    async fn set_project_archived(&self, project: Uuid, archived: bool) -> Result<()> {
        let r = sqlx::query!(
            "UPDATE project SET archived_at = CASE WHEN $2 THEN now() END WHERE id = $1",
            project, archived
        )
        .execute(&self.pool)
        .await?;
        require_row(r.rows_affected(), "Only the project owner can archive it.")
    }

    async fn project_members(&self, project: Uuid) -> Result<Vec<AppUser>> {
        Ok(sqlx::query_as!(
            AppUser,
            "SELECT u.id, u.role_name, u.display_name, u.email, u.is_admin, u.active, u.default_project_id, u.last_project_id
             FROM app_user u JOIN project_member m ON m.user_id = u.id
             WHERE m.project_id = $1 ORDER BY u.display_name",
            project
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn add_project_member(&self, project: Uuid, user: Uuid) -> Result<()> {
        sqlx::query!(
            "INSERT INTO project_member (project_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            project, user
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn remove_project_member(&self, project: Uuid, user: Uuid) -> Result<()> {
        let owner = sqlx::query_scalar!("SELECT owner_id FROM project WHERE id = $1", project).fetch_one(&self.pool).await?;
        if owner == user {
            return Err(CoreError::invalid("The project owner cannot be removed."));
        }
        let r = sqlx::query!("DELETE FROM project_member WHERE project_id = $1 AND user_id = $2", project, user)
            .execute(&self.pool)
            .await?;
        require_row(r.rows_affected(), "Only the project owner can remove members.")
    }

    async fn pick_values(&self, project: Option<Uuid>) -> Result<Vec<PickValue>> {
        Ok(sqlx::query_as!(
            PickValue,
            "SELECT id, list_name, value, project_id FROM pick_list_value
             WHERE project_id IS NULL OR project_id = $1
             ORDER BY list_name, sort_order, value",
            project
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn add_pick_value(&self, list_name: &str, value: &str, project: Option<Uuid>) -> Result<()> {
        let value = value.trim();
        if value.is_empty() {
            return Err(CoreError::invalid("Enter a value."));
        }
        sqlx::query!(
            "INSERT INTO pick_list_value (list_name, value, project_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            list_name, value, project
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn remove_pick_value(&self, id: Uuid) -> Result<()> {
        let r = sqlx::query!("DELETE FROM pick_list_value WHERE id = $1", id).execute(&self.pool).await?;
        require_row(r.rows_affected(), "You can't remove this value.")
    }
}

fn check_password(p: &str) -> Result<()> {
    if p.chars().count() < 8 {
        return Err(CoreError::invalid("Passwords need at least 8 characters."));
    }
    Ok(())
}

/// A write that row-level security filtered out reports zero rows rather than an error.
pub(super) fn require_row(affected: u64, message: &str) -> Result<()> {
    if affected == 0 {
        Err(CoreError::new("NO_PERMISSION", message))
    } else {
        Ok(())
    }
}
