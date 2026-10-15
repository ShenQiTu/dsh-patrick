use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use url::Url;

pub fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "p{:x}{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub model: String,
    pub protocol: String,
    pub base_url: String,
    pub api_key: String,
    pub headers: BTreeMap<String, String>,
    pub enabled: bool,
    pub context_window: Option<u32>,
    pub max_output: Option<u32>,
    pub reasoning: String,
    pub prompt: String,
    #[serde(default = "default_prompt_enabled")]
    pub prompt_enabled: bool,
    #[serde(default)]
    pub use_full_url: bool,
    #[serde(default)]
    pub notes: String,
    #[serde(default = "default_headers_enabled")]
    pub headers_enabled: bool,
    #[serde(default)]
    pub zen_test_enabled: bool,
    #[serde(default)]
    pub extra_enabled: bool,
    #[serde(default)]
    pub extra_params: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub thinking_budget: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelInput {
    pub id: Option<String>,
    pub name: String,
    pub model: String,
    pub protocol: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub enabled: bool,
    pub context_window: Option<u32>,
    pub max_output: Option<u32>,
    pub reasoning: String,
    pub prompt: String,
    #[serde(default = "default_prompt_enabled")]
    pub prompt_enabled: bool,
    #[serde(default)]
    pub use_full_url: bool,
    #[serde(default)]
    pub notes: String,
    #[serde(default = "default_headers_enabled")]
    pub headers_enabled: bool,
    #[serde(default)]
    pub zen_test_enabled: bool,
    #[serde(default)]
    pub extra_enabled: bool,
    #[serde(default)]
    pub extra_params: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub thinking_budget: Option<u32>,
}

fn default_prompt_enabled() -> bool { true }

fn default_headers_enabled() -> bool {
    true
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    #[serde(flatten)]
    pub model: Model,
    pub has_key: bool,
}
impl Model {
    pub fn view(&self) -> ModelView {
        let mut clean = self.clone();
        clean.api_key.clear();
        ModelView {
            model: clean,
            has_key: !self.api_key.is_empty(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenPricing {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}
impl TokenPricing {
    pub fn validate(&self) -> Result<(), String> {
        if [self.input,self.output,self.cache_read,self.cache_write].into_iter().flatten()
            .any(|v| !v.is_finite() || !(0.0..=1_000_000.0).contains(&v)) {
            return Err("Token 单价需为 0–1000000 的有限数值，或留空".into());
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    #[serde(default = "crate::dsh::default_profile")]
    pub dsh_profile: String,
    pub port: u16,
    pub timeout_seconds: u32,
    #[serde(default = "default_stream_idle_seconds")]
    pub stream_idle_seconds: u32,
    #[serde(default = "default_stream_grace_seconds")]
    pub stream_grace_seconds: u32,
    #[serde(default = "default_max_body_mb")]
    pub max_body_mb: usize,
    #[serde(default = "default_body_concurrency")]
    pub body_concurrency: usize,
    #[serde(default = "default_upstream_concurrency")]
    pub upstream_concurrency: usize,
    pub theme: String,
    pub log_limit: usize,
    #[serde(default)]
    pub detailed_logging: bool,
}
fn default_stream_idle_seconds() -> u32 { 600 }
fn default_stream_grace_seconds() -> u32 { 10 }
fn default_body_concurrency() -> usize { 16 }
fn default_upstream_concurrency() -> usize { 32 }
fn default_max_body_mb() -> usize {
    64
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            port: 8787,
            dsh_profile: crate::dsh::default_profile(),
            timeout_seconds: 120,
            stream_idle_seconds: default_stream_idle_seconds(),
            stream_grace_seconds: default_stream_grace_seconds(),
            max_body_mb: default_max_body_mb(),
            body_concurrency: default_body_concurrency(),
            upstream_concurrency: default_upstream_concurrency(),
            theme: "light".into(),
            log_limit: 200,
            detailed_logging: false,
        }
    }
}
impl Settings {
    pub fn stream_wait_seconds(&self) -> u64 {
        u64::from(self.stream_idle_seconds) + u64::from(self.stream_grace_seconds)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.dsh_profile.len() > 2048 || self.dsh_profile.chars().any(char::is_control) || self.port < 1024
            || !(5..=700).contains(&self.timeout_seconds)
            || !(5..=7200).contains(&self.stream_idle_seconds)
            || !(1..=60).contains(&self.stream_grace_seconds)
            || !(1..=128).contains(&self.max_body_mb)
            || !(1..=16).contains(&self.body_concurrency)
            || !(1..=32).contains(&self.upstream_concurrency)
            || !(10..=1000).contains(&self.log_limit)
            || !["light", "dark", "system"].contains(&self.theme.as_str())
        {
            return Err(
                "设置值超出范围：端口 1024–65535，排队/非流式超时 5–700 秒，流式无数据基准 5–7200 秒，宽限 1–60 秒，请求体 1–128 MiB，接收并发 1–16，转发并发 1–32，记录 10–1000 条"
                    .into(),
            );
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub pricing: TokenPricing,
    pub version: u32,
    pub models: Vec<Model>,
    pub settings: Settings,
    #[serde(default)]
    pub gateway_token: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            pricing: TokenPricing::default(),
            models: vec![],
            settings: Settings::default(),
            gateway_token: String::new(),
        }
    }
}

pub fn validate_model(m: &Model) -> Result<(), String> {
    if m.name.trim().is_empty()
        || m.name.len() > 120
        || m.model.trim().is_empty()
        || m.model.len() > 200
    {
        return Err("请填写显示名称和模型 ID，并控制长度".into());
    }
    if !["openai", "responses", "anthropic"].contains(&m.protocol.as_str()) {
        return Err("未知 API 协议".into());
    }
    let u = Url::parse(&m.base_url).map_err(|_| "上游地址不是有效 URL")?;
    let host = u.host_str().ok_or("上游地址缺少主机")?;
    let loopback = host == "localhost" || host == "127.0.0.1" || host == "[::1]";
    if (u.scheme() != "https" && !(u.scheme() == "http" && loopback))
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err("上游使用 HTTPS；本机测试可用 HTTP。地址中不要包含凭据、查询或片段".into());
    }
    if m.base_url.len() > 2048 {
        return Err(format!("服务器地址过长：当前 {} 字节，上限 2048 字节（UTF-8）", m.base_url.len()));
    }
    if m.api_key.len() > 8192 {
        return Err(format!("API Key 过长：当前 {} 字节，上限 8192 字节（UTF-8）", m.api_key.len()));
    }
    if m.api_key.chars().any(char::is_control) {
        return Err("API Key 含有控制字符，请移除换行、制表等不可见字符".into());
    }
    if m.prompt.len() > 128 * 1024 {
        return Err(format!("注入提示词过长：当前 {} 字节，上限 131072 字节（128 KiB，按 UTF-8 计算）", m.prompt.len()));
    }
    if m.context_window
        .is_some_and(|n| !(1024..=4_000_000).contains(&n))
        || m.max_output
            .is_some_and(|n| n == 0 || n > m.context_window.unwrap_or(4_000_000))
    {
        return Err("上下文需为 1024–4000000，最大输出需大于 0 且不超过上下文".into());
    }
    if !["default", "none", "low", "medium", "high", "xhigh", "max"].contains(&m.reasoning.as_str())
    {
        return Err("未知思考强度".into());
    }
    if m.notes.len() > 2048
        || serde_json::to_vec(&m.extra_params)
            .map_err(|_| "额外参数无效")?
            .len()
            > 32768
    {
        return Err("备注或额外参数过长".into());
    }
    if m.extra_params.keys().any(|k| {
        [
            "model",
            "messages",
            "input",
            "instructions",
            "system",
            "stream",
            "api_key",
            "apiKey",
            "headers",
            "base_url",
            "baseUrl",
            "url",
            "__proto__",
            "constructor",
            "prototype",
        ]
        .contains(&k.as_str())
    }) {
        return Err("额外参数不要覆盖模型、正文、流式开关、地址或鉴权字段".into());
    }
    if m.protocol == "anthropic"
        && m.thinking_budget
            .is_some_and(|n| n < 1024 || n >= m.max_output.unwrap_or(8192))
    {
        return Err("思考预算需至少 1024 且小于最大输出 Token".into());
    }
    if m.headers.len() > 32 {
        return Err("自定义请求头最多 32 项".into());
    }
    let mut seen = HashSet::new();
    for (name, value) in &m.headers {
        let lower = name.to_ascii_lowercase();
        let reserved = [
            "host",
            "authorization",
            "x-api-key",
            "proxy-authorization",
            "connection",
            "content-length",
            "transfer-encoding",
            "cookie",
            "set-cookie",
            "upgrade",
            "te",
            "trailer",
            "content-type",
            "x-oc-upstream",
        ];
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            || reserved.contains(&lower.as_str())
            || !seen.insert(lower)
            || value.len() > 8192
            || value.bytes().any(|b| b == 127 || b < 32)
        {
            return Err("请求头名称/值无效、重复或属于网关保留头；Key 请填入专用字段".into());
        }
    }
    Ok(())
}

pub struct Store {
    pub config: Config,
    path: PathBuf,
    _lock: std::fs::File,
}
impl Store {
    pub fn usage_path(&self) -> PathBuf { self.path.with_file_name("usage.dpapi") }
    pub fn open(directory: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&directory).map_err(|_| "创建应用数据目录失败")?;
        let lock = crate::vault::lock(&directory)?;
        let path = directory.join("config.dpapi");
        let mut config: Config = if path.exists() {
            serde_json::from_slice(&crate::vault::read(&path)?)
                .map_err(|_| "配置解析失败，原文件已保留")?
        } else {
            Config::default()
        };
        if config.version != 1 || config.models.len() > 100 {
            return Err("配置版本或模型数量不受支持".into());
        }
        config.settings.validate()?;
        config.pricing.validate()?;
        let mut names = HashSet::new();
        let mut ids = HashSet::new();
        for m in &config.models {
            validate_model(m)?;
            if !names.insert(m.name.clone()) || !ids.insert(m.id.clone()) {
                return Err("配置中存在重复模型".into());
            }
        }
        // Migrate a v1 configuration without ever writing a plaintext token.
        if config.gateway_token.is_empty() {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|_| "本机鉴权令牌生成失败")?;
            config.gateway_token = format!(
                "patrick_{}",
                bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
            crate::vault::write(
                &path,
                &serde_json::to_vec(&config).map_err(|_| "配置序列化失败")?,
            )?;
        }
        Ok(Self {
            config,
            path,
            _lock: lock,
        })
    }
    fn commit(&mut self, next: Config) -> Result<(), String> {
        let data = serde_json::to_vec(&next).map_err(|_| "配置序列化失败")?;
        crate::vault::write(&self.path, &data)?;
        self.config = next;
        Ok(())
    }
    pub fn resolve_input(&self, input: ModelInput) -> Result<Model, String> {
        let existing = match &input.id {
            Some(id) => Some(
                self.config
                    .models
                    .iter()
                    .find(|m| &m.id == id)
                    .ok_or("模型已不存在，请刷新")?,
            ),
            None => None,
        };
        if let Some(old) = existing {
            if !old.api_key.is_empty() && input.api_key.is_none() {
                let new_url =
                    Url::parse(input.base_url.trim()).map_err(|_| "上游地址不是有效 URL")?;
                let old_url = Url::parse(&old.base_url).map_err(|_| "原配置地址错误")?;
                if new_url.origin() != old_url.origin() {
                    return Err("更换上游服务商时，请重新填写或明确清除 Key".into());
                }
            }
        }
        let id = existing.map(|m| m.id.clone()).unwrap_or_else(new_id);
        let api_key = input
            .api_key
            .unwrap_or_else(|| existing.map(|m| m.api_key.clone()).unwrap_or_default());
        let m = Model {
            id: id.clone(),
            name: input.name.trim().into(),
            model: input.model.trim().into(),
            protocol: input.protocol,
            base_url: if input.use_full_url {
                input.base_url.trim().into()
            } else {
                input.base_url.trim().trim_end_matches('/').into()
            },
            api_key,
            headers: input.headers,
            enabled: input.enabled,
            context_window: input.context_window,
            max_output: input.max_output,
            reasoning: input.reasoning,
            prompt: input.prompt,
            prompt_enabled: input.prompt_enabled,
            use_full_url: input.use_full_url,
            notes: input.notes,
            headers_enabled: input.headers_enabled,
            zen_test_enabled: input.zen_test_enabled,
            extra_enabled: input.extra_enabled,
            extra_params: input.extra_params,
            thinking_budget: input.thinking_budget,
        };
        validate_model(&m)?;
        Ok(m)
    }
    pub fn save(&mut self, input: ModelInput) -> Result<String, String> {
        let m = self.resolve_input(input)?;
        let id = m.id.clone();
        if self
            .config
            .models
            .iter()
            .any(|other| other.id != id && other.name == m.name)
        {
            return Err("显示名称已存在，请使用不同名称".into());
        }
        let mut next = self.config.clone();
        if let Some(index) = next.models.iter().position(|m| m.id == id) {
            next.models[index] = m;
        } else {
            if next.models.len() >= 100 {
                return Err("最多保存 100 个模型".into());
            }
            next.models.push(m);
        }
        self.commit(next)?;
        Ok(id)
    }
    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let mut next = self.config.clone();
        let size = next.models.len();
        next.models.retain(|m| m.id != id);
        if size == next.models.len() {
            return Err("模型已不存在".into());
        }
        self.commit(next)
    }
    pub fn duplicate(&mut self, id: &str) -> Result<String, String> {
        let m = self
            .config
            .models
            .iter()
            .find(|m| m.id == id)
            .ok_or("模型已不存在")?
            .clone();
        let mut suffix = 1;
        let mut name = format!("{} 副本", m.name);
        while self.config.models.iter().any(|m| m.name == name) {
            suffix += 1;
            name = format!("{} 副本 {}", m.name, suffix);
        }
        self.save(ModelInput {
            id: None,
            name,
            model: m.model,
            protocol: m.protocol,
            base_url: m.base_url,
            api_key: Some(m.api_key),
            headers: m.headers,
            enabled: m.enabled,
            context_window: m.context_window,
            max_output: m.max_output,
            reasoning: m.reasoning,
            prompt: m.prompt,
            prompt_enabled: m.prompt_enabled,
            use_full_url: m.use_full_url,
            notes: m.notes,
            headers_enabled: m.headers_enabled,
            zen_test_enabled: m.zen_test_enabled,
            extra_enabled: m.extra_enabled,
            extra_params: m.extra_params,
            thinking_budget: m.thinking_budget,
        })
    }
    pub fn pricing(&mut self, pricing: TokenPricing) -> Result<(), String> {
        pricing.validate()?;
        let mut next = self.config.clone();
        next.pricing = pricing;
        self.commit(next)
    }
    pub fn settings(&mut self, settings: Settings) -> Result<(), String> {
        settings.validate()?;
        let mut next = self.config.clone();
        next.settings = settings;
        self.commit(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pricing_legacy_validation_and_encrypted_persistence() {
        let path = std::env::temp_dir().join(new_id());
        let mut store = Store::open(path.clone()).unwrap();
        let mut value = serde_json::to_value(sample()).unwrap(); value["id"] = serde_json::Value::Null;
        store.save(serde_json::from_value(value).unwrap()).unwrap();
        let models = serde_json::to_value(&store.config.models).unwrap();
        let token = store.config.gateway_token.clone();
        let mut legacy = serde_json::to_value(&store.config).unwrap();
        legacy.as_object_mut().unwrap().remove("pricing");
        drop(store);
        crate::vault::write(&path.join("config.dpapi"), &serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut store = Store::open(path.clone()).unwrap();
        assert!(store.config.pricing.input.is_none());
        let price = TokenPricing { input:Some(0.0), output:Some(2.5), cache_read:None, cache_write:Some(1000000.0) };
        store.pricing(price.clone()).unwrap();
        let disk = std::fs::read(path.join("config.dpapi")).unwrap();
        for bad in [-1.0, 1000000.1, f64::INFINITY, f64::NAN] {
            assert!(store.pricing(TokenPricing { input:Some(bad), ..price.clone() }).is_err());
            assert_eq!(std::fs::read(path.join("config.dpapi")).unwrap(),disk);
        }
        store.settings(store.config.settings.clone()).unwrap();
        drop(store);
        let store = Store::open(path.clone()).unwrap();
        assert_eq!(store.config.pricing.input,Some(0.0)); assert_eq!(store.config.pricing.output,Some(2.5));
        assert_eq!(store.config.pricing.cache_read,None); assert_eq!(store.config.pricing.cache_write,Some(1000000.0));
        assert_eq!(serde_json::to_value(&store.config.models).unwrap(),models);
        assert_eq!(store.config.gateway_token,token);
        let mut invalid=serde_json::to_value(&store.config).unwrap(); invalid["pricing"]["input"]=serde_json::json!(-1);
        drop(store);
        crate::vault::write(&path.join("config.dpapi"), &serde_json::to_vec(&invalid).unwrap()).unwrap();
        let original=std::fs::read(path.join("config.dpapi")).unwrap();
        assert!(Store::open(path.clone()).is_err());
        assert_eq!(std::fs::read(path.join("config.dpapi")).unwrap(),original);
        std::fs::remove_file(path.join("config.dpapi")).unwrap();
        std::fs::remove_file(path.join("instance.lock")).unwrap(); std::fs::remove_dir(path).unwrap();
        println!("PRICING_PASS legacy=EMPTY zero=VALID range=VALIDATED persistence=PASS models_keys_token=UNCHANGED invalid_disk=PRESERVED");
    }
    fn sample() -> Model {
        Model {
            id: "test".into(),
            name: "Test model".into(),
            model: "test-model".into(),
            protocol: "openai".into(),
            base_url: "https://example.invalid/v1".into(),
            api_key: "TEST_ONLY_KEY".into(),
            headers: BTreeMap::new(),
            enabled: true,
            context_window: Some(128000),
            max_output: Some(4096),
            reasoning: "default".into(),
            prompt: String::new(),
            prompt_enabled: true,
            use_full_url: false,
            notes: String::new(),
            headers_enabled: true,
            zen_test_enabled: false,
            extra_enabled: false,
            extra_params: BTreeMap::new(),
            thinking_budget: None,
        }
    }
    #[test]
    fn valid_models() {
        let mut m = sample();
        assert!(validate_model(&m).is_ok());
        m.base_url = "http://127.0.0.1:45678/v1".into();
        assert!(validate_model(&m).is_ok());
    }
    #[test]
    fn bad_urls() {
        for url in [
            "http://example.invalid/v1",
            "https://user:pass@example.invalid/v1",
            "https://example.invalid/?key=secret",
            "file:///C:/a",
            "https://example.invalid/#x",
        ] {
            let mut m = sample();
            m.base_url = url.into();
            assert!(validate_model(&m).is_err(), "{url}");
        }
    }
    #[test]
    fn reserved_headers() {
        for h in [
            "Authorization",
            "HOST",
            "X-Api-Key",
            "x-oc-upstream",
            "Cookie",
        ] {
            let mut m = sample();
            m.headers.insert(h.into(), "test".into());
            assert!(validate_model(&m).is_err());
        }
    }
    #[test]
    fn crlf_headers() {
        let mut m = sample();
        m.headers.insert("X-Test".into(), "a\r\nX-Evil: 1".into());
        assert!(validate_model(&m).is_err());
    }
    #[test]
    fn duplicate_headers() {
        let mut m = sample();
        m.headers.insert("X-Test".into(), "a".into());
        m.headers.insert("x-test".into(), "b".into());
        assert!(validate_model(&m).is_err());
    }
    #[test]
    fn secrets_not_returned() {
        let m = sample();
        let text = serde_json::to_string(&m.view()).unwrap();
        assert!(!text.contains("TEST_ONLY_KEY"));
        assert!(m.view().has_key);
    }
    #[test]
    fn output_limits() {
        let mut m = sample();
        m.max_output = m.context_window.map(|n| n + 1);
        assert!(validate_model(&m).is_err());
    }
    #[test]
    fn optional_limits_and_legacy_numbers() {
        let mut m = sample();
        let legacy = serde_json::to_value(&m).unwrap();
        let restored: Model = serde_json::from_value(legacy).unwrap();
        assert_eq!(restored.context_window, Some(128000));
        assert_eq!(restored.max_output, Some(4096));
        m.context_window = None;
        m.max_output = None;
        assert!(validate_model(&m).is_ok());
        let value = serde_json::to_value(&m).unwrap();
        assert!(value["contextWindow"].is_null() && value["maxOutput"].is_null());
        let restored: Model = serde_json::from_value(value).unwrap();
        assert_eq!(restored.max_output, None);
        m.max_output = Some(256000);
        assert!(validate_model(&m).is_ok());
        m.context_window = Some(128000);
        assert!(validate_model(&m).is_err());
        m.context_window = None;
        m.max_output = Some(0);
        assert!(validate_model(&m).is_err());
        m.max_output = None;
        m.protocol = "anthropic".into();
        m.thinking_budget = Some(8192);
        assert!(validate_model(&m).is_err());
        m.thinking_budget = Some(1024);
        assert!(validate_model(&m).is_ok());
    }
    #[test]
    fn timeout_700_boundary_and_persistence() {
        assert_eq!(Settings::default().timeout_seconds,120);
        for timeout in [5,120,600,700,601] {
            let settings=Settings{timeout_seconds:timeout,..Settings::default()};
            assert!(settings.validate().is_ok(),"TIMEOUT_700_REJECTED: {timeout}");
        }
        for timeout in [0,4,701,u32::MAX] {
            assert!(Settings{timeout_seconds:timeout,..Settings::default()}.validate().is_err());
        }
        let directory=std::env::temp_dir().join(new_id());
        let mut store=Store::open(directory.clone()).unwrap();
        let mut settings=store.config.settings.clone();settings.timeout_seconds=700;
        store.settings(settings.clone()).unwrap();
        settings.timeout_seconds=701;assert!(store.settings(settings).is_err());
        assert_eq!(store.config.settings.timeout_seconds,700);drop(store);
        let reopened=Store::open(directory.clone()).unwrap();
        assert_eq!(reopened.config.settings.timeout_seconds,700);drop(reopened);
        for name in ["config.dpapi","instance.lock"] {std::fs::remove_file(directory.join(name)).unwrap();}
        std::fs::remove_dir(directory).unwrap();
        println!("TIMEOUT_700_PASS min=5 max=700 rejected=4/701 default=120 persisted_reopen=700 invalid_save=PRESERVED");
    }
    #[test]
    fn stream_timeout_legacy_defaults_boundaries_and_persistence() {
        let mut json=serde_json::to_value(Settings::default()).unwrap();
        json.as_object_mut().unwrap().remove("streamIdleSeconds");
        json.as_object_mut().unwrap().remove("streamGraceSeconds");
        json["timeoutSeconds"]=serde_json::json!(610);
        let legacy:Settings=serde_json::from_value(json).unwrap();
        assert_eq!(legacy.timeout_seconds,610);assert_eq!(legacy.stream_wait_seconds(),610);
        for (base,grace,valid) in [(5,1,true),(600,10,true),(7200,60,true),(4,10,false),(7201,10,false),(600,0,false),(600,61,false),(u32::MAX,10,false)] {
            assert_eq!(Settings{stream_idle_seconds:base,stream_grace_seconds:grace,..legacy.clone()}.validate().is_ok(),valid);
        }
        let directory=std::env::temp_dir().join(new_id());
        let mut store=Store::open(directory.clone()).unwrap();
        let mut settings=legacy;settings.stream_idle_seconds=180;settings.stream_grace_seconds=10;
        store.settings(settings.clone()).unwrap();settings.stream_grace_seconds=0;assert!(store.settings(settings).is_err());drop(store);
        let reopened=Store::open(directory.clone()).unwrap();
        assert_eq!(reopened.config.settings.stream_wait_seconds(),190);assert_eq!(reopened.config.settings.timeout_seconds,610);drop(reopened);
        for name in ["config.dpapi","instance.lock"] {std::fs::remove_file(directory.join(name)).unwrap();}std::fs::remove_dir(directory).unwrap();
        println!("STREAM_SETTINGS_PASS legacy=600+10 old_value=PRESERVED boundaries=PASS dpapi_reopen=190 invalid_save=PRESERVED");
    }

    #[test]
    fn invalid_settings() {
        let s = Settings {
            port: 80,
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }
    #[test]
    fn legacy_settings_migrate_request_limit() {
        let old: Settings = serde_json::from_str(
            r#"{"port":8787,"timeoutSeconds":120,"theme":"light","logLimit":200}"#,
        )
        .unwrap();
        assert_eq!(old.max_body_mb, 64);
        assert!(old.validate().is_ok());
        for size in [0, 129, usize::MAX] {
            assert!(Settings {
                max_body_mb: size,
                ..Settings::default()
            }
            .validate()
            .is_err());
        }
        for size in [1, 64, 128] {
            assert!(Settings {
                max_body_mb: size,
                ..Settings::default()
            }
            .validate()
            .is_ok());
        }
    }
    #[test]
    fn zen_test_flag_legacy_defaults_and_encrypted_persistence() {
        let path = std::env::temp_dir().join(new_id());
        let mut value = serde_json::to_value(sample()).unwrap();
        value.as_object_mut().unwrap().remove("zenTestEnabled");
        let legacy: Model = serde_json::from_value(value.clone()).unwrap();
        assert!(!legacy.zen_test_enabled);
        value["id"] = serde_json::Value::Null;
        let mut input: ModelInput = serde_json::from_value(value).unwrap();
        assert!(!input.zen_test_enabled);
        let mut store = Store::open(path.clone()).unwrap();
        input.zen_test_enabled = true;
        let id = store.save(input).unwrap();
        let copy = store.duplicate(&id).unwrap();
        assert!(store.config.models.iter().all(|m| m.zen_test_enabled));
        drop(store);
        let mut store = Store::open(path.clone()).unwrap();
        assert_eq!(store.config.models.len(), 2);
        assert!(store.config.models.iter().all(|m| m.zen_test_enabled));
        let mut input: ModelInput = serde_json::from_value(serde_json::to_value(&store.config.models[0]).unwrap()).unwrap();
        input.zen_test_enabled = false;
        store.save(input).unwrap();
        drop(store);
        let mut store = Store::open(path.clone()).unwrap();
        assert!(!store.config.models[0].zen_test_enabled);
        assert!(store.config.models[1].zen_test_enabled);
        store.remove(&id).unwrap();
        store.remove(&copy).unwrap();
        drop(store);
        std::fs::remove_file(path.join("config.dpapi")).unwrap();
        std::fs::remove_file(path.join("instance.lock")).unwrap();
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn config_persistence() {
        let path = std::env::temp_dir().join(new_id());
        let mut s = Store::open(path.clone()).unwrap();
        let mut m = sample();
        m.context_window = None;
        m.max_output = None;
        let id = s
            .save(ModelInput {
                id: None,
                name: m.name,
                model: m.model,
                protocol: m.protocol,
                base_url: m.base_url,
                api_key: Some(m.api_key),
                headers: m.headers,
                enabled: true,
                context_window: m.context_window,
                max_output: m.max_output,
                reasoning: m.reasoning,
                prompt: m.prompt,
            prompt_enabled: m.prompt_enabled,
                use_full_url: false,
                notes: String::new(),
                headers_enabled: true,
                zen_test_enabled: false,
                extra_enabled: false,
                extra_params: BTreeMap::new(),
                thinking_budget: None,
            })
            .unwrap();
        assert!(Store::open(path.clone()).is_err());
        let copy = s.duplicate(&id).unwrap();
        s.remove(&copy).unwrap();
        drop(s);
        let mut s = Store::open(path.clone()).unwrap();
        assert_eq!(s.config.models[0].api_key, "TEST_ONLY_KEY");
        assert_eq!(s.config.models[0].context_window, None);
        assert_eq!(s.config.models[0].max_output, None);
        s.remove(&id).unwrap();
        assert_eq!(s.config.models.len(), 0);
        drop(s);
        std::fs::remove_file(path.join("config.dpapi")).unwrap();
        std::fs::remove_file(path.join("instance.lock")).unwrap();
        std::fs::remove_dir(path).unwrap();
    }
}

#[cfg(test)]
mod prompt_limit_regression {
    use super::*;
    fn sample() -> Model {
        serde_json::from_value(serde_json::json!({
            "id":"synthetic", "name":"synthetic", "model":"cn:deepseek-v4.1-flash",
            "protocol":"openai", "baseUrl":"https://example.invalid/v1",
            "apiKey":"SYNTHETIC_KEY_NOT_A_REAL_KEY", "headers":{}, "enabled":true,
            "contextWindow":null, "maxOutput":null, "reasoning":"default", "prompt":"",
            "promptEnabled":false
        })).unwrap()
    }
    #[test]
    fn utf8_boundaries_and_disabled_prompt() {
        let mut m=sample();
        for enabled in [false,true] {
            m.prompt_enabled=enabled;
            for text in ["中".repeat(17*1024),"a".repeat(131072),format!("{}aa","中".repeat(43690))] {
                m.prompt=text;assert!(validate_model(&m).is_ok());
            }
            m.prompt.push('a');
            let e=validate_model(&m).unwrap_err();
            assert!(e.starts_with("注入提示词过长") && e.contains("131073") && e.contains("131072"));
            assert!(!e.contains("Key") && !e.contains("地址"));
        }
        println!("PROMPT_128K_PASS bytes_52224=ACCEPTED bytes_131072=ACCEPTED bytes_131073=REJECTED utf8=CONSISTENT toggles=BOTH");
    }
    #[test]
    fn address_and_key_limits_unchanged_errors_specific() {
        let mut m=sample();let prefix="https://example.invalid/";
        m.base_url=format!("{prefix}{}","a".repeat(2048-prefix.len()));assert!(validate_model(&m).is_ok());
        m.base_url.push('a');let e=validate_model(&m).unwrap_err();assert!(e.starts_with("服务器地址过长") && e.contains("2049") && e.contains("2048"));
        m=sample();m.api_key="k".repeat(8192);assert!(validate_model(&m).is_ok());m.api_key.push('k');
        let e=validate_model(&m).unwrap_err();assert!(e.starts_with("API Key 过长") && e.contains("8193") && e.contains("8192"));
        for key in ["PRIVATE_SYNTHETIC\nKEY","PRIVATE_SYNTHETIC\rKEY","PRIVATE_SYNTHETIC\tKEY"] {
            m.api_key=key.into();let e=validate_model(&m).unwrap_err();
            assert!(e.starts_with("API Key 含有控制字符") && !e.contains("PRIVATE_SYNTHETIC"));
        }
        println!("OTHER_FIELDS_PASS url_limit=2048 key_limit=8192 controls=REJECTED errors=FIELD_SPECIFIC secrets=NOT_ECHOED");
    }
    #[test]
    fn prompt_51k_persistence_and_injection_three_protocols() {
        for protocol in ["openai","responses","anthropic"] {
            let directory=std::env::temp_dir().join(new_id());
            let mut store=Store::open(directory.clone()).unwrap();let mut m=sample();
            m.protocol=protocol.into();m.prompt="中".repeat(17*1024);
            for enabled in [true,false] {
                m.prompt_enabled=enabled;
                let mut value=serde_json::to_value(&m).unwrap();value["id"]=serde_json::Value::Null;
                if let Some(saved)=store.config.models.first(){value["id"]=serde_json::json!(saved.id);}
                store.save(serde_json::from_value(value).unwrap()).unwrap();
                drop(store);store=Store::open(directory.clone()).unwrap();
                let saved=&store.config.models[0];assert_eq!(saved.prompt,m.prompt);assert_eq!(saved.prompt_enabled,enabled);
                let body=if protocol=="responses" {serde_json::json!({"input":"synthetic"})}
                    else {serde_json::json!({"messages":[{"role":"user","content":"synthetic"}]})};
                let out=crate::protocol::prepare_body(saved,body).unwrap().to_string();
                assert_eq!(out.contains(&m.prompt),enabled);
            }
            drop(store);
            std::fs::remove_file(directory.join("config.dpapi")).unwrap();
            std::fs::remove_file(directory.join("instance.lock")).unwrap();std::fs::remove_dir(directory).unwrap();
        }
        println!("PROMPT_PERSIST_PASS bytes=52224 protocols=3 encrypted_reopen=EXACT enabled=INJECTED disabled=NOT_INJECTED");
    }
}
