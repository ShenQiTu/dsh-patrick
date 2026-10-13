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

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    #[serde(default = "crate::dsh::default_profile")]
    pub dsh_profile: String,
    pub port: u16,
    pub timeout_seconds: u32,
    #[serde(default = "default_max_body_mb")]
    pub max_body_mb: usize,
    pub theme: String,
    pub log_limit: usize,
}
fn default_max_body_mb() -> usize {
    64
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            port: 8787,
            dsh_profile: crate::dsh::default_profile(),
            timeout_seconds: 120,
            max_body_mb: default_max_body_mb(),
            theme: "light".into(),
            log_limit: 200,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.dsh_profile.len() > 2048 || self.dsh_profile.chars().any(char::is_control) || self.port < 1024
            || !(5..=600).contains(&self.timeout_seconds)
            || !(1..=128).contains(&self.max_body_mb)
            || !(10..=1000).contains(&self.log_limit)
            || !["light", "dark", "system"].contains(&self.theme.as_str())
        {
            return Err(
                "设置值超出范围：端口 1024–65535，超时 5–600 秒，请求体 1–128 MiB，记录 10–1000 条"
                    .into(),
            );
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
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
    if m.base_url.len() > 2048
        || m.api_key.len() > 8192
        || m.api_key.chars().any(char::is_control)
        || m.prompt.len() > 32768
    {
        return Err("地址、Key 或提示词长度/格式不正确".into());
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
