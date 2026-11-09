//! The fixed AI jobs of section 9.1. The pattern is the same for each: rebuild the A3's memory, call
//! the model, run the Rust checks, then persist or return only what survives. Discards are logged
//! to `ai_run` for quality tracking and never shown.

use super::context::{self, Context, ContextOptions};
use super::guard;
use super::prompts;
use super::provider::{AiProvider, CompletionRequest};
use crate::domain::{norm_key, normalize, sha256_hex, word_count};
use crate::error::{CoreError, Result};
use crate::extract::ImageData;
use crate::model::*;
use crate::repo::Repository;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

const MAX_TOKENS: u32 = 16_000;
const SHORT_TOKENS: u32 = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub added: usize,
    pub confirms: usize,
    pub discarded: usize,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub sentence_id: String,
    pub original: String,
    pub rewrite: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarningDraft {
    pub headline: String,
    pub what_happened: String,
    pub who_should_check: Vec<String>,
    pub quick_check: String,
    pub severity: String,
    pub cause_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CitedSource {
    pub segment_id: Option<Uuid>,
    pub label: String,
    pub quote: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskAnswer {
    pub text: String,
    pub sources: Vec<CitedSource>,
    pub note: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewOutcome {
    pub findings: Vec<Finding>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTest {
    pub key_works: bool,
    pub structured_output_works: bool,
    pub image_reading_works: bool,
    pub message: Option<String>,
}

/// Per-call accounting, written to `ai_run` when the job ends.
#[derive(Default)]
struct RunLog {
    input_tokens: i32,
    output_tokens: i32,
    discards: Vec<String>,
}

impl RunLog {
    fn discard(&mut self, check: &str, detail: impl Into<String>) {
        // Check code and reason only. A3 text stays out of the run log.
        let d = detail.into();
        self.discards.push(format!("{check}: {}", d.chars().take(120).collect::<String>()));
    }
}

pub struct AiService {
    pub repo: Arc<dyn Repository>,
    pub provider: Arc<dyn AiProvider>,
}

fn extract_json(text: &str) -> &str {
    let t = text.trim();
    match (t.find('{'), t.rfind('}')) {
        (Some(a), Some(b)) if b > a => &t[a..=b],
        _ => t,
    }
}

impl AiService {
    pub fn new(repo: Arc<dyn Repository>, provider: Arc<dyn AiProvider>) -> Self {
        Self { repo, provider }
    }

    /// D4: a project owner can turn AI off for a project.
    async fn ensure_enabled(&self, sheet: Uuid) -> Result<()> {
        let d = self.repo.sheet_detail(sheet).await?;
        if !d.sheet.project_ai_enabled {
            return Err(CoreError::new("AI_OFF", "AI is turned off for this project."));
        }
        Ok(())
    }

    /// G2: structured JSON only. Invalid output gets one retry, then a visible failure.
    async fn call_json<T: DeserializeOwned>(&self, req: CompletionRequest, log: &mut RunLog) -> Result<T> {
        let mut req = req;
        for attempt in 0..2 {
            let c = self.provider.complete(&req).await?;
            log.input_tokens += c.input_tokens;
            log.output_tokens += c.output_tokens;
            match serde_json::from_str::<T>(extract_json(&c.text)) {
                Ok(v) => return Ok(v),
                Err(e) => {
                    log.discard("G2", format!("attempt {}: {}", attempt + 1, e));
                    req.user.push_str("\n\nYour last reply was not valid JSON for the required schema. Reply again with valid JSON only.");
                }
            }
        }
        Err(CoreError::ai("The AI reply was not in the expected form."))
    }

    async fn finish(&self, sheet: Option<Uuid>, job: &str, ctx: Option<&Context>, log: RunLog, outcome: &str) -> Uuid {
        let sources: Value = ctx.map(|c| json!(c.source_ids)).unwrap_or_else(|| json!([]));
        let run = NewRun {
            sheet_id: sheet,
            job: job.into(),
            provider: self.provider.name().into(),
            model: self.provider.model().into(),
            sources,
            outcome: outcome.into(),
            input_tokens: log.input_tokens,
            output_tokens: log.output_tokens,
            discarded_count: log.discards.len() as i32,
            discard_reasons: json!(log.discards),
        };
        match self.repo.insert_run(&run).await {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(error = %e, "could not record ai_run");
                Uuid::nil()
            }
        }
    }

    /// G6: a second model call. Returns the ids of items that claim something their input does not.
    async fn verify_claims(&self, items: &[(String, String, String)], log: &mut RunLog) -> Result<HashSet<String>> {
        if items.is_empty() {
            return Ok(HashSet::new());
        }
        let mut user = String::new();
        for (id, input, output) in items {
            user.push_str(&format!("<item id=\"{id}\">\n<input>{input}</input>\n<output>{output}</output>\n</item>\n"));
        }
        #[derive(Deserialize)]
        struct R {
            adds_claim_ids: Vec<String>,
        }
        let r: R = self
            .call_json(
                CompletionRequest {
                    system: prompts::VERIFY_SYSTEM.into(),
                    user,
                    schema: Some(prompts::verify_schema()),
                    max_tokens: SHORT_TOKENS,
                    ..Default::default()
                },
                log,
            )
            .await?;
        Ok(r.adds_claim_ids.into_iter().collect())
    }

    // ---------------------------------------------------------------- import and new information

    /// Import mapping and the new-information check (sections 8 and 9.5). One job covers both,
    /// because an empty A3 simply has nothing to update or conflict with.
    pub async fn import_attachment(&self, attachment: Uuid) -> Result<ImportOutcome> {
        let att = self.repo.attachment(attachment).await?;
        self.ensure_enabled(att.sheet_id).await?;
        let mut log = RunLog::default();
        let ctx = context::build(
            self.repo.as_ref(),
            att.sheet_id,
            &ContextOptions { attachment: Some(attachment), terms: att.file_name.clone(), ..Default::default() },
        )
        .await?;
        if ctx.passages.is_empty() {
            self.finish(Some(att.sheet_id), "import", Some(&ctx), log, "no_text").await;
            return Ok(ImportOutcome { added: 0, confirms: 0, discarded: 0, note: None });
        }
        let picks = self.repo.pick_values(Some(ctx.sheet.project_id)).await?;
        let list = |n: &str| picks.iter().filter(|p| p.list_name == n).map(|p| p.value.clone()).collect::<Vec<_>>();
        let (categories, areas, products) = (list("Category"), list("Area"), list("Product"));

        #[derive(Deserialize)]
        struct Point {
            result: String,
            step: String,
            text: String,
            source: String,
            quote: String,
            replaces: Option<String>,
            why: Option<String>,
        }
        #[derive(Deserialize)]
        struct Meta {
            field: String,
            value: String,
            source: String,
            quote: String,
        }
        #[derive(Deserialize)]
        struct Reply {
            summary: String,
            points: Vec<Point>,
            metadata: Vec<Meta>,
        }
        let user = format!(
            "{}\nAllowed categories: {}.\nKnown areas: {}.\nKnown products: {}.\n\nThe new file is {}.",
            ctx.text,
            categories.join(", "),
            areas.join(", "),
            products.join(", "),
            att.file_name
        );
        let result = self
            .call_json::<Reply>(
                CompletionRequest {
                    system: prompts::import_system(),
                    user,
                    schema: Some(prompts::import_schema()),
                    max_tokens: MAX_TOKENS,
                    ..Default::default()
                },
                &mut log,
            )
            .await;
        let reply = match result {
            Ok(r) => r,
            Err(e) => {
                self.finish(Some(att.sheet_id), "import", Some(&ctx), log, "failed").await;
                return Err(e);
            }
        };

        // Accepted ledger text counts as known, so a repeated point becomes "confirms", whatever the model labelled it.
        let mut known: HashSet<String> = self.repo.known_norm_keys(att.sheet_id, Some(&att.blob_hash)).await?.into_iter().collect();
        for f in &ctx.accepted_facts {
            known.insert(norm_key(&f.text));
        }
        let mut confirms = 0usize;
        let mut pending: Vec<NewProposal> = Vec::new();

        for pt in reply.points {
            if pt.result == "confirms" {
                confirms += 1;
                continue;
            }
            let text = pt.text.trim().to_string();
            if text.is_empty() {
                log.discard("schema", "empty text");
                continue;
            }
            let Some(cite) = ctx.resolve(&pt.source).filter(|c| c.segment_id.is_some()) else {
                log.discard("G3", format!("unknown source id {}", pt.source));
                continue;
            };
            if !guard::quote_in_text(&pt.quote, &cite.text) {
                log.discard("G4", format!("quote not in {}", pt.source));
                continue;
            }
            let bad = guard::unsupported_tokens(&text, &cite.text);
            if !bad.is_empty() {
                log.discard("G5", format!("{} not in {}", bad.join(", "), pt.source));
                continue;
            }
            let (kind, replaces_fact, current) = match pt.result.as_str() {
                "new" => ("add", None, None),
                k @ ("update" | "conflict") => {
                    let Some(r) = pt.replaces.as_deref() else {
                        log.discard("schema", format!("{k} without replaces"));
                        continue;
                    };
                    let (fact, cur) = if let Some(f) = ctx.fact(r) {
                        (Some(f.id), f.text.clone())
                    } else if let Some(s) = ctx.sentences.get(r) {
                        (None, s.clone())
                    } else {
                        log.discard("G3", format!("unknown replaces id {r}"));
                        continue;
                    };
                    (k, fact, Some(cur))
                }
                other => {
                    log.discard("schema", format!("unknown result {other}"));
                    continue;
                }
            };
            let key = norm_key(&text);
            if !known.insert(key.clone()) {
                confirms += 1;
                continue;
            }
            pending.push(NewProposal {
                sheet_id: att.sheet_id,
                attachment_id: Some(attachment),
                letter: Some(pt.step),
                kind: kind.into(),
                proposed_text: text,
                current_text: current,
                replaces_fact_id: replaces_fact,
                source_segment_id: cite.segment_id,
                source_label: Some(cite.label),
                quote: Some(pt.quote),
                from_image: cite.from_image,
                why: pt.why.as_deref().map(two_sentences).filter(|w| !w.is_empty()),
                norm_key: key,
                source_blob_hash: Some(att.blob_hash.clone()),
                ..Default::default()
            });
        }

        let unset = |f: &str| match f {
            "title" => ctx.sheet.title.trim().is_empty(),
            "problem_date" => ctx.sheet.problem_date.is_none(),
            "product" => ctx.sheet.product.is_none(),
            "area" => ctx.sheet.area.is_none(),
            "category" => ctx.sheet.category.is_none(),
            _ => false,
        };
        let mut seen_fields = HashSet::new();
        for m in reply.metadata {
            let value = m.value.trim().to_string();
            if !unset(&m.field) || value.is_empty() || !seen_fields.insert(m.field.clone()) {
                continue;
            }
            let Some(cite) = ctx.resolve(&m.source).filter(|c| c.segment_id.is_some()) else {
                log.discard("G3", format!("unknown source id {}", m.source));
                continue;
            };
            if !guard::quote_in_text(&m.quote, &cite.text) {
                log.discard("G4", format!("quote not in {}", m.source));
                continue;
            }
            let valid = match m.field.as_str() {
                "problem_date" => chrono::NaiveDate::parse_from_str(&value, "%Y-%m-%d").is_ok(),
                "category" => categories.contains(&value),
                "title" => value.chars().count() <= 90 && guard::unsupported_tokens(&value, &cite.text).is_empty(),
                _ => guard::unsupported_tokens(&value, &cite.text).is_empty(),
            };
            if !valid {
                log.discard("G5", format!("metadata {} failed validation", m.field));
                continue;
            }
            // "Matched to it": prefer the project's own spelling when the value is already on its list.
            let matched = match m.field.as_str() {
                "area" => areas.iter().find(|a| normalize(a) == normalize(&value)).cloned(),
                "product" => products.iter().find(|a| normalize(a) == normalize(&value)).cloned(),
                _ => None,
            };
            let value = matched.unwrap_or(value);
            let key = norm_key(&format!("{}:{value}", m.field));
            if !known.insert(key.clone()) {
                continue;
            }
            pending.push(NewProposal {
                sheet_id: att.sheet_id,
                attachment_id: Some(attachment),
                kind: "metadata".into(),
                field: Some(m.field),
                proposed_text: value,
                source_segment_id: cite.segment_id,
                source_label: Some(cite.label),
                quote: Some(m.quote),
                from_image: cite.from_image,
                norm_key: key,
                source_blob_hash: Some(att.blob_hash.clone()),
                ..Default::default()
            });
        }

        let discarded = log.discards.len();
        let note = ctx.coverage.clone();
        let added = pending.len();
        let run = self.finish(Some(att.sheet_id), "import", Some(&ctx), log, "ok").await;
        for mut p in pending {
            p.run_id = Some(run).filter(|r| !r.is_nil());
            self.repo.insert_proposal(&p).await?;
        }
        self.repo.set_attachment_summary(attachment, reply.summary.trim(), confirms as i32).await?;
        Ok(ImportOutcome { added, confirms, discarded, note })
    }

    // ---------------------------------------------------------------- validity review

    pub async fn review(&self, sheet: Uuid) -> Result<ReviewOutcome> {
        self.ensure_enabled(sheet).await?;
        let mut log = RunLog::default();
        let ctx = context::build(self.repo.as_ref(), sheet, &ContextOptions::default()).await?;
        let current = ctx.current_letter();

        #[derive(Deserialize)]
        struct F {
            code: String,
            step: String,
            text: String,
            why: String,
            source: String,
            quote: String,
        }
        #[derive(Deserialize)]
        struct R {
            findings: Vec<F>,
        }
        let reply = self
            .call_json::<R>(
                CompletionRequest {
                    system: prompts::review_system(&current),
                    user: ctx.text.clone(),
                    schema: Some(prompts::review_schema()),
                    max_tokens: MAX_TOKENS,
                    ..Default::default()
                },
                &mut log,
            )
            .await;
        let reply = match reply {
            Ok(r) => r,
            Err(e) => {
                self.finish(Some(sheet), "review", Some(&ctx), log, "failed").await;
                return Err(e);
            }
        };

        let dismissed = self.repo.dismissed_finding_keys(sheet).await?;
        let mut kept = Vec::new();
        for f in reply.findings {
            let allowed_step = match check_step(&f.code) {
                Some("Any") => true,
                Some(s) => s == f.step,
                None => false,
            };
            if !allowed_step {
                log.discard("schema", format!("{} does not apply to step {}", f.code, f.step));
                continue;
            }
            // Section 9.3: only the current step and earlier ones.
            if f.step.as_str() > current.as_str() {
                log.discard("scope", format!("{} on later step {}", f.code, f.step));
                continue;
            }
            if word_count(&f.text) > 20 {
                log.discard("limit", format!("{} over 20 words", f.code));
                continue;
            }
            let Some(cite) = ctx.resolve(&f.source) else {
                log.discard("G3", format!("unknown source id {}", f.source));
                continue;
            };
            if !guard::quote_in_text(&f.quote, &cite.text) {
                log.discard("G4", format!("quote not in {}", f.source));
                continue;
            }
            let bad = guard::unsupported_tokens(&f.text, &ctx.text);
            if !bad.is_empty() {
                log.discard("G5", format!("{} not in the A3", bad.join(", ")));
                continue;
            }
            let hash = sha256_hex(normalize(&ctx.step(&f.step).map(|s| s.plain_text.clone()).unwrap_or_default()).as_bytes());
            // Dismissed findings stay dismissed unless the related text changes.
            if dismissed.iter().any(|(c, l, h)| *c == f.code && *l == f.step && *h == hash) {
                continue;
            }
            kept.push(NewFinding {
                sheet_id: sheet,
                rank: check_rank(&f.code),
                check_code: f.code,
                letter: f.step,
                text: f.text.trim().to_string(),
                why: Some(two_sentences(&f.why)),
                source_segment_id: cite.segment_id,
                source_label: Some(cite.label),
                quote: Some(f.quote),
                step_text_hash: hash,
                run_id: None,
            });
        }
        let run = self.finish(Some(sheet), "review", Some(&ctx), log, "ok").await;
        for k in &mut kept {
            k.run_id = Some(run).filter(|r| !r.is_nil());
        }
        let findings = self.repo.replace_findings(sheet, &kept).await?;
        Ok(ReviewOutcome { findings, note: ctx.coverage })
    }

    // ---------------------------------------------------------------- phrasing and condensing

    /// Shared by Improve and Condense. Certainty and every fact must survive unchanged.
    async fn rewrites(&self, sheet: Uuid, letter: &str, job: &str, system: String, selection: Option<&str>, limit: usize) -> Result<Vec<Suggestion>> {
        self.ensure_enabled(sheet).await?;
        let mut log = RunLog::default();
        let ctx = context::build(self.repo.as_ref(), sheet, &ContextOptions::default()).await?;
        let mut user = format!("{}\nThe step to work on is step {letter}.", ctx.text);
        if let Some(sel) = selection {
            user.push_str(&format!("\nOnly work on sentences within this selected text: <selection>{}</selection>", sel.replace("</", "<\\/")));
        }

        #[derive(Deserialize)]
        struct S {
            sentence: String,
            rewrite: String,
            why: String,
        }
        #[derive(Deserialize)]
        struct R {
            suggestions: Vec<S>,
        }
        let reply = self
            .call_json::<R>(
                CompletionRequest { system, user, schema: Some(prompts::suggestion_schema()), max_tokens: MAX_TOKENS, ..Default::default() },
                &mut log,
            )
            .await;
        let reply = match reply {
            Ok(r) => r,
            Err(e) => {
                self.finish(Some(sheet), job, Some(&ctx), log, "failed").await;
                return Err(e);
            }
        };

        let mut kept = Vec::new();
        for s in reply.suggestions.into_iter().take(limit) {
            if !s.sentence.starts_with(&format!("{letter}.")) {
                log.discard("scope", format!("{} is outside step {letter}", s.sentence));
                continue;
            }
            let Some(original) = ctx.sentences.get(s.sentence.trim()) else {
                log.discard("G3", format!("unknown sentence id {}", s.sentence));
                continue;
            };
            let rewrite = s.rewrite.trim().to_string();
            if rewrite.is_empty() || normalize(&rewrite) == normalize(original) {
                continue;
            }
            if let Some(reason) = guard::phrasing_violation(original, &rewrite) {
                log.discard("G5", format!("{}: {reason}", s.sentence));
                continue;
            }
            kept.push(Suggestion { sentence_id: s.sentence.trim().to_string(), original: original.clone(), rewrite, why: two_sentences(&s.why) });
        }
        let items: Vec<_> = kept.iter().map(|k| (k.sentence_id.clone(), k.original.clone(), k.rewrite.clone())).collect();
        let flagged = match self.verify_claims(&items, &mut log).await {
            Ok(f) => f,
            Err(e) => {
                self.finish(Some(sheet), job, Some(&ctx), log, "failed").await;
                return Err(e);
            }
        };
        kept.retain(|k| {
            let bad = flagged.contains(&k.sentence_id);
            if bad {
                log.discard("G6", k.sentence_id.clone());
            }
            !bad
        });
        self.finish(Some(sheet), job, Some(&ctx), log, "ok").await;
        Ok(kept)
    }

    pub async fn improve(&self, sheet: Uuid, letter: &str, selection: Option<&str>) -> Result<Vec<Suggestion>> {
        self.rewrites(sheet, letter, "phrasing", prompts::phrasing_system(), selection, 3).await
    }

    pub async fn condense(&self, sheet: Uuid, letter: &str, target_chars: usize) -> Result<Vec<Suggestion>> {
        self.rewrites(sheet, letter, "condense", prompts::condense_system(target_chars), None, 200).await
    }

    // ---------------------------------------------------------------- early warning draft

    pub async fn draft_warning(&self, sheet: Uuid) -> Result<WarningDraft> {
        self.ensure_enabled(sheet).await?;
        let mut log = RunLog::default();
        let opts = ContextOptions { hide_open_cause: true, ..Default::default() };
        let ctx = context::build(self.repo.as_ref(), sheet, &opts).await?;
        let closed = |l: &str| ctx.step(l).is_some_and(|s| s.state == "closed");
        let cause_open = !closed("B");
        let cause_status = crate::domain::cause_status(closed("B"), closed("D")).to_string();
        let picks = self.repo.pick_values(Some(ctx.sheet.project_id)).await?;
        let targets: Vec<String> = picks.iter().filter(|p| p.list_name == "Area" || p.list_name == "Product").map(|p| p.value.clone()).collect();
        // With B hidden, passages could still state a cause, so a draft while B is open sees step text only.
        let input = if cause_open { strip_passages(&ctx.text) } else { ctx.text.clone() };
        let user = format!("{input}\nAllowed areas and products: {}.", targets.join(", "));

        #[derive(Deserialize)]
        struct D {
            headline: String,
            what_happened: String,
            who_should_check: Vec<String>,
            quick_check: Option<String>,
            quick_check_source: Option<String>,
            quick_check_quote: Option<String>,
            source: String,
            quote: String,
        }
        let mut last_problem = String::new();
        for attempt in 0..2 {
            let mut req_user = user.clone();
            if attempt == 1 {
                req_user.push_str(&format!("\n\nYour previous draft was rejected: {last_problem}. Follow the rules exactly."));
            }
            let reply = self
                .call_json::<D>(
                    CompletionRequest {
                        system: prompts::warning_system(cause_open),
                        user: req_user,
                        schema: Some(prompts::warning_schema()),
                        max_tokens: SHORT_TOKENS,
                        ..Default::default()
                    },
                    &mut log,
                )
                .await;
            let d = match reply {
                Ok(d) => d,
                Err(e) => {
                    self.finish(Some(sheet), "warning_draft", Some(&ctx), log, "failed").await;
                    return Err(e);
                }
            };
            let (headline, what) = (d.headline.trim().to_string(), d.what_happened.trim().to_string());
            let draft_text = format!("{headline}\n{what}");
            let problem = if headline.is_empty() || headline.chars().count() > 80 {
                Some(("limit", "the headline must be 1 to 80 characters".to_string()))
            } else if what.is_empty() || word_count(&what) > 40 {
                Some(("limit", "what_happened must be 1 to 40 words".to_string()))
            } else if cause_open && guard::states_a_cause(&draft_text) {
                Some(("cause", "the cause is not confirmed, so the card may not state one".to_string()))
            } else if !ctx.resolve(&d.source).is_some_and(|c| guard::quote_in_text(&d.quote, &c.text)) {
                Some(("G4", "the citation for what_happened did not check out".to_string()))
            } else {
                let bad = guard::unsupported_tokens(&draft_text, &input);
                (!bad.is_empty()).then(|| ("G5", format!("these are not in the A3: {}", bad.join(", "))))
            };
            if let Some((check, why)) = problem {
                log.discard(check, why.clone());
                last_problem = why;
                continue;
            }

            // Quick check stays blank unless the A3 itself states the check.
            let quick = d.quick_check.as_deref().map(str::trim).filter(|q| !q.is_empty()).and_then(|q| {
                let cite = d.quick_check_source.as_deref().and_then(|id| ctx.resolve(id));
                let quote = d.quick_check_quote.as_deref().unwrap_or("");
                let ok = cite.is_some_and(|c| guard::quote_in_text(quote, &c.text))
                    && guard::unsupported_tokens(q, &input).is_empty()
                    && q.lines().count() <= 2
                    && !(cause_open && guard::states_a_cause(q));
                ok.then(|| q.to_string())
            });
            if d.quick_check.is_some() && quick.is_none() {
                log.discard("G4", "quick check could not be traced");
            }

            let items = vec![("card".to_string(), input.clone(), format!("{draft_text}\n{}", quick.as_deref().unwrap_or("")))];
            let flagged = match self.verify_claims(&items, &mut log).await {
                Ok(f) => f,
                Err(e) => {
                    self.finish(Some(sheet), "warning_draft", Some(&ctx), log, "failed").await;
                    return Err(e);
                }
            };
            if flagged.contains("card") {
                log.discard("G6", "draft claims something the A3 does not");
                last_problem = "it claims something the A3 does not".into();
                continue;
            }
            let who: Vec<String> = d.who_should_check.into_iter().filter(|w| targets.contains(w)).collect();
            self.finish(Some(sheet), "warning_draft", Some(&ctx), log, "ok").await;
            return Ok(WarningDraft {
                headline,
                what_happened: what,
                who_should_check: who,
                quick_check: quick.unwrap_or_default(),
                severity: ctx.sheet.severity.clone().unwrap_or_else(|| "Medium".into()),
                cause_status,
            });
        }
        self.finish(Some(sheet), "warning_draft", Some(&ctx), log, "failed").await;
        Err(CoreError::ai("AI could not produce a draft that passed the checks. You can write the card yourself."))
    }

    // ---------------------------------------------------------------- ask

    pub async fn ask(&self, sheet: Uuid, question: &str, more: bool) -> Result<AskAnswer> {
        let question = question.trim();
        if question.is_empty() {
            return Err(CoreError::invalid("Type a question."));
        }
        self.ensure_enabled(sheet).await?;
        let mut log = RunLog::default();
        let ctx = context::build(
            self.repo.as_ref(),
            sheet,
            &ContextOptions { terms: question.to_string(), ask_history: true, ..Default::default() },
        )
        .await?;
        let words = if more { 150 } else { 60 };
        #[derive(Deserialize)]
        struct Src {
            id: String,
            quote: String,
        }
        #[derive(Deserialize)]
        struct R {
            answer: String,
            sources: Vec<Src>,
        }
        let user = format!("{}\nQuestion: <question>{}</question>", ctx.text, question.replace("</", "<\\/"));
        let reply = self
            .call_json::<R>(
                CompletionRequest { system: prompts::ask_system(words), user, schema: Some(prompts::ask_schema()), max_tokens: SHORT_TOKENS, ..Default::default() },
                &mut log,
            )
            .await;
        let reply = match reply {
            Ok(r) => r,
            Err(e) => {
                self.finish(Some(sheet), "ask", Some(&ctx), log, "failed").await;
                return Err(e);
            }
        };

        let mut sources = Vec::new();
        for s in reply.sources {
            match ctx.resolve(&s.id) {
                Some(c) if guard::quote_in_text(&s.quote, &c.text) => {
                    sources.push(CitedSource { segment_id: c.segment_id, label: c.label, quote: s.quote })
                }
                Some(_) => log.discard("G4", format!("quote not in {}", s.id)),
                None => log.discard("G3", format!("unknown source id {}", s.id)),
            }
        }
        let bad = guard::unsupported_tokens(&reply.answer, &format!("{}\n{question}", ctx.text));
        let traced = !sources.is_empty() && bad.is_empty();
        if !bad.is_empty() {
            log.discard("G5", bad.join(", "));
        }
        let (text, sources, has_more) = if traced {
            let (t, cut) = limit_words(reply.answer.trim(), words);
            (t, sources, cut || !more)
        } else {
            // Principle 3: if it cannot be traced, it is not shown.
            ("I can't find that in this A3's content.".to_string(), Vec::new(), false)
        };
        self.repo.insert_message(sheet, "user", question, &json!([])).await?;
        self.repo.insert_message(sheet, "assistant", &text, &json!(sources)).await?;
        if let Err(e) = self.roll_up_history(sheet, &mut log).await {
            log.discard("summary", e.message);
        }
        self.finish(Some(sheet), "ask", Some(&ctx), log, "ok").await;
        Ok(AskAnswer { text, sources, note: ctx.coverage, has_more })
    }

    /// Section 9.2: the last 6 exchanges stay word for word. Older ones fold into a rolling summary.
    async fn roll_up_history(&self, sheet: Uuid, log: &mut RunLog) -> Result<()> {
        let messages = self.repo.messages(sheet).await?;
        let start = messages.iter().rposition(|m| m.role == "summary").unwrap_or(0);
        let previous = messages.get(start).filter(|m| m.role == "summary").map(|m| m.text.clone());
        let tail: Vec<&AiMessage> = messages.iter().skip(start).filter(|m| m.role != "summary").collect();
        if tail.len() <= 16 {
            return Ok(());
        }
        let old = &tail[..tail.len() - 12];
        let mut user = String::new();
        if let Some(p) = previous {
            user.push_str(&format!("Earlier summary: {p}\n"));
        }
        for m in old {
            user.push_str(&format!("{}: {}\n", m.role, m.text));
        }
        #[derive(Deserialize)]
        struct R {
            summary: String,
        }
        let r: R = self
            .call_json(
                CompletionRequest { system: prompts::SUMMARY_SYSTEM.into(), user, schema: Some(prompts::summary_schema()), max_tokens: SHORT_TOKENS, ..Default::default() },
                log,
            )
            .await?;
        self.repo.insert_message(sheet, "summary", r.summary.trim(), &json!([])).await?;
        Ok(())
    }

    // ---------------------------------------------------------------- follow-up suggestions

    pub async fn suggest_tasks(&self, warning: Uuid) -> Result<Vec<String>> {
        let w = self.repo.warning(warning).await?;
        self.ensure_enabled(w.sheet_id).await?;
        if w.quick_check.trim().is_empty() {
            return Ok(vec![]);
        }
        let mut log = RunLog::default();
        let input = format!("Headline: {}\nWhat happened: {}\nQuick check: {}", w.headline, w.what_happened, w.quick_check);
        #[derive(Deserialize)]
        struct R {
            tasks: Vec<String>,
        }
        let reply = self
            .call_json::<R>(
                CompletionRequest { system: prompts::tasks_system(), user: input.clone(), schema: Some(prompts::tasks_schema()), max_tokens: SHORT_TOKENS, ..Default::default() },
                &mut log,
            )
            .await;
        let reply = match reply {
            Ok(r) => r,
            Err(e) => {
                self.finish(Some(w.sheet_id), "follow_up", None, log, "failed").await;
                return Err(e);
            }
        };
        let mut kept = Vec::new();
        for t in reply.tasks.into_iter().take(3) {
            let bad = guard::unsupported_tokens(&t, &input);
            if bad.is_empty() && !t.trim().is_empty() {
                kept.push(t.trim().to_string());
            } else {
                log.discard("G5", bad.join(", "));
            }
        }
        // G6: these are drafts too, so the second check applies.
        let items: Vec<_> = kept.iter().enumerate().map(|(i, t)| (format!("T{i}"), input.clone(), t.clone())).collect();
        let flagged = match self.verify_claims(&items, &mut log).await {
            Ok(f) => f,
            Err(e) => {
                self.finish(Some(w.sheet_id), "follow_up", None, log, "failed").await;
                return Err(e);
            }
        };
        let kept: Vec<String> = kept.into_iter().enumerate().filter(|(i, _)| !flagged.contains(&format!("T{i}"))).map(|(_, t)| t).collect();
        self.finish(Some(w.sheet_id), "follow_up", None, log, "ok").await;
        Ok(kept)
    }

    // ---------------------------------------------------------------- reading pictures

    pub async fn read_image(&self, sheet: Uuid, images: Vec<ImageData>) -> Result<String> {
        if images.is_empty() {
            return Ok(String::new());
        }
        let mut log = RunLog::default();
        #[derive(Deserialize)]
        struct R {
            text: String,
        }
        let reply = self
            .call_json::<R>(
                CompletionRequest {
                    system: prompts::VISION_SYSTEM.into(),
                    user: "Read the text in this image.".into(),
                    images,
                    schema: Some(prompts::vision_schema()),
                    max_tokens: MAX_TOKENS,
                },
                &mut log,
            )
            .await;
        let outcome = if reply.is_ok() { "ok" } else { "failed" };
        self.finish(Some(sheet), "read_image", None, log, outcome).await;
        Ok(reply?.text)
    }
}

/// FR-AI-04: reports three things. A failed check does not hide the ones that passed.
pub async fn test_connection(provider: &dyn AiProvider) -> ConnectionTest {
    let mut t = ConnectionTest { key_works: false, structured_output_works: false, image_reading_works: false, message: None };
    #[derive(Deserialize)]
    struct Ok_ {
        ok: bool,
    }
    let schema = json!({"type": "object", "properties": {"ok": {"type": "boolean"}}, "required": ["ok"], "additionalProperties": false});
    let req = CompletionRequest {
        system: "Reply with JSON only.".into(),
        user: "Return {\"ok\": true}.".into(),
        schema: Some(schema),
        max_tokens: 2000,
        ..Default::default()
    };
    match provider.complete(&req).await {
        Ok(c) => {
            t.key_works = true;
            t.structured_output_works = serde_json::from_str::<Ok_>(extract_json(&c.text)).map(|o| o.ok).unwrap_or(false);
        }
        Err(e) => {
            t.message = Some(e.message);
            return t;
        }
    }
    #[derive(Deserialize)]
    struct Colour {
        colour: String,
    }
    let schema = json!({"type": "object", "properties": {"colour": {"type": "string"}}, "required": ["colour"], "additionalProperties": false});
    let vreq = CompletionRequest {
        system: "Reply with JSON only.".into(),
        user: "What colour fills this image? Answer with one common colour name in the field colour.".into(),
        images: vec![test_image()],
        schema: Some(schema),
        max_tokens: 2000,
    };
    match provider.complete(&vreq).await {
        Ok(c) => {
            t.image_reading_works = serde_json::from_str::<Colour>(extract_json(&c.text))
                .map(|x| x.colour.to_lowercase().contains("red"))
                .unwrap_or(false);
        }
        Err(e) => t.message = Some(format!("Image reading failed: {}", e.message)),
    }
    t
}

fn test_image() -> ImageData {
    use base64::Engine;
    let img = image::RgbImage::from_pixel(64, 64, image::Rgb([220, 20, 20]));
    let mut buf = std::io::Cursor::new(Vec::new());
    let _ = image::DynamicImage::ImageRgb8(img).write_to(&mut buf, image::ImageFormat::Png);
    ImageData { mime: "image/png".into(), base64: base64::engine::general_purpose::STANDARD.encode(buf.into_inner()) }
}

/// The step a check applies to.
pub fn check_step(code: &str) -> Option<&'static str> {
    Some(match code {
        "V1" => "Any",
        "V2" | "V3" => "A",
        "V4" | "V5" => "B",
        "V6" | "V7" | "V8" => "C",
        "V9" | "V10" => "D",
        _ => return None,
    })
}

/// Section 9.3: contradictions, then broken logic, then missing detail.
pub fn check_rank(code: &str) -> i32 {
    match code {
        "V1" => 1,
        "V2" | "V5" | "V6" | "V7" | "V9" => 2,
        _ => 3,
    }
}

fn two_sentences(text: &str) -> String {
    crate::domain::split_sentences(text).into_iter().take(2).collect::<Vec<_>>().join(" ")
}

fn limit_words(text: &str, max: usize) -> (String, bool) {
    if word_count(text) <= max {
        return (text.to_string(), false);
    }
    let mut out: Vec<String> = Vec::new();
    let mut n = 0;
    for s in crate::domain::split_sentences(text) {
        let w = word_count(&s);
        if n + w > max {
            break;
        }
        n += w;
        out.push(s);
    }
    if out.is_empty() {
        let cut: Vec<&str> = text.split_whitespace().take(max).collect();
        return (format!("{}…", cut.join(" ")), true);
    }
    (out.join(" "), true)
}

fn strip_passages(text: &str) -> String {
    match (text.find("<source_passages>"), text.find("</source_passages>")) {
        (Some(a), Some(b)) if b > a => format!("{}{}", &text[..a], &text[b + "</source_passages>".len()..]),
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_found_inside_fences() {
        assert_eq!(extract_json("```json\n{\"a\":1}\n```"), "{\"a\":1}");
    }

    #[test]
    fn checks_rank_contradictions_first() {
        assert!(check_rank("V1") < check_rank("V2"));
        assert!(check_rank("V2") < check_rank("V3"));
        assert_eq!(check_step("V8"), Some("C"));
        assert_eq!(check_step("V11"), None);
    }

    #[test]
    fn answers_are_limited_by_sentence() {
        let (t, cut) = limit_words("One two three. Four five six. Seven eight nine.", 6);
        assert_eq!(t, "One two three. Four five six.");
        assert!(cut);
        assert_eq!(limit_words("Short.", 60), ("Short.".to_string(), false));
    }

    #[test]
    fn passages_can_be_removed_from_a_prompt() {
        let t = "<sheet_state>x</sheet_state>\n<source_passages>secret cause</source_passages>\nend";
        assert!(!strip_passages(t).contains("secret"));
        assert!(strip_passages(t).contains("<sheet_state>"));
    }
}
