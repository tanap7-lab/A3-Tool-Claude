use super::users::require_row;
use super::{AttachmentRepo, PgRepository, UserRepo};
use crate::error::{CoreError, Result};
use crate::extract::ExtractedSegment;
use crate::model::*;
use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

const ATTACHMENT_SELECT: &str = r#"SELECT a.id, a.sheet_id, a.file_name, a.version, a.blob_hash, b.size_bytes, a.file_type, a.read_state, a.read_error,
    a.ai_state, a.ai_error, a.ai_note, a.summary, a.excluded_sheets, a.confirms_count, a.added_by, u.display_name AS added_by_name, a.added_at, a.deleted_at,
    COALESCE((SELECT array_agg(DISTINCT f.letter ORDER BY f.letter) FROM a3_fact f JOIN source_segment s ON s.id = f.source_segment_id
              WHERE s.attachment_id = a.id AND f.state = 'accepted'), '{}') AS informs_steps,
    COALESCE((SELECT array_agg(DISTINCT s.group_name) FROM source_segment s WHERE s.attachment_id = a.id AND s.group_name IS NOT NULL), '{}') AS sheet_names,
    (SELECT count(*) FROM source_segment s WHERE s.attachment_id = a.id AND s.needs_image) AS pending_images
    FROM attachment a JOIN attachment_blob b ON b.hash = a.blob_hash JOIN app_user u ON u.id = a.added_by"#;

#[async_trait]
impl AttachmentRepo for PgRepository {
    async fn add_attachment(&self, sheet: Uuid, file_name: &str, blob_hash: &str, file_type: &str) -> Result<AddOutcome> {
        // FR-ATT-04: identical content is detected and not stored twice.
        let existing = sqlx::query_scalar!(
            "SELECT id FROM attachment WHERE sheet_id = $1 AND blob_hash = $2 AND deleted_at IS NULL ORDER BY added_at DESC LIMIT 1",
            sheet, blob_hash
        )
        .fetch_optional(&self.pool)
        .await?;
        if let Some(id) = existing {
            return Ok(AddOutcome { attachment: self.attachment(id).await?, duplicate: true });
        }
        let mut tx = self.pool.begin().await?;
        // Serialises version numbering for this A3 and file name.
        sqlx::query!("SELECT pg_advisory_xact_lock(hashtext($1 || $2))", sheet.to_string(), file_name).execute(&mut *tx).await?;
        let version = sqlx::query_scalar!(
            r#"SELECT COALESCE(max(version), 0) + 1 AS "v!" FROM attachment WHERE sheet_id = $1 AND file_name = $2"#,
            sheet, file_name
        )
        .fetch_one(&mut *tx)
        .await?;
        let id = sqlx::query_scalar!(
            "INSERT INTO attachment (sheet_id, file_name, version, blob_hash, file_type, added_by) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
            sheet, file_name, version, blob_hash, file_type, self.me.id
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(AddOutcome { attachment: self.attachment(id).await?, duplicate: false })
    }

    async fn list_attachments(&self, sheet: Uuid) -> Result<Vec<Attachment>> {
        let sql = format!("{ATTACHMENT_SELECT} WHERE a.sheet_id = $1 ORDER BY a.added_at DESC, a.file_name");
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql)).bind(sheet).fetch_all(&self.pool).await?)
    }

    async fn attachment(&self, id: Uuid) -> Result<Attachment> {
        let sql = format!("{ATTACHMENT_SELECT} WHERE a.id = $1");
        sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| CoreError::not_found("Attachment"))
    }

    async fn set_read_state(&self, id: Uuid, state: &str, error: Option<&str>) -> Result<()> {
        sqlx::query!("UPDATE attachment SET read_state = $2, read_error = $3 WHERE id = $1", id, state, error)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_ai_state(&self, id: Uuid, state: &str, error: Option<&str>, note: Option<&str>) -> Result<()> {
        sqlx::query!(
            "UPDATE attachment SET ai_state = $2, ai_error = $3, ai_note = COALESCE($4, ai_note) WHERE id = $1",
            id, state, error, note
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_attachment_summary(&self, id: Uuid, summary: &str, confirms: i32) -> Result<()> {
        sqlx::query!("UPDATE attachment SET summary = $2, confirms_count = $3 WHERE id = $1", id, summary, confirms)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_excluded_sheets(&self, id: Uuid, sheets: &[String]) -> Result<()> {
        let r = sqlx::query!("UPDATE attachment SET excluded_sheets = $2 WHERE id = $1", id, sheets).execute(&self.pool).await?;
        require_row(r.rows_affected(), "Only project members can change this.")
    }

    async fn delete_attachment(&self, id: Uuid) -> Result<()> {
        let a = self.attachment(id).await?;
        let r = sqlx::query!(
            "UPDATE attachment SET deleted_at = now(), deleted_by = $2 WHERE id = $1 AND deleted_at IS NULL",
            id, self.me.id
        )
        .execute(&self.pool)
        .await?;
        require_row(r.rows_affected(), "Only the A3 owner can delete an attachment.")?;
        // Waiting cards from a removed file can no longer be traced, so they do not stay on the review screen.
        sqlx::query!("UPDATE ai_proposal SET state = 'rejected', decided_by = $2, decided_at = now() WHERE attachment_id = $1 AND state = 'pending'", id, self.me.id)
            .execute(&self.pool)
            .await?;
        self.audit("attachment", Some(id), Some(a.sheet_id), "attachment_deleted", None, json!({"fileName": a.file_name})).await
    }

    async fn restore_attachment(&self, id: Uuid) -> Result<()> {
        let a = self.attachment(id).await?;
        let r = sqlx::query!(
            "UPDATE attachment SET deleted_at = NULL, deleted_by = NULL WHERE id = $1 AND deleted_at > now() - interval '30 days'",
            id
        )
        .execute(&self.pool)
        .await?;
        require_row(r.rows_affected(), "This attachment can no longer be restored.")?;
        self.audit("attachment", Some(id), Some(a.sheet_id), "attachment_restored", None, json!({"fileName": a.file_name})).await
    }

    async fn unfinished_attachments(&self) -> Result<Vec<Attachment>> {
        // FR-ATT-06: interrupted jobs resume at next launch. Only the person who started them resumes them.
        let sql = format!(
            "{ATTACHMENT_SELECT} WHERE a.added_by = $1 AND a.deleted_at IS NULL
             AND (a.read_state IN ('pending', 'reading') OR a.ai_state IN ('pending', 'running')) ORDER BY a.added_at"
        );
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql)).bind(self.me.id).fetch_all(&self.pool).await?)
    }

    async fn replace_segments(&self, attachment: Uuid, segments: &[ExtractedSegment]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!("DELETE FROM source_segment WHERE attachment_id = $1", attachment).execute(&mut *tx).await?;
        for (i, s) in segments.iter().enumerate() {
            // PostgreSQL text cannot hold NUL, which some office files contain.
            let text = s.text.replace('\0', "");
            sqlx::query!(
                "INSERT INTO source_segment (attachment_id, seq, label, location, text, needs_image, ai_visible, group_name)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                attachment, i as i32, s.label, s.location, text, s.needs_image, s.ai_visible, s.group_name
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn segments(&self, attachment: Uuid) -> Result<Vec<Segment>> {
        Ok(sqlx::query_as!(
            Segment,
            "SELECT id, attachment_id, seq, label, location, text, from_image, needs_image, ai_visible, group_name
             FROM source_segment WHERE attachment_id = $1 ORDER BY seq",
            attachment
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn segment(&self, id: Uuid) -> Result<Segment> {
        sqlx::query_as!(
            Segment,
            "SELECT id, attachment_id, seq, label, location, text, from_image, needs_image, ai_visible, group_name
             FROM source_segment WHERE id = $1",
            id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| CoreError::new("SOURCE_REMOVED", "Source removed."))
    }

    async fn set_segment_from_image(&self, id: Uuid, text: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE source_segment SET text = $2, from_image = true, needs_image = false WHERE id = $1",
            id, text.replace('\0', "")
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn source_view(&self, segment: Uuid) -> Result<SourceView> {
        sqlx::query_as!(
            SourceView,
            r#"SELECT s.id AS segment_id, s.attachment_id, a.file_name, s.label, a.deleted_at IS NOT NULL AS "deleted!"
               FROM source_segment s JOIN attachment a ON a.id = s.attachment_id WHERE s.id = $1"#,
            segment
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| CoreError::new("SOURCE_REMOVED", "Source removed."))
    }

    async fn search_passages(&self, sheet: Uuid, attachment: Option<Uuid>, terms: &str, limit: i64) -> Result<(Vec<Passage>, i64)> {
        let total = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM source_segment s JOIN attachment a ON a.id = s.attachment_id
               WHERE a.sheet_id = $1 AND a.deleted_at IS NULL AND ($2::uuid IS NULL OR a.id = $2)
                 AND s.ai_visible AND NOT s.needs_image AND NOT (s.group_name IS NOT NULL AND s.group_name = ANY(a.excluded_sheets))"#,
            sheet, attachment
        )
        .fetch_one(&self.pool)
        .await?;
        // Any term may match: an OR query ranks passages rather than filtering them out.
        let words: Vec<String> = terms
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
            .take(40)
            .map(|w| w.to_lowercase())
            .collect();
        let query = words.join(" OR ");
        let rows = sqlx::query_as!(
            Passage,
            r#"SELECT s.id, a.file_name, s.label, s.text, s.from_image
               FROM source_segment s JOIN attachment a ON a.id = s.attachment_id
               WHERE a.sheet_id = $1 AND a.deleted_at IS NULL AND ($2::uuid IS NULL OR a.id = $2)
                 AND s.ai_visible AND NOT s.needs_image AND NOT (s.group_name IS NOT NULL AND s.group_name = ANY(a.excluded_sheets))
               ORDER BY ts_rank(s.search, websearch_to_tsquery('english', $3)) DESC, a.added_at DESC, s.seq
               LIMIT $4"#,
            sheet, attachment, query, limit
        )
        .fetch_all(&self.pool)
        .await?;
        Ok((rows, total))
    }

    async fn purge_deleted(&self) -> Result<i32> {
        Ok(sqlx::query_scalar!("SELECT purge_deleted_attachments()").fetch_one(&self.pool).await?.unwrap_or(0))
    }
}
