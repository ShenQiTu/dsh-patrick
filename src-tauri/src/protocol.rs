use crate::config::{validate_model, Model};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};

pub fn endpoint(protocol: &str) -> &'static str {
    match protocol {
        "anthropic" => "messages",
        "responses" => "responses",
        _ => "chat/completions",
    }
}

pub fn upstream_url(model: &Model) -> Result<String, String> {
    validate_model(model)?;
    if model.use_full_url {
        return Ok(model.base_url.clone());
    }
    let base = model.base_url.trim_end_matches('/');
    if ["/chat/completions", "/responses", "/messages"]
        .iter()
        .any(|p| base.ends_with(p))
    {
        return Err("请填写 API 根地址，而不是完整的请求端点".into());
    }
    let version = if base.ends_with("/v1") { "" } else { "/v1" };
    Ok(format!("{base}{version}/{}", endpoint(&model.protocol)))
}

pub fn prepare_body(model: &Model, mut body: Value) -> Result<Value, String> {
    let object = body.as_object_mut().ok_or("请求体必须为 JSON 对象")?;
    if model.extra_enabled {
        for (key, value) in &model.extra_params {
            object.insert(key.clone(), value.clone());
        }
    }
    object.insert("model".into(), json!(model.model));
    match model.protocol.as_str() {
        "openai" => {
            let messages = object
                .get_mut("messages")
                .and_then(Value::as_array_mut)
                .ok_or("Chat 请求需要 messages 数组")?;
            if model.prompt_enabled && !model.prompt.is_empty() {
                messages.insert(0, json!({"role":"system","content":model.prompt}));
            }
            if !object.contains_key("max_tokens") && !object.contains_key("max_completion_tokens") {
                if let Some(max) = model.max_output {
                    object.insert("max_completion_tokens".into(), json!(max));
                }
            }
            if model.reasoning != "default" {
                object.insert("reasoning_effort".into(), json!(model.reasoning));
            }
        }
        "responses" => {
            if !object
                .get("input")
                .is_some_and(|v| v.is_string() || v.is_array())
            {
                return Err("Responses 请求需要 input 字符串或数组".into());
            }
            if model.prompt_enabled && !model.prompt.is_empty() {
                let original = match object.get("instructions") {
                    None | Some(Value::Null) => "",
                    Some(Value::String(v)) => v,
                    _ => return Err("instructions 必须为字符串".into()),
                };
                object.insert(
                    "instructions".into(),
                    json!(if original.is_empty() {
                        model.prompt.clone()
                    } else {
                        format!("{}\n\n{}", model.prompt, original)
                    }),
                );
            }
            if let Some(max) = model.max_output {
                object.entry("max_output_tokens").or_insert(json!(max));
            }
            if model.reasoning != "default" {
                let reasoning = object
                    .entry("reasoning")
                    .or_insert(json!({}))
                    .as_object_mut()
                    .ok_or("reasoning 必须为对象")?;
                reasoning.insert("effort".into(), json!(model.reasoning));
            }
        }
        "anthropic" => {
            if !object.get("messages").is_some_and(Value::is_array) {
                return Err("Anthropic 请求需要 messages 数组".into());
            }
            if model.prompt_enabled && !model.prompt.is_empty() {
                let original = object.remove("system");
                let system = match original {
                    None | Some(Value::Null) => json!(model.prompt),
                    Some(Value::String(text)) => json!(format!("{}\n\n{}", model.prompt, text)),
                    Some(Value::Array(mut blocks)) => {
                        blocks.insert(0, json!({"type":"text","text":model.prompt}));
                        json!(blocks)
                    }
                    _ => return Err("system 必须为字符串或内容块数组".into()),
                };
                object.insert("system".into(), system);
            }
            object
                .entry("max_tokens")
                .or_insert(json!(model.max_output.unwrap_or(8192)));
            if let Some(budget) = model.thinking_budget {
                let max = object
                    .get("max_tokens")
                    .and_then(Value::as_u64)
                    .ok_or("Anthropic max_tokens 必须为正整数")?;
                if budget < 1024 || u64::from(budget) >= max {
                    return Err("思考预算需至少 1024 且小于本次请求最大输出".into());
                }
                object.insert(
                    "thinking".into(),
                    json!({"type":"enabled","budget_tokens":budget}),
                );
            } else if model.reasoning != "default" {
                if model.reasoning == "none" {
                    object.insert("thinking".into(), json!({"type":"disabled"}));
                } else {
                    if model.reasoning == "xhigh" {
                        return Err(
                            "Anthropic 自适应思考不使用 xhigh；请选择 low、medium、high 或默认"
                                .into(),
                        );
                    }
                    object.insert("thinking".into(), json!({"type":"adaptive"}));
                    let output = object
                        .entry("output_config")
                        .or_insert(json!({}))
                        .as_object_mut()
                        .ok_or("output_config 必须为对象")?;
                    output.insert("effort".into(), json!(model.reasoning));
                }
            }
        }
        _ => return Err("协议未支持".into()),
    }
    Ok(body)
}

pub fn test_body(model: &Model) -> Value {
    let prompt = "Reply with exactly: DSH Patrick connection OK.";
    match model.protocol.as_str() {
        "responses" => {
            json!({"model":model.id,"input":prompt,"stream":true,"max_output_tokens":512})
        }
        "anthropic" => {
            json!({"model":model.id,"messages":[{"role":"user","content":prompt}],"stream":true,"max_tokens":model.thinking_budget.map_or(512, |n| n.saturating_add(512).min(model.max_output.unwrap_or(8192)))})
        }
        _ => {
            json!({"model":model.id,"messages":[{"role":"user","content":prompt}],"stream":true,"max_completion_tokens":512,"stream_options":{"include_usage":true}})
        }
    }
}

pub fn valid_zen_session(value: &str) -> bool {
    value.len() == 30
        && value.starts_with("ses_")
        && value.as_bytes()[4..16]
            .iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
        && value.as_bytes()[16..].iter().all(u8::is_ascii_alphanumeric)
}

pub const ZEN_SESSION_HEADERS: [&str; 2] = ["x-opencode-session", "x-opencode-session-id"];

pub fn has_zen_header(headers: &HeaderMap) -> bool {
    ZEN_SESSION_HEADERS.iter().any(|name| headers.contains_key(*name))
}

pub struct ZenSession {
    pub value: Option<HeaderValue>,
    pub state: &'static str,
}

// Preserve the verified CLI format. Empty primary falls back to the alias, like the relay.
// Reject malformed or ambiguous repeated values without echoing any header contents.
pub fn zen_session(headers: &HeaderMap) -> Result<ZenSession, String> {
    let mut values = Vec::new();
    for name in ZEN_SESSION_HEADERS {
        let mut selected: Option<HeaderValue> = None;
        for value in headers.get_all(name) {
            let text = value.to_str().map_err(|_| "Zen 会话头格式无效")?;
            if text.trim().is_empty() { continue; }
            if !valid_zen_session(text) { return Err("Zen 会话头格式无效".into()); }
            if selected.as_ref().is_some_and(|prior| prior != value) {
                return Err("Zen 同名会话头存在重复值冲突".into());
            }
            selected = Some(value.clone());
        }
        values.push(selected);
    }
    let state = match (&values[0], &values[1]) {
        (Some(a), Some(b)) if a != b => "conflict_primary",
        (Some(_), Some(_)) => "both",
        (Some(_), None) => "primary",
        (None, Some(_)) => "alias",
        _ => "absent",
    };
    Ok(ZenSession { value: values[0].take().or(values[1].take()), state })
}

fn set_zen_session(headers: &mut HeaderMap, mut value: HeaderValue) {
    value.set_sensitive(true);
    for name in ZEN_SESSION_HEADERS { headers.insert(name, value.clone()); }
}

pub fn outgoing_headers(model: &Model, incoming: &HeaderMap) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    headers.insert(
        "accept",
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert("user-agent", HeaderValue::from_static("DSH-Patrick/0.1"));
    if model.protocol == "anthropic" {
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    }
    for (name, value) in model.headers.iter().filter(|_| model.headers_enabled) {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| "请求头名称无效")?;
        let mut value = HeaderValue::from_str(value).map_err(|_| "请求头值无效")?;
        value.set_sensitive(true);
        headers.insert(name, value);
    }
    if !model.api_key.is_empty() {
        let key = if model.protocol == "anthropic" {
            model.api_key.clone()
        } else {
            format!("Bearer {}", model.api_key)
        };
        let mut value = HeaderValue::from_str(&key).map_err(|_| "API Key 格式无效")?;
        value.set_sensitive(true);
        headers.insert(
            if model.protocol == "anthropic" {
                "x-api-key"
            } else {
                "authorization"
            },
            value,
        );
    }
    // Only the two session aliases are inherited, never client auth or arbitrary headers.
    // Normalize after static headers so neither stale alias can override a dynamic session.
    let dynamic = zen_session(incoming)?;
    if let Some(value) = dynamic.value {
        set_zen_session(&mut headers, value);
    } else if has_zen_header(&headers) {
        let configured = zen_session(&headers)?;
        if let Some(value) = configured.value { set_zen_session(&mut headers, value); }
        else { for name in ZEN_SESSION_HEADERS { headers.remove(name); } }
    }
    Ok(headers)
}

#[derive(Default)]
pub struct Metrics {
    pub first_valid_ms: Option<u64>,
    pub finish_reason: Option<String>,
    pub reasoning_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub first_token_ms: Option<u64>,
    pub text: String,
    pub saw_text: bool,
    pub api_error: bool,
    pub completed: bool,
    zen_stream_error: Option<&'static str>,
    zen_nontext_output: bool,
    event_name: String,
    pending: Vec<u8>,
    event_data: String,
    json_body: Vec<u8>,
    sse: bool,
}
impl Metrics {
    pub fn new(sse: bool) -> Self {
        Self {
            sse,
            ..Self::default()
        }
    }
    pub fn zen_failure(&self) -> Option<&'static str> {
        if let Some(error) = self.zen_stream_error { Some(error) }
        else if !self.completed { Some("Zen 上游流提前结束（缺少模型结束事件）") }
        else if !self.saw_text && !self.zen_nontext_output { Some("Zen 上游未返回模型输出（空响应或仅心跳）") }
        else { None }
    }
    pub fn zen_success(&self) -> bool { self.zen_failure().is_none() }
    fn event(&mut self, value: &Value, elapsed: u64) {
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let root = value.get("response").or_else(|| value.get("message")).unwrap_or(value);
        let status = root.get("status").and_then(Value::as_str).unwrap_or("");
        // Zen diagnostics inspect the result, not mere presence of an optional error:null.
        // Keep the pre-existing non-Zen metrics/result policy unchanged.
        if [value, root].iter().any(|v| v.get("error").is_some_and(|e| !e.is_null()))
            || matches!(kind, "error" | "response.failed" | "response.incomplete")
            || matches!(status, "failed" | "incomplete" | "cancelled")
            || ((!self.sse || kind == "response.completed") && matches!(status, "in_progress" | "queued"))
        { self.zen_stream_error = Some("Zen 上游返回流内或结构化错误"); }
        let zen_reason = value.pointer("/choices/0/finish_reason")
            .or_else(|| value.pointer("/delta/stop_reason")).or_else(|| root.get("stop_reason"));
        if zen_reason.and_then(Value::as_str).is_some_and(|r| matches!(r,"length"|"max_tokens"|"content_filter"|"error")) {
            self.zen_stream_error = Some("Zen 模型输出未正常结束（达到长度上限或中止）");
        }
        if value.get("error").is_some()
            || matches!(kind, "error" | "response.failed" | "response.incomplete")
        {
            self.api_error = true;
        }
        if matches!(kind, "response.completed" | "message_stop")
            || value
                .pointer("/choices/0/finish_reason")
                .is_some_and(|v| !v.is_null())
        {
            self.completed = true;
        }
        if let Some(reason)=value.pointer("/choices/0/finish_reason").or_else(||value.pointer("/delta/stop_reason")).and_then(Value::as_str) {
            self.finish_reason=Some(reason.chars().take(100).collect());
        } else if kind=="response.completed" {self.finish_reason=Some("completed".into());}
        let has_content=value.pointer("/choices/0/delta/content").and_then(Value::as_str).is_some_and(|s|!s.is_empty())
            || value.pointer("/choices/0/delta/reasoning_content").and_then(Value::as_str).is_some_and(|s|!s.is_empty())
            || value.pointer("/choices/0/delta/tool_calls").and_then(Value::as_array).is_some_and(|s|!s.is_empty())
            || value.pointer("/choices/0/message").is_some()
            || matches!(kind,"content_block_start"|"content_block_delta"|"response.output_text.delta"|"response.reasoning_summary_text.delta"|"response.function_call_arguments.delta");
        if has_content {self.first_valid_ms.get_or_insert(elapsed);}
        self.zen_nontext_output |= value.pointer("/choices/0/delta/reasoning_content").or_else(||value.pointer("/choices/0/message/reasoning_content")).and_then(Value::as_str).is_some_and(|s| !s.is_empty())
            || value.pointer("/choices/0/delta/tool_calls").or_else(||value.pointer("/choices/0/message/tool_calls")).and_then(Value::as_array).is_some_and(|v| !v.is_empty())
            || (kind=="response.function_call_arguments.delta" && value.get("delta").and_then(Value::as_str).is_some_and(|s| !s.is_empty()))
            || (kind=="content_block_start" && value.pointer("/content_block/type").and_then(Value::as_str)==Some("tool_use"))
            || value.pointer("/delta/thinking").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
        if let Some(usage) = root.get("usage").or_else(|| value.get("usage")) {
            if let Some(n)=usage.pointer("/completion_tokens_details/reasoning_tokens").or_else(||usage.pointer("/output_tokens_details/reasoning_tokens")).and_then(Value::as_u64) {self.reasoning_tokens=Some(n);}
            let cache_read = usage.pointer("/prompt_tokens_details/cached_tokens")
                .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
                .or_else(|| usage.get("cache_read_input_tokens"))
                .or_else(|| usage.get("prompt_cache_hit_tokens")).and_then(Value::as_u64);
            let cache_write = usage.get("cache_creation_input_tokens").and_then(Value::as_u64);
            if let Some(n) = cache_read { self.cache_read_tokens = Some(n); }
            if let Some(n) = cache_write { self.cache_write_tokens = Some(n); }
            if let Some(n) = usage
                .get("prompt_tokens")
                .or_else(|| usage.get("input_tokens"))
                .and_then(Value::as_u64)
            {
                self.input_tokens = Some(if usage.get("cache_read_input_tokens").is_some() || usage.get("cache_creation_input_tokens").is_some() {
                    n.saturating_add(cache_read.unwrap_or(0)).saturating_add(cache_write.unwrap_or(0))
                } else { n });
            }
            if let Some(n) = usage
                .get("completion_tokens")
                .or_else(|| usage.get("output_tokens"))
                .and_then(Value::as_u64)
            {
                self.output_tokens = Some(n);
            }
        }
        let mut text = String::new();
        if let Some(t) = value
            .pointer("/choices/0/delta/content")
            .or_else(|| value.pointer("/choices/0/message/content"))
            .and_then(Value::as_str)
        {
            text.push_str(t);
        }
        if value.get("type").and_then(Value::as_str) == Some("response.output_text.delta") {
            if let Some(t) = value.get("delta").and_then(Value::as_str) {
                text.push_str(t);
            }
        }
        if let Some(t) = value.pointer("/delta/text").and_then(Value::as_str) {
            text.push_str(t);
        }
        if let Some(blocks) = root.get("content").and_then(Value::as_array) {
            for block in blocks {
                self.zen_nontext_output |= block.get("type").and_then(Value::as_str)==Some("tool_use");
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    text.push_str(t);
                }
            }
        }
        if !self.sse {
            if let Some(output) = root.get("output").and_then(Value::as_array) {
                for part in output {
                    self.zen_nontext_output |= part.get("type").and_then(Value::as_str)==Some("function_call");
                    if let Some(blocks) = part.get("content").and_then(Value::as_array) {
                        for block in blocks {
                            if let Some(t) = block.get("text").and_then(Value::as_str) {
                                text.push_str(t);
                            }
                        }
                    }
                }
            }
        }
        if !text.is_empty() {
            self.saw_text = true;
            self.first_token_ms.get_or_insert(elapsed);
            let remaining = 500usize.saturating_sub(self.text.chars().count());
            self.text.extend(text.chars().take(remaining));
        }
    }
    pub fn feed(&mut self, bytes: &[u8], elapsed: u64) {
        if !self.sse {
            if self.json_body.len() + bytes.len() <= 2 * 1024 * 1024 {
                self.json_body.extend_from_slice(bytes);
            }
            return;
        }
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.pending.drain(..=end).collect();
            if let Ok(line) = std::str::from_utf8(&line) {
                let line = line.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    self.flush_event(elapsed);
                } else if let Some(name) = line.strip_prefix("event:") {
                    self.event_name = name.trim().chars().take(64).collect();
                } else if let Some(data) = line.strip_prefix("data:") {
                    if self.event_data.len() + data.len() < 128 * 1024 {
                        self.event_data.push_str(data.trim_start_matches(' '));
                        self.event_data.push('\n');
                    } else {
                        self.event_data.clear();
                        self.api_error = true;
                        self.zen_stream_error = Some("Zen 上游 SSE 事件超出诊断解析上限");
                    }
                }
            }
        }
        if self.pending.len() > 128 * 1024 {
            self.pending.clear();
            self.zen_stream_error = Some("Zen 上游 SSE 行超出诊断解析上限");
        }
    }
    pub fn finish(&mut self, elapsed: u64) {
        if self.sse {
            if self.pending.iter().any(|b| !b.is_ascii_whitespace()) || !self.event_data.is_empty() || !self.event_name.is_empty() {self.zen_stream_error.get_or_insert("Zen 上游 SSE 事件截断");}
            self.feed(b"\n\n", elapsed);
        } else if let Ok(value) = serde_json::from_slice(&self.json_body) {
            self.event(&value, elapsed);
            self.first_token_ms = None;
            self.completed = true;
        }
    }
    fn flush_event(&mut self, elapsed: u64) {
        let data = std::mem::take(&mut self.event_data);
        let name = std::mem::take(&mut self.event_name);
        if matches!(name.as_str(), "error" | "response.failed" | "response.incomplete") {self.zen_stream_error=Some("Zen 上游返回 SSE 错误事件");}
        if data.trim() == "[DONE]" {
            self.completed = true;
        } else if let Ok(value) = serde_json::from_str(data.trim()) {
            self.event(&value, elapsed);
        } else if !data.trim().is_empty() { self.zen_stream_error=Some("Zen 上游 SSE 数据格式无效"); }
    }
}

pub fn redact_preview(text: &str, secrets: &[String]) -> String {
    let mut clean = text.to_owned();
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        clean = clean.replace(secret, "[已脱敏]");
        // Also suppress the prefix of a credential cut off at the preview boundary.
        let mut lengths: Vec<usize> = secret
            .char_indices()
            .map(|(i, _)| i)
            .filter(|n| *n > 0 && *n <= clean.len())
            .collect();
        lengths.reverse();
        for length in lengths {
            if clean.ends_with(&secret[..length]) {
                clean.truncate(clean.len() - length);
                clean.push_str("[已脱敏]");
                break;
            }
        }
    }
    clean.chars().take(500).collect()
}
