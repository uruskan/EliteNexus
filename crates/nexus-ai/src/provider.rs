//! Which model service answers, plus the HTTP plumbing shared by the protocol clients.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ProviderKind {
    /// Google Gemini through its OpenAI-compatible endpoint (has a free tier).
    #[default]
    Gemini,
    /// Anthropic Claude (native Messages API, prompt caching).
    Claude,
    /// A model running on this PC (Ollama, LM Studio, llama.cpp server): free, no key.
    Local,
    /// Any other OpenAI-compatible service (OpenRouter, DeepSeek, Groq, …).
    Custom,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 4] = [ProviderKind::Gemini, ProviderKind::Claude, ProviderKind::Local, ProviderKind::Custom];

    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Gemini => "Google Gemini",
            ProviderKind::Claude => "Anthropic Claude",
            ProviderKind::Local => "Local model (Ollama, LM Studio)",
            ProviderKind::Custom => "Other OpenAI-compatible service",
        }
    }

    pub fn needs_key(self) -> bool {
        !matches!(self, ProviderKind::Local)
    }

    /// The commander sets this provider's address; Gemini and Claude always use their own.
    pub fn takes_address(self) -> bool {
        matches!(self, ProviderKind::Local | ProviderKind::Custom)
    }

    /// Where users get an API key.
    pub fn key_url(self) -> Option<&'static str> {
        match self {
            ProviderKind::Gemini => Some("https://aistudio.google.com/apikey"),
            ProviderKind::Claude => Some("https://platform.claude.com/settings/keys"),
            _ => None,
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            ProviderKind::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai/",
            ProviderKind::Claude => "https://api.anthropic.com/v1/",
            ProviderKind::Local => "http://localhost:11434/v1/",
            ProviderKind::Custom => "",
        }
    }

    /// Models tried in order when none is configured: cheap ones with good tool calling first.
    pub fn preferred_models(self) -> &'static [&'static str] {
        match self {
            ProviderKind::Gemini => &["gemini-3.5-flash-lite", "gemini-3.1-flash-lite", "gemini-2.5-flash-lite", "gemini-3.8-flash", "gemini-2.5-flash"],
            ProviderKind::Claude => &["claude-haiku-5-5", "claude-sonnet-5-5"],
            ProviderKind::Local | ProviderKind::Custom => &[],
        }
    }
}

/// The user's model service. `api_key` is plain text in memory only (the app stores it sealed).
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub api_key: String,
    /// Empty = choose automatically.
    pub model: String,
    /// Empty = the provider's default address. Only used where [`ProviderKind::takes_address`].
    pub base_url: String,
}

impl fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("kind", &self.kind)
            .field("api_key", &if self.api_key.is_empty() { "<empty>" } else { "<redacted>" })
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl ProviderConfig {
    /// Service address ending in `/`. Gemini and Claude ignore a stored address: one left over from a custom or
    /// local service would send their requests, and their key, there.
    pub fn base_url(&self) -> String {
        let u = self.base_url.trim();
        let u = if u.is_empty() || !self.kind.takes_address() { self.kind.default_base_url() } else { u };
        if u.is_empty() || u.ends_with('/') { u.to_string() } else { format!("{u}/") }
    }

    /// Enough is configured to try a request.
    pub fn is_ready(&self) -> bool {
        (!self.kind.needs_key() || !self.api_key.trim().is_empty()) && !self.base_url().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AiError {
    #[error("network error: {0}")]
    Http(String),
    #[error("{message} (HTTP {code})")]
    Status { code: u16, message: String },
    #[error("unexpected answer from the model service: {0}")]
    Parse(String),
    #[error("no model available")]
    NoModel,
    /// The commander stopped the question.
    #[error("stopped")]
    Stopped,
}

impl AiError {
    /// Bad or missing API key.
    pub fn is_auth(&self) -> bool {
        matches!(self, AiError::Status { code: 400 | 401 | 403, message } if message.to_lowercase().contains("key") || message.to_lowercase().contains("auth") || message.to_lowercase().contains("permission"))
            || matches!(self, AiError::Status { code: 401 | 403, .. })
    }
}

/// One JSON request, retried after rate limits and server errors (at most twice). Setting `stop` ends it at once,
/// waits included, with [`AiError::Stopped`].
pub(crate) fn send_json(http: &reqwest::Client, rt: &Handle, url: &str, headers: &[(&str, String)], body: Option<&Value>, stop: Option<&AtomicBool>) -> Result<Value, AiError> {
    for attempt in 0..3u32 {
        if is_set(stop) {
            return Err(AiError::Stopped);
        }
        let mut req = match body {
            Some(b) => http.post(url).json(b),
            None => http.get(url),
        };
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        let request = async move {
            let r = req.timeout(Duration::from_secs(120)).send().await?;
            let status = r.status().as_u16();
            let retry_after = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse::<u64>().ok());
            let text = r.text().await?;
            Ok::<_, reqwest::Error>((status, retry_after, text))
        };
        // Losing the race to `stop` drops the request, which cancels it.
        let res = rt.block_on(async move {
            tokio::select! {
                r = request => Some(r),
                () = until_set(stop) => None,
            }
        });
        let Some(res) = res else { return Err(AiError::Stopped) };
        match res {
            Ok((status, _, text)) if (200..300).contains(&status) => {
                return serde_json::from_str(&text).map_err(|e| AiError::Parse(e.to_string()));
            }
            Ok((status, retry_after, text)) => {
                let retryable = status >= 500 || (status == 429 && retry_after.is_some());
                if retryable && attempt < 2 {
                    pause(Duration::from_secs(retry_after.unwrap_or(2 << attempt).min(20)), stop);
                    continue;
                }
                return Err(AiError::Status { code: status, message: error_message(&text) });
            }
            Err(e) if attempt < 2 && (e.is_timeout() || e.is_connect()) => {
                pause(Duration::from_secs(1 + u64::from(attempt)), stop);
            }
            Err(e) => return Err(AiError::Http(e.to_string())),
        }
    }
    Err(AiError::Http("gave up after retries".into()))
}

fn is_set(stop: Option<&AtomicBool>) -> bool {
    stop.is_some_and(|s| s.load(Ordering::Relaxed))
}

/// Resolves once `stop` is set; never without a flag.
async fn until_set(stop: Option<&AtomicBool>) {
    match stop {
        Some(s) => {
            while !s.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        None => std::future::pending().await,
    }
}

/// Waits `d` before a retry, or less once `stop` is set.
fn pause(d: Duration, stop: Option<&AtomicBool>) {
    let end = Instant::now() + d;
    while !is_set(stop) {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        std::thread::sleep(left.min(Duration::from_millis(100)));
    }
}

/// The human-readable part of an error body (OpenAI, Gemini and Anthropic shapes).
fn error_message(body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let v = if let Value::Array(a) = &v { a.first().cloned().unwrap_or(Value::Null) } else { v };
    v.pointer("/error/message")
        .or_else(|| v.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| body.chars().take(300).collect())
}

/// The provider's model IDs (`models/` prefixes removed).
pub fn list_models(http: &reqwest::Client, rt: &Handle, cfg: &ProviderConfig) -> Result<Vec<String>, AiError> {
    let url = format!("{}models", cfg.base_url());
    let v = send_json(http, rt, &url, &auth_headers(cfg), None, None)?;
    let ids = v
        .get("data")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|m| m.get("id").and_then(Value::as_str)).map(|s| s.trim_start_matches("models/").to_string()).collect())
        .unwrap_or_default();
    Ok(ids)
}

/// The configured model, else the first preferred model the provider offers, else its first model.
pub fn resolve_model(cfg: &ProviderConfig, available: &[String]) -> Option<String> {
    if !cfg.model.trim().is_empty() {
        return Some(cfg.model.trim().to_string());
    }
    let preferred = cfg.kind.preferred_models();
    preferred
        .iter()
        .find(|p| available.iter().any(|a| a == *p))
        .map(|s| s.to_string())
        .or_else(|| if available.is_empty() { preferred.first().map(|s| s.to_string()) } else { None })
        .or_else(|| available.first().cloned())
}

pub(crate) fn auth_headers(cfg: &ProviderConfig) -> Vec<(&'static str, String)> {
    let key = cfg.api_key.trim();
    match cfg.kind {
        ProviderKind::Claude => vec![("x-api-key", key.to_string()), ("anthropic-version", "2023-06-01".to_string())],
        _ if key.is_empty() => Vec::new(),
        _ => vec![("authorization", format!("Bearer {key}"))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_resolution() {
        let gemini = ProviderConfig { kind: ProviderKind::Gemini, ..Default::default() };
        let listed = vec!["gemini-2.5-flash".to_string(), "gemini-2.5-flash-lite".to_string()];
        assert_eq!(resolve_model(&gemini, &listed).as_deref(), Some("gemini-2.5-flash-lite"), "cheapest preferred one that exists");
        assert_eq!(resolve_model(&gemini, &[]).as_deref(), Some("gemini-3.5-flash-lite"), "no list: first preference");
        let local = ProviderConfig { kind: ProviderKind::Local, ..Default::default() };
        assert_eq!(resolve_model(&local, &["qwen3:8b".to_string()]).as_deref(), Some("qwen3:8b"));
        let chosen = ProviderConfig { model: "my-model".into(), ..gemini };
        assert_eq!(resolve_model(&chosen, &listed).as_deref(), Some("my-model"));
    }

    #[test]
    fn urls_keys_and_errors() {
        let c = ProviderConfig { kind: ProviderKind::Custom, base_url: "https://example.org/v1".into(), ..Default::default() };
        assert_eq!(c.base_url(), "https://example.org/v1/");
        assert!(!c.is_ready(), "custom services need a key");
        assert!(ProviderConfig { kind: ProviderKind::Local, ..Default::default() }.is_ready());
        let k = ProviderConfig { api_key: "secret-123".into(), ..Default::default() };
        assert!(!format!("{k:?}").contains("secret-123"));
        assert_eq!(auth_headers(&k)[0].1, "Bearer secret-123");
        assert_eq!(error_message(r#"[{"error":{"code":400,"message":"API key not valid."}}]"#), "API key not valid.");
        assert_eq!(error_message(r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#), "slow down");
        assert!(AiError::Status { code: 400, message: "API key not valid. Please pass a valid API key.".into() }.is_auth());
    }

    #[test]
    fn gemini_and_claude_ignore_a_left_over_address() {
        // Switching from a custom service back to Gemini used to keep sending Gemini's key to the custom address.
        for kind in [ProviderKind::Gemini, ProviderKind::Claude] {
            let c = ProviderConfig { kind, base_url: "https://agentrouter.org/v1".into(), ..Default::default() };
            assert_eq!(c.base_url(), kind.default_base_url(), "{kind:?}");
        }
        let local = ProviderConfig { kind: ProviderKind::Local, base_url: "http://localhost:1234/v1".into(), ..Default::default() };
        assert_eq!(local.base_url(), "http://localhost:1234/v1/", "LM Studio's port is kept");
    }

    #[test]
    fn stop_ends_a_request_that_hangs() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        // A server that takes the connection and never answers.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let _held = listener.accept();
            std::thread::sleep(Duration::from_secs(30));
        });
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let http = reqwest::Client::new();
        assert_eq!(send_json(&http, rt.handle(), &url, &[], None, Some(&stop)), Err(AiError::Stopped));
        assert!(started.elapsed() < Duration::from_secs(5), "stopped after {:?}", started.elapsed());
        assert_eq!(send_json(&http, rt.handle(), &url, &[], None, Some(&stop)), Err(AiError::Stopped), "a set flag sends nothing");
    }
}
