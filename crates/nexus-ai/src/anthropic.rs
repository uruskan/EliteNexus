//! Anthropic Messages API with tool use. The system prompt and tool list are marked for prompt
//! caching, so follow-up requests in a conversation re-read them at a fraction of the price.

use crate::agent::Remote;
use crate::chat::{Reply, ToolCall, ToolSpec, Turn, Usage};
use crate::provider::{AiError, auth_headers, send_json};
use serde_json::{Value, json};

const MAX_TOKENS: u32 = 2048;

pub(crate) fn request_body(model: &str, system: &str, tools: &[ToolSpec], turns: &[Turn]) -> Value {
    let mut tool_list: Vec<Value> = tools
        .iter()
        .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.parameters }))
        .collect();
    if let Some(last) = tool_list.last_mut() {
        last["cache_control"] = json!({ "type": "ephemeral" });
    }
    let mut messages: Vec<Value> = Vec::new();
    for t in turns {
        match t {
            Turn::User(s) => messages.push(json!({ "role": "user", "content": s })),
            Turn::Assistant { raw, .. } => messages.push(json!({ "role": "assistant", "content": raw })),
            Turn::ToolResult { call_id, content, is_error, .. } => {
                let block = json!({ "type": "tool_result", "tool_use_id": call_id, "content": content, "is_error": is_error });
                // All results of one assistant turn go into a single user message.
                match messages.last_mut() {
                    Some(m) if m["role"] == "user" && m["content"].as_array().is_some_and(|a| a.iter().all(|b| b["type"] == "tool_result")) => {
                        if let Some(a) = m["content"].as_array_mut() {
                            a.push(block);
                        }
                    }
                    _ => messages.push(json!({ "role": "user", "content": [block] })),
                }
            }
        }
    }
    let mut body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "system": [{ "type": "text", "text": system, "cache_control": { "type": "ephemeral" } }],
        "messages": messages,
    });
    if !tool_list.is_empty() {
        body["tools"] = Value::Array(tool_list);
    }
    body
}

pub(crate) fn parse_reply(v: &Value) -> Result<Reply, AiError> {
    let content = v.get("content").cloned().ok_or_else(|| AiError::Parse("no content".into()))?;
    let blocks = content.as_array().cloned().unwrap_or_default();
    let mut text = blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("");
    let calls = blocks
        .iter()
        .filter(|b| b["type"] == "tool_use")
        .map(|b| ToolCall {
            id: b["id"].as_str().unwrap_or_default().to_string(),
            name: b["name"].as_str().unwrap_or_default().to_string(),
            args: b.get("input").cloned().unwrap_or_else(|| json!({})),
        })
        .collect();
    let stop = v.get("stop_reason").and_then(Value::as_str).unwrap_or_default();
    if stop == "refusal" && text.is_empty() {
        text = v.pointer("/stop_details/explanation").and_then(Value::as_str).unwrap_or("The model declined to answer.").to_string();
    }
    let u = |k: &str| v.pointer(&format!("/usage/{k}")).and_then(Value::as_u64).unwrap_or(0);
    let cached = u("cache_read_input_tokens");
    let usage = Usage { input_tokens: u("input_tokens") + cached + u("cache_creation_input_tokens"), output_tokens: u("output_tokens"), cached_tokens: cached };
    let truncated = matches!(stop, "max_tokens" | "model_context_window_exceeded");
    Ok(Reply { text, calls, raw: content, usage, truncated })
}

pub(crate) fn complete(remote: &Remote, system: &str, tools: &[ToolSpec], turns: &[Turn]) -> Result<Reply, AiError> {
    let body = request_body(&remote.model, system, tools, turns);
    let url = format!("{}messages", remote.cfg.base_url());
    let v = send_json(remote.http, remote.rt, &url, &auth_headers(remote.cfg), Some(&body), remote.stop)?;
    parse_reply(&v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caches_prefix_and_groups_tool_results() {
        let tools = vec![
            ToolSpec { name: "a", description: "A", parameters: json!({ "type": "object", "properties": {} }) },
            ToolSpec { name: "b", description: "B", parameters: json!({ "type": "object", "properties": {} }) },
        ];
        let raw = json!([{ "type": "thinking", "thinking": "", "signature": "s1" }, { "type": "tool_use", "id": "t1", "name": "a", "input": {} }, { "type": "tool_use", "id": "t2", "name": "b", "input": {} }]);
        let turns = vec![
            Turn::User("hi".into()),
            Turn::Assistant { text: String::new(), calls: vec![], raw },
            Turn::ToolResult { call_id: "t1".into(), name: "a".into(), content: "1".into(), is_error: false },
            Turn::ToolResult { call_id: "t2".into(), name: "b".into(), content: "boom".into(), is_error: true },
        ];
        let b = request_body("claude-haiku-5-5", "sys", &tools, &turns);
        assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(b["tools"][0].get("cache_control").is_none() && b["tools"][1]["cache_control"]["type"] == "ephemeral", "breakpoint on the last tool");
        assert_eq!(b["messages"][1]["content"][0]["signature"], "s1", "thinking block resent unchanged");
        assert_eq!(b["messages"].as_array().unwrap().len(), 3, "both results in one user message");
        assert_eq!(b["messages"][2]["content"][1]["is_error"], true);
        assert!(b.get("temperature").is_none() && b.get("top_p").is_none());
    }

    #[test]
    fn parses_tool_use_usage_and_refusals() {
        let v = json!({ "content": [{ "type": "text", "text": "Checking." }, { "type": "tool_use", "id": "toolu_1", "name": "commander", "input": { "x": 1 } }],
                        "stop_reason": "tool_use", "usage": { "input_tokens": 50, "cache_read_input_tokens": 3000, "cache_creation_input_tokens": 0, "output_tokens": 20 } });
        let r = parse_reply(&v).unwrap();
        assert_eq!(r.text, "Checking.");
        assert_eq!(r.calls[0].name, "commander");
        assert_eq!(r.calls[0].args["x"], 1);
        assert_eq!(r.usage, Usage { input_tokens: 3050, output_tokens: 20, cached_tokens: 3000 });
        let v = json!({ "content": [], "stop_reason": "refusal", "stop_details": { "explanation": "Not allowed." } });
        assert_eq!(parse_reply(&v).unwrap().text, "Not allowed.");
    }
}
