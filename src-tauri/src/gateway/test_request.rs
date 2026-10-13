//! Test-button helpers only. Never synthesize or replace a DSH conversation ID.
use crate::protocol::{self, Metrics};
use serde_json::Value;
use std::{sync::Mutex, time::{SystemTime, UNIX_EPOCH}};

#[derive(Default)]
struct SessionClock {
    last_ms: u64,
    counter: u64,
}
impl SessionClock {
    fn prefix(&mut self, millis: u64) -> u64 {
        self.counter = if self.last_ms == millis { self.counter.wrapping_add(1) } else { 1 };
        self.last_ms = millis;
        // Same encoding as the read-only relay reference: zen_session_id.go.
        !((millis << 12) | (self.counter & 0xfff)) & 0xffffffffffff
    }
}
static SESSION_CLOCK: Mutex<SessionClock> = Mutex::new(SessionClock { last_ms: 0, counter: 0 });

pub(super) fn session_id() -> Result<String, String> {
    const CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut bytes = [0u8; 14];
    getrandom::fill(&mut bytes).map_err(|_| "测试用 Zen 会话头生成失败")?;
    let prefix = {
        let mut clock = SESSION_CLOCK.lock().map_err(|_| "测试用 Zen 时间编码锁异常")?;
        let millis = SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|_| "系统时间早于 Unix 时间起点")?.as_millis() as u64;
        clock.prefix(millis)
    };
    let mut session = format!("ses_{prefix:012x}");
    for byte in bytes { session.push(CHARS[usize::from(byte) % CHARS.len()] as char); }
    Ok(session)
}

fn event_error(value: &Value, event_name: &str) -> Option<String> {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or(event_name);
    let root = value.get("response").unwrap_or(value);
    let error = value.get("error").filter(|v| !v.is_null())
        .or_else(|| root.get("error").filter(|v| !v.is_null()));
    let status = root.get("status").and_then(Value::as_str).unwrap_or("");
    let error_event = matches!(event_name, "error" | "response.failed" | "response.incomplete")
        || matches!(kind, "error" | "response.failed" | "response.incomplete");
    if error.is_some() || error_event
        || matches!(status, "failed" | "incomplete" | "cancelled" | "in_progress" | "queued")
    {
        // Intermediate Responses events may legitimately be in_progress.
        if error.is_none() && !error_event && matches!(status, "in_progress" | "queued")
            && kind.starts_with("response.") && kind != "response.completed" { return None; }
        let detail = error.and_then(|e| e.get("message").and_then(Value::as_str).or_else(|| e.as_str()))
            .or_else(|| value.get("message").and_then(Value::as_str))
            .or_else(|| root.pointer("/incomplete_details/reason").and_then(Value::as_str))
            .unwrap_or(if !status.is_empty() { status } else if !kind.is_empty() { kind } else { event_name });
        return Some(format!("上游返回错误：{detail}"));
    }
    let reason = value.pointer("/choices/0/finish_reason")
        .or_else(|| value.pointer("/delta/stop_reason"))
        .or_else(|| value.get("stop_reason")).and_then(Value::as_str);
    if let Some(reason) = reason {
        if !matches!(reason, "stop" | "end_turn" | "stop_sequence") {
            return Some(format!("模型未正常结束：{reason}"));
        }
    }
    None
}

pub(super) fn response_result(body: &[u8], sse: bool, success: bool, status: u16, metrics: &Metrics) -> Result<(), String> {
    if !success {
        let detail = serde_json::from_slice::<Value>(body).ok()
            .and_then(|v| event_error(&v, "error"))
            .unwrap_or_else(|| "错误正文非结构化 JSON，未记录正文".into());
        return Err(format!("上游 HTTP {status}：{detail}"));
    }
    if sse {
        let text = std::str::from_utf8(body).map_err(|_| "上游 SSE 包含无效 UTF-8")?
            .replace("\r\n", "\n").replace('\r', "\n");
        let mut events = text.split("\n\n").peekable();
        while let Some(event) = events.next() {
            let mut data = Vec::new();
            let mut name = "";
            for line in event.lines() {
                if let Some(s) = line.strip_prefix("data:") { data.push(s.strip_prefix(' ').unwrap_or(s)); }
                if let Some(s) = line.strip_prefix("event:") { name = s.trim(); }
            }
            if data.is_empty() {
                if matches!(name, "error" | "response.failed" | "response.incomplete") { return Err("上游 SSE 错误事件（未提供错误内容）".into()); }
                continue;
            }
            if events.peek().is_none() { return Err("上游 SSE 事件截断（缺少事件结束分隔符）".into()); }
            let data = data.join("\n");
            if data.trim() != "[DONE]" {
                let value: Value = serde_json::from_str(&data).map_err(|_| "上游 SSE 数据格式无效（非 JSON 事件）")?;
                if let Some(error) = event_error(&value, name) { return Err(error); }
            }
        }
    } else {
        let value: Value = serde_json::from_slice(body).map_err(|_| {
            if body.is_empty() { "上游返回空响应" } else { "上游响应不是有效 JSON" }
        })?;
        if let Some(error) = event_error(&value, "") { return Err(error); }
    }
    if !metrics.saw_text || metrics.text.trim().is_empty() { return Err("上游未返回模型文本（空响应）".into()); }
    if !metrics.completed { return Err("上游流提前结束（缺少模型结束事件）".into()); }
    Ok(())
}

pub(super) fn redact_reason(reason: &str, secrets: &[String]) -> String {
    // Never persist a raw response/header dump. Strip known credentials before truncation.
    // Suppress auth-shaped details even when an upstream echoes an unfamiliar credential.
    let lower = reason.to_ascii_lowercase();
    if ["authorization", "bearer ", "api_key", "api-key", "cookie"].iter().any(|s| lower.contains(s)) {
        return "上游返回错误（包含鉴权信息，详情已隐藏）".into();
    }
    protocol::redact_preview(reason, secrets).chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_time_vectors_and_counter() {
        let mut clock = SessionClock::default();
        for (ms, expected) in [(0, 0xfffffffffffe), (1, 0xffffffffeffe),
            (1, 0xffffffffeffd), (1791875767433, 0xecbe1a376ffe),
            (1791875767433, 0xecbe1a376ffd), (1791875767434, 0xecbe1a375ffe)] {
            assert_eq!(clock.prefix(ms), expected);
        }
        clock.counter = 4095;
        assert_eq!(clock.prefix(1791875767434), 0xecbe1a375fff);
    }
}
