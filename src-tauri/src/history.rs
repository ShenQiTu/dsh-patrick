//! Bounded encrypted usage metadata. No messages, headers, keys or response previews.
use crate::{gateway::CallRecord, vault};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub id: String,
    pub started_at: u64,
    pub source: String,
    pub state: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

pub struct History {
    path: PathBuf,
    records: Vec<UsageRecord>,
    pub version: u64,
    pub error: Option<String>,
    readable: bool,
}
impl History {
    pub fn open(path: PathBuf) -> Self {
        let loaded = if path.exists() {
            vault::read(&path).and_then(|bytes| serde_json::from_slice::<Vec<UsageRecord>>(&bytes).map_err(|_| "统计历史格式错误；原文件保持不变".into()))
        } else { Ok(Vec::new()) };
        match loaded {
            Ok(mut records) => {
                Self::trim(&mut records);
                Self { path, records, version: 0, error: None, readable: true }
            }
            Err(error) => Self { path, records: Vec::new(), version: 0, error: Some(error), readable: false },
        }
    }
    fn trim(records: &mut Vec<UsageRecord>) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        records.retain(|r| r.started_at >= now.saturating_sub(366 * 86_400_000));
        records.sort_by_key(|r| r.started_at);
        if records.len() > 10000 { records.drain(..records.len() - 10000); }
    }
    pub fn records(&self) -> Vec<UsageRecord> { self.records.clone() }
    pub fn record(&mut self, call: &CallRecord) {
        self.records.push(UsageRecord {
            id: call.id.clone(), started_at: call.started_at, source: call.source.clone(), state: call.state.clone(),
            input_tokens: call.input_tokens, output_tokens: call.output_tokens,
            cache_read_tokens: call.cache_read_tokens, cache_write_tokens: call.cache_write_tokens,
        });
        Self::trim(&mut self.records);
        self.version += 1;
        // A corrupt/unreadable original is never silently replaced with an empty history.
        if self.readable {
            self.error = serde_json::to_vec(&self.records).map_err(|_| "统计历史序列化失败".to_string())
                .and_then(|bytes| vault::write(&self.path, &bytes)).err();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_history_reopens_without_payloads() {
        let dir = std::env::temp_dir().join(crate::config::new_id());
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("usage.dpapi");
        let mut h = History::open(path.clone());
        let mut call: CallRecord = serde_json::from_value(serde_json::json!({
            "id":"test-usage", "modelId":"test-model", "name":"test", "protocol":"openai", "source":"gateway", "host":"localhost",
            "startedAt":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64,
            "state":"success", "status":200, "durationMs":10, "firstTokenMs":5,
            "inputTokens":100, "outputTokens":20, "cacheReadTokens":40, "cacheWriteTokens":0,
            "tokensPerSecond":2, "streamed":true, "bytes":1000,
            "preview":"DO_NOT_STORE_PREVIEW", "message":"DO_NOT_STORE_MESSAGE"
        })).unwrap();
        h.record(&call); assert!(h.error.is_none());
        let plain = String::from_utf8(vault::read(&path).unwrap()).unwrap();
        assert!(!plain.contains("DO_NOT_STORE"));
        assert!(!plain.contains("modelId"));
        let reopened = History::open(path.clone());
        assert_eq!(reopened.records()[0].cache_read_tokens, Some(40));
        assert_eq!(reopened.records().len(), 1);
        call.source = "test".into(); h.record(&call);
        assert_eq!(History::open(path.clone()).records()[1].source, "test");
        std::fs::remove_file(path).unwrap(); std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn corrupt_history_is_preserved() {
        let dir=std::env::temp_dir().join(crate::config::new_id()); std::fs::create_dir(&dir).unwrap();
        let path=dir.join("usage.dpapi"); std::fs::write(&path,b"CORRUPT_FIXTURE").unwrap();
        let h=History::open(path.clone()); assert!(h.error.is_some()); assert!(!h.readable);
        assert_eq!(std::fs::read(&path).unwrap(),b"CORRUPT_FIXTURE");
        std::fs::remove_file(path).unwrap(); std::fs::remove_dir(dir).unwrap();
    }
}
