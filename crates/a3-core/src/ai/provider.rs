//! `AiProvider` is the boundary around model vendors (section 5), with one adapter per provider.

use crate::error::{CoreError, Result};
use crate::extract::ImageData;
use async_trait::async_trait;
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    /// `anthropic`, `openai`, `google` or `custom`
    pub provider: String,
    #[serde(default)]
    pub base_url: Option<String>,
    pub model: String,
    /// From the last Test connection.
    #[serde(default)]
    pub vision_ok: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CompletionRequest {
    pub system: String,
    pub user: String,
    pub images: Vec<ImageData>,
    pub schema: Option<Value>,
    pub max_tokens: u32,
    /// Bounded rewriting/checking: avoid expensive thinking where explicitly supported.
    pub fast: bool,
    /// Let the model search the web for this request. Only providers whose `supports_web` is true honour it.
    pub web: bool,
}

/// A page the provider's own search tool returned. These come from the provider, never from model text,
/// so a link shown to the user is one that was really retrieved.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WebSource {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Default)]
pub struct Completion {
    pub text: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub web_sources: Vec<WebSource>,
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    fn name(&self) -> &str;
    fn model(&self) -> &str;
    /// Whether this provider has a built-in web search tool the app can switch on.
    fn supports_web(&self) -> bool {
        false
    }
    async fn complete(&self, req: &CompletionRequest) -> Result<Completion>;
    async fn list_models(&self) -> Result<Vec<String>>;
}

pub fn make_provider(config: &AiConfig, key: &str) -> Result<Box<dyn AiProvider>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| CoreError::ai(format!("Could not start the network client: {e}")))?;
    let base = config.base_url.as_deref().map(str::trim).filter(|b| !b.is_empty()).map(|b| b.trim_end_matches('/').to_string());
    if let Some(b) = &base {
        check_base_url(b)?;
    }
    let shared = Shared { client, key: key.to_string(), model: config.model.clone(), base };
    Ok(match config.provider.as_str() {
        "anthropic" => Box::new(Anthropic(shared)),
        "openai" => Box::new(OpenAi { s: shared, custom: false }),
        "custom" => {
            if shared.base.is_none() {
                return Err(CoreError::invalid("Enter the address of the OpenAI-compatible service."));
            }
            Box::new(OpenAi { s: shared, custom: true })
        }
        "google" => Box::new(Google(shared)),
        other => return Err(CoreError::invalid(format!("Unknown AI provider: {other}"))),
    })
}

fn check_base_url(b: &str) -> Result<()> {
    let lower = b.to_lowercase();
    let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"].iter().any(|p| lower.starts_with(p));
    if lower.starts_with("https://") || local {
        Ok(())
    } else {
        Err(CoreError::invalid("The AI address must use https://, because A3 text and your key travel over it."))
    }
}

struct Shared {
    client: Client,
    key: String,
    model: String,
    base: Option<String>,
}

/// Sends with one retry on rate limits and server errors, then maps failures to one plain message.
async fn send(build: impl Fn() -> RequestBuilder) -> Result<Response> {
    let mut last: Option<Response> = None;
    for attempt in 0..2 {
        let resp = build().send().await?;
        let s = resp.status();
        if s.is_success() {
            return Ok(resp);
        }
        let retry = s == StatusCode::TOO_MANY_REQUESTS || s.is_server_error();
        last = Some(resp);
        if !retry || attempt == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    let resp = last.expect("the loop runs at least once");
    Err(http_error(resp).await)
}

async fn http_error(resp: Response) -> CoreError {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            v["error"]["message"].as_str().map(str::to_string).or_else(|| v["message"].as_str().map(str::to_string))
        })
        .unwrap_or_default();
    let code = if status == StatusCode::BAD_REQUEST { "AI_BAD_REQUEST" } else { "AI_FAILED" };
    let message = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "The AI provider rejected the API key.".to_string(),
        StatusCode::NOT_FOUND => "The AI provider does not recognise this model or address.".to_string(),
        StatusCode::TOO_MANY_REQUESTS => "The AI provider is rate limiting this key. Try again shortly.".to_string(),
        s if s.is_server_error() => "The AI provider is having a problem. Try again shortly.".to_string(),
        _ if !detail.is_empty() => format!("The AI provider refused the request: {detail}"),
        s => format!("The AI provider answered with status {s}."),
    };
    CoreError::new(code, message)
}

// ------------------------------------------------------------------ Anthropic

struct Anthropic(Shared);

impl Anthropic {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.0.base.as_deref().unwrap_or("https://api.anthropic.com"))
    }
    fn auth(&self, b: RequestBuilder) -> RequestBuilder {
        b.header("x-api-key", &self.0.key).header("anthropic-version", "2026-10-21")
    }
    /// `resumed` holds the assistant turn of a search the server paused, so the same request can carry on.
    async fn call(&self, req: &CompletionRequest, use_format: bool, resumed: &[Value]) -> Result<Value> {
        let mut content: Vec<Value> = req
            .images
            .iter()
            .map(|i| json!({"type": "image", "source": {"type": "base64", "media_type": i.mime, "data": i.base64}}))
            .collect();
        content.push(json!({"type": "text", "text": req.user}));
        let mut messages = vec![json!({"role": "user", "content": content})];
        messages.extend(resumed.iter().cloned());
        let mut body = json!({
            "model": self.0.model,
            "max_tokens": req.max_tokens,
            "system": req.system,
            "messages": messages,
        });
        if let (true, Some(schema)) = (use_format, &req.schema) {
            body["output_config"] = json!({"format": {"type": "json_schema", "schema": schema}});
        }
        if req.web {
            // The basic variant works on every model the app can be pointed at.
            body["tools"] = json!([{"type": "web_search_20250305", "name": "web_search", "max_uses": 3}]);
        }
        let resp = send(|| self.auth(self.0.client.post(self.url("/v1/messages"))).json(&body)).await?;
        Ok(resp.json().await?)
    }
}

/// Pages the search tool returned. A failed search comes back as an error object instead of a list, and is skipped.
fn web_sources_in(reply: &Value, into: &mut Vec<WebSource>) {
    for block in reply["content"].as_array().into_iter().flatten() {
        if block["type"] != "web_search_tool_result" {
            continue;
        }
        for hit in block["content"].as_array().into_iter().flatten() {
            let (Some(url), title) = (hit["url"].as_str(), hit["title"].as_str().unwrap_or("")) else { continue };
            let safe = url.starts_with("https://") || url.starts_with("http://");
            if safe && !into.iter().any(|s| s.url == url) {
                into.push(WebSource { title: if title.is_empty() { url.to_string() } else { title.to_string() }, url: url.to_string() });
            }
        }
    }
}

#[async_trait]
impl AiProvider for Anthropic {
    fn name(&self) -> &str {
        "anthropic"
    }
    fn model(&self) -> &str {
        &self.0.model
    }
    fn supports_web(&self) -> bool {
        true
    }
    async fn complete(&self, req: &CompletionRequest) -> Result<Completion> {
        let mut out = Completion::default();
        let mut resumed: Vec<Value> = Vec::new();
        // A long web search can pause the turn. Resume it a few times, then give up.
        for _ in 0..4 {
            // Older models lack structured outputs. Falling back is safe, because G2 validates every reply.
            let v = match self.call(req, true, &resumed).await {
                Err(e) if e.code == "AI_BAD_REQUEST" && req.schema.is_some() => self.call(&with_schema_in_prompt(req), false, &resumed).await?,
                other => other?,
            };
            match v["stop_reason"].as_str() {
                Some("refusal") => return Err(CoreError::ai("The AI model declined this request.")),
                Some("max_tokens") => return Err(CoreError::new("AI_TRUNCATED", "The AI reply was too long and got cut off.")),
                _ => {}
            }
            out.input_tokens += v["usage"]["input_tokens"].as_i64().unwrap_or(0) as i32;
            out.output_tokens += v["usage"]["output_tokens"].as_i64().unwrap_or(0) as i32;
            web_sources_in(&v, &mut out.web_sources);
            out.text = v["content"]
                .as_array()
                .map(|blocks| {
                    blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("")
                })
                .unwrap_or_default();
            if v["stop_reason"].as_str() == Some("pause_turn") && req.web {
                resumed = vec![json!({"role": "assistant", "content": v["content"]})];
                continue;
            }
            return Ok(out);
        }
        Err(CoreError::ai("The web search took too long. Try again, or ask without the web."))
    }
    async fn list_models(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..10 {
            let mut url = format!("{}?limit=100", self.url("/v1/models"));
            if let Some(a) = &after {
                url.push_str(&format!("&after_id={a}"));
            }
            let resp = send(|| self.auth(self.0.client.get(&url))).await?;
            let v: Value = resp.json().await?;
            if let Some(data) = v["data"].as_array() {
                out.extend(data.iter().filter_map(|m| m["id"].as_str().map(str::to_string)));
            }
            if v["has_more"].as_bool() != Some(true) {
                break;
            }
            after = v["last_id"].as_str().map(str::to_string);
        }
        Ok(out)
    }
}

// ------------------------------------------------------------------ OpenAI and compatible services

struct OpenAi {
    s: Shared,
    custom: bool,
}

impl OpenAi {
    fn base(&self) -> &str {
        self.s.base.as_deref().unwrap_or("https://api.openai.com/v1")
    }
    fn auth(&self, b: RequestBuilder) -> RequestBuilder {
        if self.s.key.is_empty() {
            b
        } else {
            b.bearer_auth(&self.s.key)
        }
    }
    fn body(&self, req: &CompletionRequest, format: Option<Value>) -> Value {
        let mut parts: Vec<Value> = req
            .images
            .iter()
            .map(|i| json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", i.mime, i.base64)}}))
            .collect();
        parts.push(json!({"type": "text", "text": req.user}));
        let mut body = json!({
            "model": self.s.model,
            "messages": [{"role": "system", "content": req.system}, {"role": "user", "content": parts}],
        });
        // OpenAI's newer models take max_completion_tokens. Compatible services mostly take max_tokens.
        body[if self.custom { "max_tokens" } else { "max_completion_tokens" }] = json!(req.max_tokens);
        let deepseek = reqwest::Url::parse(self.base()).ok()
            .is_some_and(|url| url.host_str() == Some("api.deepseek.com"));
        if req.fast && self.custom && deepseek && matches!(self.s.model.as_str(), "deepseek-v4-pro" | "deepseek-flash") {
            body["thinking"] = json!({"type": "disabled"});
        }
        if let Some(f) = format {
            body["response_format"] = f;
        }
        body
    }
    async fn call(&self, req: &CompletionRequest, format: Option<Value>) -> Result<Value> {
        let body = self.body(req, format);
        let url = format!("{}/chat/completions", self.base());
        let resp = send(|| self.auth(self.s.client.post(&url)).json(&body)).await?;
        Ok(resp.json().await?)
    }
}

#[async_trait]
impl AiProvider for OpenAi {
    fn name(&self) -> &str {
        if self.custom { "custom" } else { "openai" }
    }
    fn model(&self) -> &str {
        &self.s.model
    }
    async fn complete(&self, req: &CompletionRequest) -> Result<Completion> {
        let strict = req
            .schema
            .as_ref()
            .map(|s| json!({"type": "json_schema", "json_schema": {"name": "output", "strict": true, "schema": s}}));
        let v = match self.call(req, strict).await {
            Err(e) if e.code == "AI_BAD_REQUEST" && req.schema.is_some() => {
                let prompted = with_schema_in_prompt(req);
                match self.call(&prompted, Some(json!({"type": "json_object"}))).await {
                    Err(e) if e.code == "AI_BAD_REQUEST" => self.call(&prompted, None).await?,
                    other => other?,
                }
            }
            other => other?,
        };
        let choice = &v["choices"][0];
        match choice["finish_reason"].as_str() {
            Some("length") => return Err(CoreError::new("AI_TRUNCATED", "The AI reply was too long and got cut off.")),
            Some("content_filter") => return Err(CoreError::ai("The AI model declined this request.")),
            _ => {}
        }
        if let Some(r) = choice["message"]["refusal"].as_str() {
            if !r.is_empty() {
                return Err(CoreError::ai("The AI model declined this request."));
            }
        }
        Ok(Completion {
            text: choice["message"]["content"].as_str().unwrap_or("").to_string(),
            input_tokens: v["usage"]["prompt_tokens"].as_i64().unwrap_or(0) as i32,
            output_tokens: v["usage"]["completion_tokens"].as_i64().unwrap_or(0) as i32,
            ..Default::default()
        })
    }
    async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/models", self.base());
        let resp = send(|| self.auth(self.s.client.get(&url))).await?;
        let v: Value = resp.json().await?;
        Ok(v["data"].as_array().map(|d| d.iter().filter_map(|m| m["id"].as_str().map(str::to_string)).collect()).unwrap_or_default())
    }
}

// ------------------------------------------------------------------ Google

struct Google(Shared);

impl Google {
    fn base(&self) -> &str {
        self.0.base.as_deref().unwrap_or("https://generativelanguage.googleapis.com")
    }
}

#[async_trait]
impl AiProvider for Google {
    fn name(&self) -> &str {
        "google"
    }
    fn model(&self) -> &str {
        &self.0.model
    }
    async fn complete(&self, req: &CompletionRequest) -> Result<Completion> {
        // Google's response schema accepts only a subset of JSON Schema, so the schema goes in the
        // prompt and JSON mode forces the format. G2 still validates the reply.
        let prompted = with_schema_in_prompt(req);
        let mut parts: Vec<Value> =
            req.images.iter().map(|i| json!({"inline_data": {"mime_type": i.mime, "data": i.base64}})).collect();
        parts.push(json!({"text": prompted.user}));
        let mut config = json!({"maxOutputTokens": req.max_tokens});
        if req.schema.is_some() {
            config["responseMimeType"] = json!("application/json");
        }
        let body = json!({
            "systemInstruction": {"parts": [{"text": prompted.system}]},
            "contents": [{"role": "user", "parts": parts}],
            "generationConfig": config,
        });
        let url = format!("{}/v1beta/models/{}:generateContent", self.base(), self.0.model);
        let resp = send(|| self.0.client.post(&url).header("x-goog-api-key", &self.0.key).json(&body)).await?;
        let v: Value = resp.json().await?;
        let cand = &v["candidates"][0];
        match cand["finishReason"].as_str() {
            Some("MAX_TOKENS") => return Err(CoreError::new("AI_TRUNCATED", "The AI reply was too long and got cut off.")),
            Some("SAFETY") | Some("RECITATION") | Some("PROHIBITED_CONTENT") => {
                return Err(CoreError::ai("The AI model declined this request."))
            }
            _ => {}
        }
        let text = cand["content"]["parts"]
            .as_array()
            .map(|p| p.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join(""))
            .unwrap_or_default();
        Ok(Completion {
            text,
            input_tokens: v["usageMetadata"]["promptTokenCount"].as_i64().unwrap_or(0) as i32,
            output_tokens: v["usageMetadata"]["candidatesTokenCount"].as_i64().unwrap_or(0) as i32,
            ..Default::default()
        })
    }
    async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/v1beta/models?pageSize=200", self.base());
        let resp = send(|| self.0.client.get(&url).header("x-goog-api-key", &self.0.key)).await?;
        let v: Value = resp.json().await?;
        Ok(v["models"]
            .as_array()
            .map(|models| {
                models
                    .iter()
                    .filter(|m| {
                        m["supportedGenerationMethods"].as_array().is_some_and(|a| a.iter().any(|x| x == "generateContent"))
                    })
                    .filter_map(|m| m["name"].as_str().map(|n| n.trim_start_matches("models/").to_string()))
                    .collect()
            })
            .unwrap_or_default())
    }
}

fn with_schema_in_prompt(req: &CompletionRequest) -> CompletionRequest {
    let mut r = req.clone();
    if let Some(schema) = &req.schema {
        r.system.push_str(&format!(
            "\n\nReply with one JSON object only, no other text, matching this JSON Schema:\n{schema}"
        ));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_is_refused_except_locally() {
        assert!(check_base_url("https://llm.example.com/v1").is_ok());
        assert!(check_base_url("http://localhost:8080/v1").is_ok());
        assert!(check_base_url("http://llm.example.com/v1").is_err());
    }

    #[test]
    fn web_sources_come_only_from_search_results() {
        let reply = json!({"content": [
            {"type": "text", "text": "{\"answer\":\"\"}"},
            {"type": "web_search_tool_result", "content": [
                {"type": "web_search_result", "url": "https://example.com/a", "title": "A page"},
                {"type": "web_search_result", "url": "https://example.com/a", "title": "Duplicate"},
                {"type": "web_search_result", "url": "javascript:alert(1)", "title": "Not a web page"},
                {"type": "web_search_result", "url": "https://example.com/b", "title": ""}
            ]},
            {"type": "web_search_tool_result", "content": {"type": "web_search_tool_result_error", "error_code": "max_uses_exceeded"}}
        ]});
        let mut got = Vec::new();
        web_sources_in(&reply, &mut got);
        assert_eq!(got.len(), 2, "duplicates, non-web links and failed searches are skipped");
        assert_eq!(got[0].title, "A page");
        assert_eq!(got[1].title, "https://example.com/b", "a missing title falls back to the address");
    }

    #[test]
    fn only_anthropic_offers_web_search() {
        for (provider, expected) in [("anthropic", true), ("openai", false), ("google", false)] {
            let cfg = AiConfig { provider: provider.into(), model: "m".into(), ..Default::default() };
            assert_eq!(make_provider(&cfg, "k").unwrap().supports_web(), expected, "{provider}");
        }
    }

    #[test]
    fn unknown_provider_is_refused() {
        let cfg = AiConfig { provider: "nope".into(), model: "m".into(), ..Default::default() };
        assert!(make_provider(&cfg, "k").is_err());
    }

    #[test]
    fn custom_needs_an_address() {
        let cfg = AiConfig { provider: "custom".into(), model: "m".into(), ..Default::default() };
        assert!(make_provider(&cfg, "k").is_err());
    }

    #[test]
    fn fast_editing_only_disables_thinking_on_supported_official_deepseek_requests() {
        for (base, model, fast, expected) in [
            ("https://api.deepseek.com/v1", "deepseek-v4-pro", true, true),
            ("https://api.deepseek.com", "deepseek-flash", true, true),
            ("https://api.deepseek.com/v1", "deepseek-v4-pro", false, false),
            ("https://proxy.example.com/v1", "deepseek-v4-pro", true, false),
            ("https://api.deepseek.com.example.com/v1", "deepseek-v4-pro", true, false),
            ("https://api.deepseek.com/v1", "another-model", true, false),
        ] {
            let provider = OpenAi { s: Shared { client: Client::new(), key: String::new(), model: model.into(), base: Some(base.into()) }, custom: true };
            let body = provider.body(&CompletionRequest { fast, max_tokens: 4096, ..Default::default() }, None);
            assert_eq!(body.get("thinking").is_some(), expected, "{base} {model} fast={fast}");
            if expected { assert_eq!(body["thinking"]["type"], "disabled"); }
            assert_eq!(body["max_tokens"], 4096);
        }
    }
}
