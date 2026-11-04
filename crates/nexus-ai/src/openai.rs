//! OpenAI-compatible Chat Completions with tool calling: Gemini's compatibility endpoint, local
//! servers (Ollama, LM Studio, llama.cpp) and other compatible services.

use crate::agent::Remote;
use crate::chat::{Reply, ToolCall, ToolSpec, Turn, Usage};
use crate::provider::{AiError, ProviderConfig, ProviderKind, auth_headers, send_json};
use serde_json::{Value, json};

pub(crate) fn request_body(cfg: &ProviderConfig, model: &str, system: &str, tools: &[ToolSpec], turns: &[Turn]) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": system })];
    for t in turns {
        match t {
            Turn::User(s) => messages.push(json!({ "role": "user", "content": s })),
            Turn::Assistant { raw, .. } => messages.push(raw.clone()),
            Turn::ToolResult { call_id, content, .. } => messages.push(json!({ "role": "tool", "tool_call_id": call_id, "content": content })),
        }
    }
    let mut body = json!({ "model": model, "messages": messages });
    if !tools.is_empty() {
        body["tools"] = tools
            .iter()
            .map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.parameters } }))
            .collect();
        body["tool_choice"] = json!("auto");
    }
    if cfg.kind == ProviderKind::Gemini {
        // Little visible reasoning is needed for tool use; keeps answers fast and cheap.
        body["reasoning_effort"] = json!("low");
    }
    body
}

pub(crate) fn parse_reply(v: &Value) -> Result<Reply, AiError> {
    let choice = v.pointer("/choices/0").ok_or_else(|| AiError::Parse("no choices".into()))?;
    let msg = choice.get("message").cloned().ok_or_else(|| AiError::Parse("no message".into()))?;
    let text = match msg.get("content") {
        Some(Value::String(s)) => s.clone(),
        // Some servers return content parts.
        Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""),
        _ => String::new(),
    };
    let mut raw = msg;
    if raw.get("role").is_none() {
        raw["role"] = json!("assistant");
    }
    let mut calls = Vec::new();
    if let Some(list) = raw.get_mut("tool_calls").and_then(Value::as_array_mut) {
        for (i, c) in list.iter_mut().enumerate() {
            // Local servers sometimes omit call IDs: invent one and keep it in the resent message.
            let id = match c.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                Some(id) => id.to_string(),
                None => {
                    let id = format!("call_{i}");
                    c["id"] = json!(id);
                    id
                }
            };
            let name = c.pointer("/function/name").and_then(Value::as_str).unwrap_or_default().to_string();
            let args = match c.pointer("/function/arguments") {
                Some(Value::String(s)) => serde_json::from_str(s).unwrap_or_else(|_| json!({})),
                Some(v @ Value::Object(_)) => v.clone(),
                _ => json!({}),
            };
            if !name.is_empty() {
                calls.push(ToolCall { id, name, args });
            }
        }
    }
    let usage = Usage {
        input_tokens: v.pointer("/usage/prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
        output_tokens: v.pointer("/usage/completion_tokens").and_then(Value::as_u64).unwrap_or(0),
        cached_tokens: v.pointer("/usage/prompt_tokens_details/cached_tokens").and_then(Value::as_u64).unwrap_or(0),
    };
    let truncated = choice.get("finish_reason").and_then(Value::as_str) == Some("length");
    Ok(Reply { text, calls, raw, usage, truncated })
}

pub(crate) fn complete(remote: &Remote, system: &str, tools: &[ToolSpec], turns: &[Turn]) -> Result<Reply, AiError> {
    let cfg = remote.cfg;
    let body = request_body(cfg, &remote.model, system, tools, turns);
    let url = format!("{}chat/completions", cfg.base_url());
    let v = send_json(remote.http, remote.rt, &url, &auth_headers(cfg), Some(&body), remote.stop)?;
    parse_reply(&v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ToolSpec {
        ToolSpec { name: "commander", description: "Commander state", parameters: json!({ "type": "object", "properties": {} }) }
    }

    #[test]
    fn request_carries_history_and_tools() {
        let cfg = ProviderConfig { kind: ProviderKind::Gemini, api_key: "k".into(), ..Default::default() };
        let turns = vec![
            Turn::User("Where am I?".into()),
            Turn::Assistant {
                text: String::new(),
                calls: vec![],
                raw: json!({ "role": "assistant", "content": null, "tool_calls": [{ "id": "c1", "type": "function", "function": { "name": "commander", "arguments": "{}" }, "extra_content": { "google": { "thought_signature": "sig" } } }] }),
            },
            Turn::ToolResult { call_id: "c1".into(), name: "commander".into(), content: "{\"system\":\"Sol\"}".into(), is_error: false },
        ];
        let b = request_body(&cfg, "gemini-3.5-flash-lite", "be brief", &[spec()], &turns);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][2]["tool_calls"][0]["extra_content"]["google"]["thought_signature"], "sig", "hidden signature resent verbatim");
        assert_eq!(b["messages"][3]["role"], "tool");
        assert_eq!(b["messages"][3]["tool_call_id"], "c1");
        assert_eq!(b["tools"][0]["function"]["name"], "commander");
        assert_eq!(b["reasoning_effort"], "low");
        let local = ProviderConfig { kind: ProviderKind::Local, ..Default::default() };
        assert!(request_body(&local, "m", "s", &[], &[]).get("reasoning_effort").is_none());
    }

    #[test]
    fn parses_tool_calls_text_and_usage() {
        let v = json!({
            "choices": [{ "finish_reason": "tool_calls", "message": { "role": "assistant", "content": null,
                "tool_calls": [{ "id": "", "type": "function", "function": { "name": "find_commodity", "arguments": "{\"commodity\":\"tritium\",\"mode\":\"buy\"}" } }] } }],
            "usage": { "prompt_tokens": 1200, "completion_tokens": 40, "prompt_tokens_details": { "cached_tokens": 1000 } }
        });
        let r = parse_reply(&v).unwrap();
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.calls[0].id, "call_0", "missing id invented");
        assert_eq!(r.raw["tool_calls"][0]["id"], "call_0", "and written back for the resend");
        assert_eq!(r.calls[0].args["commodity"], "tritium");
        assert_eq!(r.usage, Usage { input_tokens: 1200, output_tokens: 40, cached_tokens: 1000 });
        let v = json!({ "choices": [{ "finish_reason": "stop", "message": { "content": "Sell at Jameson Memorial." } }] });
        let r = parse_reply(&v).unwrap();
        assert_eq!(r.text, "Sell at Jameson Memorial.");
        assert!(r.calls.is_empty() && r.raw["role"] == "assistant");
    }
}
