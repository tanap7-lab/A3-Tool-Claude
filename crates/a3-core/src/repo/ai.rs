use super::sheets::fetch_step;
use super::users::require_row;
use super::{AiRepo, PgRepository};
use crate::error::{CoreError, Result};
use crate::model::*;
use crate::richtext;
use async_trait::async_trait;
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

#[async_trait]
impl AiRepo for PgRepository {
    async fn insert_run(&self, r: &NewRun) -> Result<Uuid> {
        Ok(sqlx::query_scalar!(
            "INSERT INTO ai_run (sheet_id, job, provider, model, user_id, sources, outcome, input_tokens, output_tokens, discarded_count, discard_reasons)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING id",
            r.sheet_id, r.job, r.provider, r.model, self.me.id, r.sources, r.outcome,
            r.input_tokens, r.output_tokens, r.discarded_count, r.discard_reasons
        )
        .fetch_one(&self.pool)
        .await?)
    }

    async fn facts(&self, sheet: Uuid) -> Result<Vec<Fact>> {
        Ok(sqlx::query_as!(
            Fact,
            "SELECT id, letter, text, source_segment_id, state FROM a3_fact WHERE sheet_id = $1 ORDER BY created_at",
            sheet
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn insert_proposal(&self, p: &NewProposal) -> Result<Uuid> {
        Ok(sqlx::query_scalar!(
            "INSERT INTO ai_proposal (sheet_id, attachment_id, letter, kind, field, proposed_text, current_text, replaces_fact_id,
                source_segment_id, source_label, quote, from_image, why, norm_key, source_blob_hash, run_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16) RETURNING id",
            p.sheet_id, p.attachment_id, p.letter, p.kind, p.field, p.proposed_text, p.current_text, p.replaces_fact_id,
            p.source_segment_id, p.source_label, p.quote, p.from_image, p.why, p.norm_key, p.source_blob_hash, p.run_id
        )
        .fetch_one(&self.pool)
        .await?)
    }

    async fn proposals(&self, sheet: Uuid, pending_only: bool) -> Result<Vec<Proposal>> {
        Ok(sqlx::query_as!(
            Proposal,
            r#"SELECT p.id, p.sheet_id, p.attachment_id, p.letter, p.kind, p.field, p.proposed_text, p.current_text, p.source_segment_id,
                      p.source_label, p.quote, p.from_image, p.why, p.state,
                      COALESCE((SELECT st.state = 'closed' FROM a3_step st WHERE st.sheet_id = p.sheet_id AND st.letter = p.letter), false) AS "step_closed!",
                      p.created_at
               FROM ai_proposal p
               WHERE p.sheet_id = $1 AND (NOT $2 OR p.state = 'pending')
               ORDER BY p.kind = 'metadata' DESC, p.letter NULLS FIRST, p.created_at"#,
            sheet, pending_only
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn known_norm_keys(&self, sheet: Uuid, blob_hash: Option<&str>) -> Result<Vec<String>> {
        // Waiting and accepted points count from any file. Rejected points count only from the same file (FR-IMP-07).
        Ok(sqlx::query_scalar!(
            "SELECT norm_key FROM ai_proposal WHERE sheet_id = $1 AND (state <> 'rejected' OR source_blob_hash = $2)",
            sheet, blob_hash
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn decide_proposal(&self, id: Uuid, decision: &ProposalDecision) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let p = sqlx::query!(
            "SELECT sheet_id, letter, kind, field, proposed_text, current_text, replaces_fact_id, source_segment_id, source_label, from_image, state
             FROM ai_proposal WHERE id = $1 FOR UPDATE",
            id
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| CoreError::not_found("Proposal"))?;
        if p.state != "pending" {
            return Err(CoreError::conflict("This proposal was already decided."));
        }

        let (accepted, keep_old, text) = match decision {
            ProposalDecision::Accept { text } => (true, false, text.clone().unwrap_or_else(|| p.proposed_text.clone())),
            ProposalDecision::UseNew => (true, false, p.proposed_text.clone()),
            ProposalDecision::KeepBoth => (true, true, p.proposed_text.clone()),
            ProposalDecision::Reject | ProposalDecision::KeepCurrent => (false, true, p.proposed_text.clone()),
        };
        let text = text.trim().to_string();
        if accepted && text.is_empty() {
            return Err(CoreError::invalid("The text is empty."));
        }

        if accepted {
            if p.kind == "metadata" {
                apply_metadata(&mut tx, p.sheet_id, p.field.as_deref().unwrap_or(""), &text).await?;
            } else {
                let letter = p.letter.clone().ok_or_else(|| CoreError::invalid("This proposal has no step."))?;
                let step = sqlx::query!(
                    "SELECT id, content, state FROM a3_step WHERE sheet_id = $1 AND letter = $2 FOR UPDATE",
                    p.sheet_id, letter
                )
                .fetch_one(&mut *tx)
                .await?;
                if step.state == "closed" {
                    return Err(CoreError::new("A3_STEP_CLOSED", format!("Reopen {letter} to apply")));
                }
                let mut doc = step.content;
                let seg = p.source_segment_id.map(|u| u.to_string());
                let replaced = !keep_old
                    && p.kind != "add"
                    && p.current_text.as_deref().is_some_and(|old| {
                        richtext::replace_text(&mut doc, old, &text, seg.as_deref(), p.source_label.as_deref(), p.from_image)
                    });
                if !replaced {
                    richtext::append_block(&mut doc, richtext::sourced_paragraph(&text, seg.as_deref(), p.source_label.as_deref(), p.from_image));
                }
                let plain = richtext::plain_text(&doc);
                let version = sqlx::query_scalar!(
                    "UPDATE a3_step SET content = $2, plain_text = $3 WHERE id = $1 RETURNING version",
                    step.id, doc, plain
                )
                .fetch_one(&mut *tx)
                .await?;
                // FR-A3-09: a snapshot on every Accept.
                sqlx::query!(
                    "INSERT INTO a3_step_snapshot (step_id, sheet_id, letter, content, plain_text, version, author_id, reason)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, 'accept')",
                    step.id, p.sheet_id, letter, doc, plain, version, self.me.id
                )
                .execute(&mut *tx)
                .await?;
                if !keep_old {
                    if let Some(fact) = p.replaces_fact_id {
                        sqlx::query!("UPDATE a3_fact SET state = 'superseded' WHERE id = $1", fact).execute(&mut *tx).await?;
                    }
                }
                sqlx::query!(
                    "INSERT INTO a3_fact (sheet_id, letter, text, source_segment_id, created_by) VALUES ($1, $2, $3, $4, $5)",
                    p.sheet_id, letter, text, p.source_segment_id, self.me.id
                )
                .execute(&mut *tx)
                .await?;
            }
        } else if let Some(letter) = &p.letter {
            // The decision log: AI is told this point was turned down.
            sqlx::query!(
                "INSERT INTO a3_fact (sheet_id, letter, text, source_segment_id, state, created_by) VALUES ($1, $2, $3, $4, 'rejected', $5)",
                p.sheet_id, letter, p.proposed_text, p.source_segment_id, self.me.id
            )
            .execute(&mut *tx)
            .await?;
        }

        let r = sqlx::query!(
            "UPDATE ai_proposal SET state = $2, decided_by = $3, decided_at = now() WHERE id = $1",
            id, if accepted { "accepted" } else { "rejected" }, self.me.id
        )
        .execute(&mut *tx)
        .await?;
        require_row(r.rows_affected(), "Only project members can decide proposals.")?;
        let action = if accepted { "proposal_accepted" } else { "proposal_rejected" };
        sqlx::query!(
            "INSERT INTO audit_event (actor_id, object_type, object_id, sheet_id, action, detail) VALUES ($1, 'ai_proposal', $2, $3, $4, $5)",
            self.me.id, id, p.sheet_id, action, json!({"letter": p.letter, "kind": p.kind, "field": p.field})
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn add_passage_to_step(&self, step: Uuid, expected_version: i32, segment: Uuid, text: &str) -> Result<Step> {
        let text = text.trim();
        if text.is_empty() {
            return Err(CoreError::invalid("The passage is empty."));
        }
        let mut tx = self.pool.begin().await?;
        let st = sqlx::query!("SELECT sheet_id, letter, content, version, state FROM a3_step WHERE id = $1 FOR UPDATE", step)
            .fetch_one(&mut *tx)
            .await?;
        if st.state == "closed" {
            return Err(CoreError::new("A3_STEP_CLOSED", format!("Reopen {} to apply", st.letter)));
        }
        if st.version != expected_version {
            return Err(CoreError::conflict("This step changed. Refresh and try again."));
        }
        let seg = sqlx::query!(
            r#"SELECT s.label, s.from_image, a.sheet_id FROM source_segment s JOIN attachment a ON a.id = s.attachment_id WHERE s.id = $1"#,
            segment
        )
        .fetch_one(&mut *tx)
        .await?;
        if seg.sheet_id != st.sheet_id {
            return Err(CoreError::invalid("That passage belongs to another A3."));
        }
        let mut doc = st.content;
        let seg_id = segment.to_string();
        richtext::append_block(&mut doc, richtext::sourced_paragraph(text, Some(&seg_id), Some(&seg.label), seg.from_image));
        let plain = richtext::plain_text(&doc);
        let version = sqlx::query_scalar!("UPDATE a3_step SET content = $2, plain_text = $3 WHERE id = $1 RETURNING version", step, doc, plain)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query!(
            "INSERT INTO a3_step_snapshot (step_id, sheet_id, letter, content, plain_text, version, author_id, reason)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'accept')",
            step, st.sheet_id, st.letter, doc, plain, version, self.me.id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "INSERT INTO a3_fact (sheet_id, letter, text, source_segment_id, created_by) VALUES ($1, $2, $3, $4, $5)",
            st.sheet_id, st.letter, text, segment, self.me.id
        )
        .execute(&mut *tx)
        .await?;
        let saved = fetch_step(&mut *tx, step).await?;
        tx.commit().await?;
        Ok(saved)
    }

    async fn replace_findings(&self, sheet: Uuid, findings: &[NewFinding]) -> Result<Vec<Finding>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!("UPDATE ai_finding SET state = 'resolved' WHERE sheet_id = $1 AND state = 'open'", sheet)
            .execute(&mut *tx)
            .await?;
        for f in findings {
            sqlx::query!(
                "INSERT INTO ai_finding (sheet_id, check_code, letter, rank, text, why, source_segment_id, source_label, quote, step_text_hash, run_id)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
                f.sheet_id, f.check_code, f.letter, f.rank, f.text, f.why, f.source_segment_id, f.source_label, f.quote, f.step_text_hash, f.run_id
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        self.findings(sheet, true).await
    }

    async fn findings(&self, sheet: Uuid, open_only: bool) -> Result<Vec<Finding>> {
        Ok(sqlx::query_as!(
            Finding,
            "SELECT id, check_code, letter, rank, text, why, source_segment_id, source_label, quote, state, created_at
             FROM ai_finding WHERE sheet_id = $1 AND (NOT $2 OR state = 'open')
             ORDER BY rank, letter, created_at",
            sheet, open_only
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn dismiss_finding(&self, id: Uuid) -> Result<()> {
        let r = sqlx::query!("UPDATE ai_finding SET state = 'dismissed', decided_by = $2 WHERE id = $1", id, self.me.id)
            .execute(&self.pool)
            .await?;
        require_row(r.rows_affected(), "Only project members can dismiss findings.")
    }

    async fn dismissed_finding_keys(&self, sheet: Uuid) -> Result<Vec<(String, String, String)>> {
        let rows = sqlx::query!("SELECT check_code, letter, step_text_hash FROM ai_finding WHERE sheet_id = $1 AND state = 'dismissed'", sheet)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| (r.check_code, r.letter, r.step_text_hash)).collect())
    }

    async fn messages(&self, sheet: Uuid) -> Result<Vec<AiMessage>> {
        Ok(sqlx::query_as!(
            AiMessage,
            r#"SELECT m.id AS "id!", m.role AS "role!", m.text AS "text!", m.sources AS "sources!", u.display_name AS "user_name?", m.created_at AS "created_at!"
               FROM ai_message m LEFT JOIN app_user u ON u.id = m.user_id
               WHERE m.sheet_id = $1 ORDER BY m.created_at, m.id"#,
            sheet
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn insert_message(&self, sheet: Uuid, role: &str, text: &str, sources: &Value) -> Result<Uuid> {
        Ok(sqlx::query_scalar!(
            "INSERT INTO ai_message (sheet_id, role, text, sources, user_id) VALUES ($1, $2, $3, $4, $5) RETURNING id",
            sheet, role, text, sources, self.me.id
        )
        .fetch_one(&self.pool)
        .await?)
    }
}

async fn apply_metadata(tx: &mut Transaction<'_, Postgres>, sheet: Uuid, field: &str, value: &str) -> Result<()> {
    let date = |v: &str| chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(|_| CoreError::invalid("That is not a date."));
    let r = match field {
        "title" => {
            if value.chars().count() > 90 {
                return Err(CoreError::invalid("Titles can be up to 90 characters."));
            }
            sqlx::query!("UPDATE a3_sheet SET title = $2 WHERE id = $1", sheet, value).execute(&mut **tx).await?
        }
        "problem_date" => {
            let d = date(value)?;
            sqlx::query!("UPDATE a3_sheet SET problem_date = $2 WHERE id = $1", sheet, d).execute(&mut **tx).await?
        }
        "category" => sqlx::query!("UPDATE a3_sheet SET category = $2 WHERE id = $1", sheet, value).execute(&mut **tx).await?,
        "area" | "product" => {
            let r = if field == "area" {
                sqlx::query!("UPDATE a3_sheet SET area = $2 WHERE id = $1", sheet, value).execute(&mut **tx).await?
            } else {
                sqlx::query!("UPDATE a3_sheet SET product = $2 WHERE id = $1", sheet, value).execute(&mut **tx).await?
            };
            // FR-SET-04: when the project owner accepts a new value it joins the project's pick-list.
            let list = if field == "area" { "Area" } else { "Product" };
            sqlx::query!(
                "INSERT INTO pick_list_value (list_name, value, project_id)
                 SELECT $1, $2, sh.project_id FROM a3_sheet sh JOIN project p ON p.id = sh.project_id
                 WHERE sh.id = $3 AND p.owner_id = app_uid()
                 ON CONFLICT DO NOTHING",
                list, value, sheet
            )
            .execute(&mut **tx)
            .await?;
            r
        }
        _ => return Err(CoreError::invalid("Unknown metadata field.")),
    };
    require_row(r.rows_affected(), "Only project members can change this A3.")
}
