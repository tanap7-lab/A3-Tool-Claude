//! The fixed AI jobs of section 9.1. The pattern is the same for each: rebuild the A3's memory, call
//! the model, run the Rust checks, then persist or return only what survives. Discards are logged
//! to `ai_run` for quality tracking and never shown.

use super::context::{self, Context, ContextOptions};
use super::guard;
use super::prompts::{self, AskKnowledge};
use super::provider::{AiProvider, CompletionRequest, WebSource};
use crate::domain::{norm_key, normalize, sha256_hex, word_count};
use crate::error::{CoreError, Result};
use crate::extract::ImageData;
use crate::model::*;
use crate::repo::Repository;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

const MAX_TOKENS: u32 = 16_000;
const SHORT_TOKENS: u32 = 2_000;
/// The warning card itself is about 150 tokens of JSON, but reasoning models spend the same budget on thinking
/// first. 2,000 was routinely used up before the JSON began, so the draft always came back cut off.
const DRAFT_TOKENS: u32 = 4_096;

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
pub struct A3Summary {
    pub steps: Vec<SummaryStep>,
    pub note: Option<String>,
    pub moves: Vec<SummaryMove>,
    #[serde(default)]
    pub tone: SummaryTone,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryTone {
    #[default]
    Plain,
    Executive,
    Technical,
}

impl SummaryTone {
    pub fn instruction(self) -> &'static str {
        match self {
            Self::Plain => "Plain English: rewrite awkward notes substantially into natural, direct sentences with familiar words and a clear subject and verb. Split overloaded sentences and remove bureaucratic phrasing. Do more than swap a few synonyms.",
            Self::Executive => "Executive briefing: lead with the issue, impact, decisions and action status. Use crisp, professional sentences and front-load the important point. Select the essentials for the short story; preserve the qualifications of selected claims. Complete evidence, owners and dates remain in the supporting details.",
            Self::Technical => "Technical report: use precise, neutral engineering language and an evidence-to-conclusion structure. Make relationships explicit only where the input states them, retaining terminology, measurement conditions and uncertainty.",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryMove {
    pub from: String,
    pub to: String,
    pub text: String,
}

#[derive(Deserialize, Serialize)]
struct SummaryStory {
    text: String,
    sources: Vec<String>,
}

#[derive(Deserialize)]
struct SummaryPlacement {
    letter: String,
    blocks: Vec<String>,
    story: Vec<SummaryStory>,
}

#[derive(Deserialize)]
struct SummaryStoryEdit {
    letter: String,
    story: Vec<SummaryStory>,
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
    /// `a3` for a passage or step sentence, `background` for outside knowledge (its text is in `quote`),
    /// `web` for a page the search tool returned (its address is in `url`). Stored with the message.
    #[serde(default = "a3_kind")]
    pub kind: String,
    #[serde(default)]
    pub url: Option<String>,
}

fn a3_kind() -> String {
    "a3".into()
}

/// What the user chose for one Ask. Both choices live in the app on the user's own machine and are sent
/// with each question, so nothing about them is shared with colleagues.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskOptions {
    /// Where the answer may draw from. This A3 only, unless the user widens it.
    pub knowledge: AskKnowledge,
    /// Whether earlier turns are sent to the model. Off means every question stands alone.
    pub memory: bool,
    /// The user started fresh at this moment. Earlier turns are not sent.
    pub since: Option<Ts>,
}

impl Default for AskOptions {
    fn default() -> Self {
        Self { knowledge: AskKnowledge::A3Only, memory: true, since: None }
    }
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
    /// Pages the provider's search tool returned during the job.
    web_sources: Vec<WebSource>,
}

impl RunLog {
    fn absorb(&mut self, other: Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.discards.extend(other.discards);
    }

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
            let c = match self.provider.complete(&req).await {
                // A cut-off reply is never usable, and the usual cause is thinking tokens eating the budget.
                Err(e) if e.code == "AI_TRUNCATED" && attempt == 0 && req.max_tokens < MAX_TOKENS => {
                    log.discard("G2", "truncated reply; retrying once with more output space");
                    req.max_tokens = (req.max_tokens * 2).min(MAX_TOKENS);
                    continue;
                }
                other => other?,
            };
            log.input_tokens += c.input_tokens;
            log.output_tokens += c.output_tokens;
            for w in c.web_sources {
                if !log.web_sources.iter().any(|x| x.url == w.url) {
                    log.web_sources.push(w);
                }
            }
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
                    max_tokens: DRAFT_TOKENS,
                    fast: true,
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

    pub async fn summarize(&self, sheet: Uuid) -> Result<A3Summary> {
        self.summarize_with_tone(sheet, SummaryTone::default()).await
    }

    pub async fn summarize_with_tone(&self, sheet: Uuid, tone: SummaryTone) -> Result<A3Summary> {
        let started = Instant::now();
        let ctx = context::build(self.repo.as_ref(), sheet, &ContextOptions { omit_step_text: true, ..Default::default() }).await?;
        if !ctx.sheet.project_ai_enabled {
            return Err(CoreError::new("AI_OFF", "AI is turned off for this project."));
        }
        if !ctx.sheet.can_edit || ctx.sheet.withdrawn_at.is_some() {
            return Err(CoreError::ai("Only an editable A3 can be summarized."));
        }
        let mut nodes = BTreeMap::new();
        let mut blocks = BTreeMap::new();
        let mut outline = Vec::new();
        let mut detail_docs = BTreeMap::new();
        for step in &ctx.steps {
            let details = if step.state == "closed" { step.content.clone() } else { crate::richtext::summary_evidence(&step.content) };
            for (id, text) in crate::richtext::text_nodes(&details) {
                nodes.insert(format!("{}.{id}", step.letter), text);
            }
            for (i, block) in details["content"].as_array().into_iter().flatten().enumerate() {
                let id = format!("{}.{i}", step.letter);
                let prefix = format!("{id}.");
                let text: BTreeMap<_, _> = nodes.iter().filter(|(key, _)| key.starts_with(&prefix)).collect();
                outline.push(json!({"block": id, "type": block["type"], "step_state": step.state, "text_nodes": text}));
                blocks.insert(id, block.clone());
            }
            detail_docs.insert(step.letter.clone(), details);
        }
        if nodes.values().all(|text| text.trim().is_empty()) {
            return Err(CoreError::ai("Add some text to this A3 before summarizing."));
        }
        #[derive(Deserialize)]
        struct Rewrite {
            node: String,
            rewrite: String,
        }
        #[derive(Deserialize)]
        struct Plan {
            steps: Vec<SummaryPlacement>,
            rewrites: Vec<Rewrite>,
        }
        let mut log = RunLog::default();
        let result = tokio::time::timeout(Duration::from_secs(180), async {
            let mut plan: Plan = self.call_json(
                CompletionRequest {
                    system: format!("{}\n\nRequested tone: {}", prompts::summarize_system(), tone.instruction()),
                    user: format!("{}\nReview, summarize and organize the full A3 using these document blocks:\n{}", ctx.text, json!(outline)),
                    schema: Some(prompts::summarize_schema()),
                    max_tokens: MAX_TOKENS,
                    fast: true,
                    ..Default::default()
                },
                &mut log,
            ).await?;
            let mut rewrites = BTreeMap::new();
            let mut seen_rewrites = HashSet::new();
            let mut retained_details = 0;
            for r in plan.rewrites {
                let original = nodes.get(&r.node).ok_or_else(|| CoreError::ai("The AI referenced an unknown text node. Please try again."))?;
                let text = r.rewrite.trim().to_string();
                if !seen_rewrites.insert(r.node.clone()) || text.is_empty() {
                    return Err(CoreError::ai("The AI returned repeated or empty text. Please try again."));
                }
                if text == *original {
                    continue;
                }
                if ctx.step(r.node.split('.').next().unwrap_or("")).is_some_and(|s| s.state == "closed") {
                    return Err(CoreError::ai("A proposed change affects a closed step. Reopen it before summarizing."));
                }
                if let Some(reason) = guard::phrasing_violation(original, &text) {
                    log.discard("G5", format!("{}: {reason}", r.node));
                    retained_details += 1;
                    continue;
                }
                rewrites.insert(r.node, text);
            }
            let mut seen_steps = HashSet::new();
            let mut seen_blocks = HashSet::new();
            let mut steps = Vec::new();
            let mut moves = Vec::new();
            for placement in &plan.steps {
                let step = ctx.step(&placement.letter).ok_or_else(|| CoreError::ai("The AI returned an unknown step."))?;
                if !seen_steps.insert(placement.letter.clone()) {
                    return Err(CoreError::ai("The AI repeated a step."));
                }
                let mut content = Vec::new();
                for id in &placement.blocks {
                    let block = blocks.get(id).ok_or_else(|| CoreError::ai("The AI referenced an unknown block."))?;
                    if !seen_blocks.insert(id.clone()) {
                        return Err(CoreError::ai("The AI repeated a document block."));
                    }
                    let from_letter = id.split('.').next().unwrap_or("");
                    if from_letter != step.letter && ctx.step(from_letter).is_some_and(|s| s.state == "closed") {
                        return Err(CoreError::ai("A proposed move changes a closed step. Reopen it before summarizing."));
                    }
                    if from_letter != step.letter {
                        moves.push(SummaryMove { from: from_letter.into(), to: step.letter.clone(), text: crate::richtext::plain_text(&blocks[id]) });
                    }
                    content.push(block.clone());
                }
                let mut doc = step.content.clone();
                doc["content"] = if content.is_empty() { json!([{"type": "paragraph"}]) } else { json!(content) };
                if step.state == "closed" && doc != step.content {
                    return Err(CoreError::ai("A proposed change affects a closed step. Reopen it before summarizing."));
                }
                steps.push(SummaryStep { id: step.id, letter: step.letter.clone(), original: step.content.clone(), content: doc, version: step.version });
            }
            if seen_steps.len() != ctx.steps.len() || seen_blocks.len() != blocks.len() {
                return Err(CoreError::ai("The AI omitted a step or document block. Your A3 was not changed. Please try again."));
            }
            steps.sort_by(|a, b| a.letter.cmp(&b.letter));
            let issues = summary_story_issues(&plan.steps, &ctx, &blocks);
            let mut repaired = false;
            if !issues.is_empty() {
                self.repair_summary_stories(&mut plan.steps, &ctx, &blocks, &issues, tone, &mut log).await?;
                repaired = true;
                ensure_summary_stories(&plan.steps, &ctx, &blocks)?;
            }
            let mut items = summary_story_items(&plan.steps, &blocks);
            for (id, rewrite) in &rewrites {
                let original = &nodes[id];
                items.push((id.clone(), original.clone(), rewrite.clone()));
            }
            let flagged = self.verify_summary(&items, &mut log).await?;
            let mut story_issues = BTreeMap::new();
            for id in flagged {
                log.discard("G6", format!("{id}: proposed claims did not pass verification"));
                if let Some(letter) = id.strip_prefix("story:").and_then(|id| id.split('.').next()) {
                    story_issues.insert(letter.to_string(), vec![
                        "A story claim failed factual verification. Remove unsupported claims and causal links, and preserve the original qualifications, quantities, certainty and action status of every selected claim.".to_string(),
                    ]);
                } else if rewrites.remove(&id).is_some() {
                    retained_details += 1;
                }
            }
            if !story_issues.is_empty() {
                if repaired {
                    return Err(CoreError::ai("The corrected story still did not pass factual verification. Your A3 was not changed. Check that the supporting notes clearly distinguish evidence, hypotheses and action status, then regenerate."));
                }
                self.repair_summary_stories(&mut plan.steps, &ctx, &blocks, &story_issues, tone, &mut log).await?;
                ensure_summary_stories(&plan.steps, &ctx, &blocks)?;
                let corrected: Vec<_> = summary_story_items(&plan.steps, &blocks).into_iter()
                    .filter(|(id, _, _)| id.strip_prefix("story:").and_then(|id| id.split('.').next()).is_some_and(|letter| story_issues.contains_key(letter)))
                    .collect();
                if !self.verify_summary(&corrected, &mut log).await?.is_empty() {
                    return Err(CoreError::ai("The corrected story still did not pass factual verification. Your A3 was not changed."));
                }
            }
            let rewritten_blocks = summary_rewritten_blocks(&detail_docs, &rewrites)?;
            for step in &mut steps {
                let placement = plan.steps.iter().find(|p| p.letter == step.letter).expect("all placements validated");
                let details: Vec<_> = placement.blocks.iter().map(|id| rewritten_blocks[id].clone()).collect();
                step.content["content"] = if details.is_empty() { json!([{"type": "paragraph"}]) } else { json!(details) };
                let story = placement.story.iter().map(|paragraph| {
                    let sources: Vec<_> = paragraph.sources.iter().map(|id| blocks[id].clone()).collect();
                    crate::richtext::narrative_paragraph(paragraph.text.trim(), &sources)
                }).collect();
                step.content = crate::richtext::story_document(&step.content, story);
            }
            let mut notes: Vec<String> = ctx.coverage.clone().into_iter().collect();
            if repaired || !story_issues.is_empty() {
                notes.push("The AI automatically corrected the story before it passed the checks.".into());
            }
            if retained_details > 0 {
                notes.push(format!("{retained_details} supporting passage(s) were kept unchanged because their suggested rewrites did not pass factual checks. The verified story is shown above the complete evidence."));
            }
            Ok(A3Summary { steps, note: (!notes.is_empty()).then(|| notes.join("\n")), moves, tone })
        }).await.unwrap_or_else(|_| Err(CoreError::new("AI_TIMEOUT", "The summary exceeded 3 minutes. Your A3 was not changed. Try a faster model or try again.")));
        if let Err(e) = &result {
            log.discard("summary", &e.message);
        }
        self.finish(Some(sheet), "summarize", Some(&ctx), log, if result.is_ok() { "ok" } else { "failed" }).await;
        tracing::info!(elapsed_ms = started.elapsed().as_millis() as u64, outcome = if result.is_ok() { "ok" } else { "failed" }, "A3 summary finished");
        result
    }

    async fn repair_summary_stories(
        &self,
        placements: &mut [SummaryPlacement],
        ctx: &Context,
        blocks: &BTreeMap<String, Value>,
        issues: &BTreeMap<String, Vec<String>>,
        tone: SummaryTone,
        log: &mut RunLog,
    ) -> Result<()> {
        #[derive(Deserialize)]
        struct Reply {
            steps: Vec<SummaryStoryEdit>,
        }
        let input: Vec<_> = placements.iter().filter(|p| issues.contains_key(&p.letter)).map(|p| {
            log.discard("summary-repair", format!("{}: {}", p.letter, issues[&p.letter].join("; ")));
            let evidence: Vec<_> = p.blocks.iter().map(|id| json!({"block": id, "text": crate::richtext::plain_text(&blocks[id])})).collect();
            json!({
                "letter": p.letter,
                "state": ctx.step(&p.letter).map(|s| &s.state),
                "problems": issues[&p.letter],
                "previous_story": p.story,
                "placed_blocks": evidence,
            })
        }).collect();
        let reply: Reply = self.call_json(CompletionRequest {
            system: format!("{}\n\nRequested tone: {}", prompts::SUMMARY_REPAIR_SYSTEM, tone.instruction()),
            user: json!(input).to_string(),
            schema: Some(prompts::summary_repair_schema()),
            max_tokens: 4_096,
            fast: true,
            ..Default::default()
        }, log).await?;
        let mut seen = HashSet::new();
        if reply.steps.len() != issues.len() || reply.steps.iter().any(|s| !issues.contains_key(&s.letter) || !seen.insert(s.letter.clone())) {
            return Err(CoreError::ai("The AI correction omitted, repeated or changed a section. Your A3 was not changed."));
        }
        for edit in reply.steps {
            placements.iter_mut().find(|p| p.letter == edit.letter).expect("correction letters validated").story = edit.story;
        }
        Ok(())
    }

    async fn verify_summary_batch(&self, items: &[(String, String, String)], log: &mut RunLog) -> Result<HashSet<String>> {
        if items.is_empty() {
            return Ok(HashSet::new());
        }
        #[derive(Deserialize)]
        struct Verdict {
            rejected_ids: Vec<String>,
        }
        let input: Vec<_> = items.iter().map(|(id, original, rewrite)| json!({"id": id, "original": original, "rewrite": rewrite})).collect();
        let verdict: Verdict = self.call_json(
            CompletionRequest {
                system: prompts::SUMMARY_VERIFY_SYSTEM.into(),
                user: json!(input).to_string(),
                schema: Some(prompts::summary_verify_schema()),
                max_tokens: 4_096,
                fast: true,
                ..Default::default()
            }, log,
        ).await.map_err(|e| CoreError::new(e.code, format!("Summary fact-check failed: {}", e.message)))?;
        if verdict.rejected_ids.iter().any(|id| !items.iter().any(|(known, _, _)| known == id)) {
            return Err(CoreError::ai("The summary fact-check returned an unknown item. Your A3 was not changed."));
        }
        Ok(verdict.rejected_ids.into_iter().collect())
    }

    async fn verify_summary(&self, items: &[(String, String, String)], log: &mut RunLog) -> Result<HashSet<String>> {
        let batches = summary_batches(items);
        let mut flagged = HashSet::new();
        for pair in batches.chunks(2) {
            let mut left_log = RunLog::default();
            let mut right_log = RunLog::default();
            let (left, right) = tokio::join!(
                self.verify_summary_batch(pair[0], &mut left_log),
                self.verify_summary_batch(pair.get(1).copied().unwrap_or(&[]), &mut right_log),
            );
            log.absorb(left_log);
            log.absorb(right_log);
            flagged.extend(left?);
            flagged.extend(right?);
        }
        Ok(flagged)
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
                        max_tokens: DRAFT_TOKENS,
                        fast: true,
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
        self.ask_with(sheet, question, more, &AskOptions::default()).await
    }

    pub async fn ask_with(&self, sheet: Uuid, question: &str, more: bool, opts: &AskOptions) -> Result<AskAnswer> {
        let question = question.trim();
        if question.is_empty() {
            return Err(CoreError::invalid("Type a question."));
        }
        if opts.knowledge == AskKnowledge::Web && !self.provider.supports_web() {
            return Err(CoreError::invalid("Web search is not available with this AI provider. Choose general knowledge, or this A3 only."));
        }
        self.ensure_enabled(sheet).await?;
        let mut log = RunLog::default();
        let ctx = context::build(
            self.repo.as_ref(),
            sheet,
            &ContextOptions { terms: question.to_string(), ask_history: opts.memory, history_since: opts.since, ..Default::default() },
        )
        .await?;
        // A question the user has asked before gets a different explanation, not the same words again.
        let repeat = if opts.memory {
            let earlier = self.repo.messages(sheet).await?;
            is_repeat(question, earlier.iter().filter(|m| opts.since.is_none_or(|s| m.created_at > s)))
        } else {
            false
        };
        let hint = match (repeat, more) {
            (true, true) => "<repeat_question>The user wants more on a question already answered. Add what the earlier answer left out and do not restate it.</repeat_question>\n",
            (true, false) => "<repeat_question>The user has asked this before and the earlier answer is in <ask_history>. Do not restate it. Explain again from a different angle, in different words, with a different structure or a concrete example.</repeat_question>\n",
            _ => "",
        };
        let words = if more { 150 } else { 60 };
        let open = opts.knowledge.is_open();
        #[derive(Deserialize)]
        struct Src {
            id: String,
            quote: String,
        }
        #[derive(Deserialize)]
        struct R {
            answer: String,
            sources: Vec<Src>,
            #[serde(default)]
            background: String,
        }
        let user = format!("{}\n{hint}Question: <question>{}</question>", ctx.text, question.replace("</", "<\\/"));
        let request = CompletionRequest {
            system: prompts::ask_system(words, opts.knowledge),
            user,
            schema: Some(if open { prompts::ask_open_schema() } else { prompts::ask_schema() }),
            max_tokens: match opts.knowledge {
                AskKnowledge::A3Only => SHORT_TOKENS,
                AskKnowledge::General => 3_000,
                AskKnowledge::Web => 6_000,
            },
            web: opts.knowledge == AskKnowledge::Web,
            ..Default::default()
        };
        let job = match opts.knowledge {
            AskKnowledge::A3Only => "ask",
            AskKnowledge::General => "ask_general",
            AskKnowledge::Web => "ask_web",
        };
        let reply = match self.call_json::<R>(request, &mut log).await {
            Ok(r) => r,
            Err(e) => {
                self.finish(Some(sheet), job, Some(&ctx), log, "failed").await;
                return Err(e);
            }
        };

        let mut sources = Vec::new();
        for s in reply.sources {
            match ctx.resolve(&s.id) {
                Some(c) if guard::quote_in_text(&s.quote, &c.text) => sources.push(CitedSource {
                    segment_id: c.segment_id,
                    label: c.label,
                    quote: s.quote,
                    kind: a3_kind(),
                    url: None,
                }),
                Some(_) => log.discard("G4", format!("quote not in {}", s.id)),
                None => log.discard("G3", format!("unknown source id {}", s.id)),
            }
        }
        // G5 accepts only what the A3 holds. Outside knowledge from earlier turns is not evidence about it.
        let bad = guard::unsupported_tokens(&reply.answer, &format!("{}\n{question}", context::evidence_text(&ctx.text)));
        if !bad.is_empty() {
            log.discard("G5", bad.join(", "));
        }
        let a3_part = !reply.answer.trim().is_empty() && !sources.is_empty() && bad.is_empty();
        // The background part is outside knowledge by design, so G3 to G5 do not apply to it. It is kept
        // apart, labelled, and never mixed into the part that cites this A3.
        let background = if open { reply.background.trim().to_string() } else { String::new() };

        let (text, sources, background, cut) = if a3_part || !background.is_empty() {
            let (text, cut) = if a3_part {
                limit_words(reply.answer.trim(), words)
            } else {
                ("This A3 does not say anything about that.".to_string(), false)
            };
            let used = if a3_part { word_count(&text) } else { 0 };
            let (bg, bg_cut) = limit_words(&background, words.saturating_sub(used).max(30));
            (text, if a3_part { sources } else { Vec::new() }, bg, cut || bg_cut)
        } else {
            // Principle 3: if it cannot be traced, it is not shown.
            ("I can't find that in this A3's content.".to_string(), Vec::new(), String::new(), false)
        };
        let shown = a3_part || !background.is_empty();
        let has_more = shown && (cut || !more);

        // The stored form: A3 sources, then the background text, then the pages it came from.
        let mut stored = sources.clone();
        if !background.is_empty() {
            let from_web = opts.knowledge == AskKnowledge::Web && !log.web_sources.is_empty();
            stored.push(CitedSource {
                segment_id: None,
                label: if from_web { "Web search" } else { "General knowledge" }.into(),
                quote: background,
                kind: "background".into(),
                url: None,
            });
            if from_web {
                for w in log.web_sources.iter().take(5) {
                    stored.push(CitedSource { segment_id: None, label: w.title.clone(), quote: String::new(), kind: "web".into(), url: Some(w.url.clone()) });
                }
            }
        }
        self.repo.insert_message(sheet, "user", question, &json!([])).await?;
        self.repo.insert_message(sheet, "assistant", &text, &json!(stored)).await?;
        if let Err(e) = self.roll_up_history(sheet, &mut log).await {
            log.discard("summary", e.message);
        }
        self.finish(Some(sheet), job, Some(&ctx), log, "ok").await;
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
                    fast: false,
                    web: false,
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
        fast: false,
        web: false,
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

/// Whether the user is asking again what they asked before: the same words, ignoring case and punctuation,
/// or nearly so.
fn is_repeat<'a>(question: &str, earlier: impl Iterator<Item = &'a AiMessage>) -> bool {
    fn words(t: &str) -> HashSet<String> {
        t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
    }
    let now = words(question);
    if now.is_empty() {
        return false;
    }
    earlier.filter(|m| m.role == "user").any(|m| {
        let then = words(&m.text);
        let shared = now.intersection(&then).count() as f64;
        let all = now.union(&then).count() as f64;
        all > 0.0 && shared / all >= 0.75
    })
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

fn summary_story_issues(placements: &[SummaryPlacement], ctx: &Context, blocks: &BTreeMap<String, Value>) -> BTreeMap<String, Vec<String>> {
    let mut issues = BTreeMap::new();
    for p in placements {
        let mut problems = Vec::new();
        let has_text = p.blocks.iter().any(|id| !crate::richtext::plain_text(&blocks[id]).trim().is_empty());
        let closed = ctx.step(&p.letter).is_some_and(|s| s.state == "closed");
        if !has_text || closed {
            if !p.story.is_empty() {
                problems.push("Empty or closed sections must have an empty story array.".into());
            }
        } else if p.story.is_empty() {
            problems.push("Write a concise story, not just rearranged details.".into());
        }
        for (i, paragraph) in p.story.iter().enumerate() {
            if paragraph.text.trim().is_empty() || paragraph.sources.is_empty() {
                problems.push(format!("Paragraph {} is empty or has no cited block ids.", i + 1));
                continue;
            }
            let mut seen = HashSet::new();
            if paragraph.sources.iter().any(|id| !p.blocks.contains(id) || !seen.insert(id)) {
                problems.push(format!("Paragraph {} must cite unique block ids placed in this section only.", i + 1));
                continue;
            }
            let input = paragraph.sources.iter().map(|id| crate::richtext::plain_text(&blocks[id])).collect::<Vec<_>>().join("\n");
            let added = guard::unsupported_story_tokens(paragraph.text.trim(), &input);
            if !added.is_empty() {
                problems.push(format!("Paragraph {} contains unsupported numbers, names, codes or units: {}. Remove them or cite the placed block that actually supports them. Equivalent quantity notation is allowed, but do not change the value, approximation, unit or identifier.", i + 1, added.join(", ")));
            }
        }
        if !problems.is_empty() {
            issues.insert(p.letter.clone(), problems);
        }
    }
    issues
}

fn ensure_summary_stories(placements: &[SummaryPlacement], ctx: &Context, blocks: &BTreeMap<String, Value>) -> Result<()> {
    if let Some((letter, problems)) = summary_story_issues(placements, ctx, blocks).into_iter().next() {
        return Err(CoreError::ai(format!("The AI could not correct the story for step {letter}: {} Your A3 was not changed.", problems.join(" "))));
    }
    Ok(())
}

fn summary_story_items(placements: &[SummaryPlacement], blocks: &BTreeMap<String, Value>) -> Vec<(String, String, String)> {
    placements.iter().flat_map(|p| p.story.iter().enumerate().map(|(i, paragraph)| {
        let input = paragraph.sources.iter().map(|id| crate::richtext::plain_text(&blocks[id])).collect::<Vec<_>>().join("\n");
        (format!("story:{}.{i}", p.letter), input, paragraph.text.trim().to_string())
    })).collect()
}

fn summary_rewritten_blocks(details: &BTreeMap<String, Value>, rewrites: &BTreeMap<String, String>) -> Result<BTreeMap<String, Value>> {
    let mut blocks = BTreeMap::new();
    for (letter, doc) in details {
        let prefix = format!("{letter}.");
        let edits = rewrites.iter().filter_map(|(id, text)| id.strip_prefix(&prefix).map(|id| (id.to_string(), text.clone()))).collect();
        let order: Vec<_> = (0..doc["content"].as_array().map_or(0, Vec::len)).collect();
        let rewritten = crate::richtext::rewrite_blocks(doc, &order, &edits).map_err(|e| CoreError::ai(&e))?;
        for (i, block) in rewritten["content"].as_array().into_iter().flatten().enumerate() {
            blocks.insert(format!("{letter}.{i}"), block.clone());
        }
    }
    Ok(blocks)
}

fn summary_batches(items: &[(String, String, String)]) -> Vec<&[(String, String, String)]> {
    let mut batches = Vec::new();
    let mut start = 0;
    let mut size = 0;
    for (i, (id, original, rewrite)) in items.iter().enumerate() {
        let bytes = id.len() + original.len() + rewrite.len();
        if i > start && (i - start >= 8 || size + bytes > 12_000) {
            batches.push(&items[start..i]);
            start = i;
            size = 0;
        }
        size += bytes;
    }
    if start < items.len() {
        batches.push(&items[start..]);
    }
    batches
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

    #[test]
    fn verification_batches_limit_count_and_size_without_dropping_large_nodes() {
        let items: Vec<_> = (0..20).map(|i| (i.to_string(), "a".repeat(3000), "b".repeat(3000))).collect();
        let batches = summary_batches(&items);
        assert_eq!(batches.iter().map(|b| b.len()).sum::<usize>(), items.len());
        assert!(batches.iter().all(|batch| batch.len() <= 8 && batch.iter().map(|(id, a, b)| id.len() + a.len() + b.len()).sum::<usize>() <= 12_000));
        let large = vec![("large".into(), "x".repeat(14_000), "y".repeat(14_000))];
        assert_eq!(summary_batches(&large)[0], large.as_slice(), "one long node stays intact");
        assert!(summary_batches(&[]).is_empty());
    }
}
