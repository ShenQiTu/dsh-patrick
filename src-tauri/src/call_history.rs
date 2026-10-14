//! Encrypted call summaries and opt-in bounded payloads, separate from usage totals.
use crate::{gateway::CallRecord, history::UsageRecord, vault};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::VecDeque, io::Write, path::PathBuf};

pub const PAYLOAD_LIMIT: usize = 128 * 1024;
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallInfo {
    pub model: Option<String>,
    pub request_url: Option<String>,
    pub reasoning: Option<String>,
    pub finish_reason: Option<String>,
    pub ttfb_ms: Option<u64>,
    pub ttfr_ms: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub message_count: Option<usize>,
    pub tool_count: Option<usize>,
    pub error_kind: Option<String>,
    pub detailed: bool,
    pub legacy: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    pub headers: Value,
    pub body: Option<Value>,
    pub response: String,
    pub response_chunks: usize,
    pub request_truncated: bool,
    pub response_truncated: bool,
}

pub struct CallHistory {
    directory: PathBuf,
    pub records: VecDeque<CallRecord>,
    pub error: Option<String>,
    readable: bool,
}
impl CallHistory {
    pub fn open(directory: PathBuf, legacy: &[UsageRecord], limit: usize) -> Self {
        let path = directory.join("calls.dpapi");
        let loaded: Result<VecDeque<CallRecord>,String> = if path.exists() {
            vault::read(&path).and_then(|bytes| serde_json::from_slice(&bytes).map_err(|_| "调用记录格式错误；原文件保持不变".into()))
        } else {
            Ok(legacy.iter().map(|r| CallRecord {
                id:r.id.clone(), started_at:r.started_at, source:r.source.clone(), state:r.state.clone(),
                input_tokens:r.input_tokens, output_tokens:r.output_tokens,
                cache_read_tokens:r.cache_read_tokens, cache_write_tokens:r.cache_write_tokens,
                name:"旧版历史摘要".into(), message:"旧版本仅保存时间、状态与 Token；模型、耗时及正文未记录".into(),
                info:CallInfo{legacy:true,..Default::default()}, ..Default::default()
            }).collect())
        };
        match loaded {
            Ok(mut records) => {
                for record in &mut records {
                    if record.state == "running" {
                        record.state = "cancelled".into();
                        record.message = "上次应用退出时未完成".into();
                        record.info.error_kind = Some("interrupted".into());
                    }
                }
                let mut history = Self {directory, records, error:None, readable:true};
                let retired=history.trim(limit);
                history.error=history.flush().err();
                if history.error.is_none() { for path in retired {let _=std::fs::remove_file(path);} }
                history
            }
            Err(error) => Self {directory,records:VecDeque::new(),error:Some(error),readable:false},
        }
    }
    fn payload_path(&self, id: &str) -> Result<PathBuf,String> {
        if id.is_empty() || id.len()>160 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b==b'-' || b==b'_') {
            return Err("调用标识无效".into());
        }
        Ok(self.directory.join("call-details").join(format!("{id}.dpapi")))
    }
    fn flush(&self) -> Result<(),String> {
        if !self.readable { return Err("调用历史未读取成功，原文件保持不变".into()); }
        let bytes=serde_json::to_vec(&self.records).map_err(|_| "调用记录序列化失败")?;
        vault::write(&self.directory.join("calls.dpapi"), &bytes)
    }
    fn trim(&mut self, limit: usize) -> Vec<PathBuf> {
        let mut retired=Vec::new();
        while self.records.len()>limit {
            if let Some(old)=self.records.pop_front() {
                if let Ok(path)=self.payload_path(&old.id) { retired.push(path); }
            }
        }
        retired
    }
    pub fn record(&mut self, record: &CallRecord, payload: Option<&Payload>, limit: usize) {
        if !self.readable { return; }
        let outcome=(|| {
            // Payloads are never included in the polling snapshot / summary file.
            if let Some(payload)=payload {
                let path=self.payload_path(&record.id)?;
                std::fs::create_dir_all(path.parent().unwrap()).map_err(|_| "创建调用详情目录失败")?;
                vault::write(&path,&serde_json::to_vec(payload).map_err(|_| "调用详情序列化失败")?)?;
            }
            let mut summary=record.clone(); summary.preview.clear();
            if let Some(old)=self.records.iter_mut().find(|r| r.id==record.id) { *old=summary; }
            else { self.records.push_back(summary); }
            // Completion order need not equal request start order.
            self.records.make_contiguous().sort_by_key(|r| r.started_at);
            let retired=self.trim(limit);
            self.flush()?;
            for path in retired {
                if path.exists() {std::fs::remove_file(path).map_err(|_| "旧调用详情清理失败")?;}
            }
            Ok::<(),String>(())
        })();
        self.error=outcome.err();
    }
    pub fn payload(&self, id: &str) -> Result<Option<Payload>,String> {
        let path=self.payload_path(id)?;
        if !self.records.iter().any(|r| r.id==id) || !path.exists() { return Ok(None); }
        serde_json::from_slice(&vault::read(&path)?).map(Some).map_err(|_| "读取调用详情失败；原文件保持不变".into())
    }
    pub fn clear(&mut self, details_only: bool) -> Result<(),String> {
        // Do not delete payloads when a locked/unwritable summary prevents clearing.
        if !details_only { vault::write(&self.directory.join("calls.dpapi"), b"[]")?; }
        // Explicit clearing only touches known IDs, never recursively removes a directory.
        for record in &self.records {
            let path=self.payload_path(&record.id)?;
            if path.exists() { std::fs::remove_file(path).map_err(|_| "删除调用详情失败")?; }
        }
        if !details_only {
            self.records.clear(); self.readable=true;
        }
        self.error=None; Ok(())
    }
}

fn sensitive(name: &str) -> bool {
    let name=name.to_ascii_lowercase().replace(['-', '_'], "");
    name.contains("authorization") || name.contains("apikey") || name.contains("cookie")
        || name.contains("password") || name.contains("secret") || name=="token"
        || name.ends_with("accesstoken") || name.ends_with("refreshtoken")
}
pub fn redact_text(text: &str, secrets: &[String]) -> String {
    let mut clean=text.to_owned();
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        clean=clean.replace(secret,"[已脱敏]");
        if let Ok(encoded)=serde_json::to_string(secret) {
            clean=clean.replace(&encoded[1..encoded.len()-1],"[已脱敏]");
        }
        // A bounded response can cut through a credential, including across transport chunks.
        for (length,_) in secret.char_indices().rev().filter(|(n,_)| *n>0) {
            if clean.ends_with(&secret[..length]) { clean.truncate(clean.len()-length);clean.push_str("[已脱敏]");break; }
        }
    }
    clean
}
fn redact_json(value: &mut Value, secrets: &[String]) {
    match value {
        Value::Object(map) => for (key,value) in map {
            if sensitive(key) { *value=Value::String("[已脱敏]".into()); }
            else { redact_json(value,secrets); }
        },
        Value::Array(values) => for value in values { redact_json(value,secrets); },
        Value::String(text) => *text=redact_text(text,secrets),
        _ => (),
    }
}
struct Limited(Vec<u8>);
impl Write for Limited {
    fn write(&mut self, buf:&[u8]) -> std::io::Result<usize> {
        if self.0.len()+buf.len()>PAYLOAD_LIMIT { return Err(std::io::Error::other("capture limit")); }
        self.0.extend_from_slice(buf);Ok(buf.len())
    }
    fn flush(&mut self)->std::io::Result<()> {Ok(())}
}
pub fn capture_request(headers: &axum::http::HeaderMap, body: &Value, secrets: &[String]) -> Payload {
    let mut safe_headers=serde_json::Map::new();
    for (name,value) in headers {
        // Header values outside this non-credential allowlist are always masked.
        let text=if matches!(name.as_str(),"content-type"|"accept"|"user-agent"|"anthropic-version"|"anthropic-beta") {
            redact_text(value.to_str().unwrap_or(""),secrets)
        } else {"[已脱敏]".into()};
        safe_headers.insert(name.to_string(),json!(text));
    }
    let mut bytes=Limited(Vec::new());
    let truncated=serde_json::to_writer(&mut bytes,body).is_err();
    let mut safe_body=if truncated {None} else {serde_json::from_slice::<Value>(&bytes.0).ok()};
    if let Some(body)=&mut safe_body {redact_json(body,secrets);}
    Payload {headers:Value::Object(safe_headers),body:safe_body,request_truncated:truncated,..Default::default()}
}
pub fn capture_response(bytes: &[u8], secrets:&[String]) -> String {
    let raw=String::from_utf8_lossy(bytes);
    if let Ok(mut json)=serde_json::from_str::<Value>(&raw) {
        redact_json(&mut json,secrets);return serde_json::to_string_pretty(&json).unwrap_or_default();
    }
    let mut lines=Vec::new();
    for line in raw.lines() {
        if let Some(data)=line.strip_prefix("data:") {
            if let Ok(mut json)=serde_json::from_str::<Value>(data.trim()) {
                redact_json(&mut json,secrets);
                lines.push(format!("data: {}",json));continue;
            }
        }
        lines.push(redact_text(line,secrets));
    }
    redact_text(&lines.join("\n"),secrets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::new_id;
    fn dir()->PathBuf { let p=std::env::temp_dir().join(new_id());std::fs::create_dir(&p).unwrap();p }
    fn cleanup(p:PathBuf) {
        if p.join("call-details").exists() { std::fs::remove_dir(p.join("call-details")).unwrap(); }
        std::fs::remove_file(p.join("calls.dpapi")).unwrap();std::fs::remove_dir(p).unwrap();
    }
    #[test]
    fn restart_legacy_interruption_and_clear_do_not_reimport() {
        let p=dir();
        let legacy=UsageRecord{id:"legacy-1".into(),started_at:1,source:"gateway".into(),state:"success".into(),input_tokens:Some(10),output_tokens:Some(2),cache_read_tokens:None,cache_write_tokens:None};
        let mut h=CallHistory::open(p.clone(),std::slice::from_ref(&legacy),10);
        assert!(h.records[0].info.legacy);assert!(h.records[0].info.model.is_none());
        h.record(&CallRecord{id:"running-1".into(),state:"running".into(),started_at:2,..Default::default()},None,10);
        let mut reopened=CallHistory::open(p.clone(),&[],10);
        assert_eq!(reopened.records[1].state,"cancelled");
        assert_eq!(reopened.records[1].info.error_kind.as_deref(),Some("interrupted"));
        reopened.clear(false).unwrap();
        assert!(CallHistory::open(p.clone(),&[legacy],10).records.is_empty());
        cleanup(p);println!("CALL_RESTART_PASS legacy=SUMMARY interrupted=CANCELLED clear=NO_REIMPORT");
    }
    #[test]
    fn payload_redaction_boundaries_and_opt_in() {
        let mut headers=axum::http::HeaderMap::new();
        headers.insert("authorization","Bearer TEST_CREDENTIAL_123".parse().unwrap());
        headers.insert("x-other-private","CUSTOM_PRIVATE".parse().unwrap());
        headers.insert("content-type","application/json".parse().unwrap());
        let secrets=vec!["TEST_CREDENTIAL_123".to_string(),"CUSTOM_PRIVATE".to_string()];
        let body=json!({"api_key":"UNKNOWN_SECRET","messages":[{"content":"SYNTHETIC_TEXT TEST_CREDENTIAL_123"}],"max_tokens":100});
        let payload=capture_request(&headers,&body,&secrets);
        let text=serde_json::to_string(&payload).unwrap();
        for secret in ["TEST_CREDENTIAL_123","CUSTOM_PRIVATE","UNKNOWN_SECRET"] {assert!(!text.contains(secret));}
        assert_eq!(payload.body.as_ref().unwrap()["max_tokens"],100);
        let response=capture_response(b"data: {\"access_token\":\"UNKNOWN_SECRET\",\"content\":\"TEST_CREDENTIAL_123\"}\n\ndata: [DONE]\n",&secrets);
        assert!(!response.contains("UNKNOWN_SECRET") && !response.contains("TEST_CREDENTIAL_123"));
        assert!(!capture_response(b"truncated TEST_CREDENTIAL_",&secrets).contains("TEST_CREDENTIAL_"));
        assert!(capture_request(&headers,&json!({"input":"X".repeat(PAYLOAD_LIMIT+1)}),&secrets).request_truncated);
        let p=dir();let mut h=CallHistory::open(p.clone(),&[],10);
        let r=CallRecord{id:"call-1".into(),preview:"MEMORY_ONLY".into(),state:"success".into(),..Default::default()};
        h.record(&r,None,10);assert!(h.payload(&r.id).unwrap().is_none());
        h.record(&r,Some(&payload),10);
        let raw=vault::read(&p.join("calls.dpapi")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("MEMORY_ONLY"));
        let mut reopened=CallHistory::open(p.clone(),&[],10);
        assert!(reopened.payload("call-1").unwrap().is_some());
        assert!(reopened.payload("../config").is_err());
        reopened.clear(true).unwrap();assert_eq!(reopened.records.len(),1);assert!(reopened.payload("call-1").unwrap().is_none());
        reopened.clear(false).unwrap();cleanup(p);
        println!("CALL_PAYLOAD_PASS headers=MASKED body_keys=MASKED chunk_boundary=MASKED bound=128KiB encrypted_reopen=YES details_clear=PRESERVES_SUMMARY");
    }
    #[test]
    fn call_retention_prunes_old_payload_and_corrupt_original_survives() {
        let p=dir();let mut h=CallHistory::open(p.clone(),&[],2);
        for n in 0..5 {h.record(&CallRecord{id:format!("c{n}"),started_at:n,state:"success".into(),..Default::default()},Some(&Payload::default()),2);}
        assert_eq!(h.records.len(),2);assert!(!p.join("call-details/c0.dpapi").exists());
        h.clear(false).unwrap();
        std::fs::write(p.join("calls.dpapi"),b"BROKEN_FIXTURE").unwrap();
        let mut h=CallHistory::open(p.clone(),&[],2);assert!(h.error.is_some());
        h.record(&CallRecord::default(),None,2);
        assert_eq!(std::fs::read(p.join("calls.dpapi")).unwrap(),b"BROKEN_FIXTURE");
        cleanup(p);println!("CALL_RETENTION_PASS oldest_payload=REMOVED corrupt_original=PRESERVED");
    }
}
