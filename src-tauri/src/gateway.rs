use crate::{
    config::{new_id, Config, Model, ModelInput, Store},
    protocol::{self, Metrics},
};
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    Json, Router,
};
use futures_util::{stream, StreamExt};
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{
        atomic::{AtomicU16, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    net::TcpListener,
    sync::{oneshot, OwnedSemaphorePermit, Semaphore},
};

mod test_request;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallRecord {
    pub id: String,
    pub model_id: String,
    pub name: String,
    pub protocol: String,
    pub source: String,
    pub host: String,
    pub started_at: u64,
    pub state: String,
    pub status: u16,
    #[serde(default)]
    pub zen_header_received: Option<bool>,
    #[serde(default)]
    pub zen_header_sent: Option<bool>,
    pub duration_ms: u64,
    pub first_token_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_tokens: Option<u64>,
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    pub tokens_per_second: Option<f64>,
    pub streamed: bool,
    pub bytes: u64,
    pub preview: String,
    pub message: String,
}

struct Running {
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}
pub struct Gateway {
    admission: Mutex<bool>,
    last_dsh_request: AtomicU64,
    pub store: Arc<Mutex<Store>>,
    client: reqwest::Client,
    control: tokio::sync::Mutex<Option<Running>>,
    port: AtomicU16,
    active: AtomicUsize,
    slots: Arc<Semaphore>,
    body_slots: Semaphore,
    records: Mutex<VecDeque<CallRecord>>,
    history: Mutex<crate::history::History>,
    results: Mutex<BTreeMap<String, CallRecord>>,
    tests: Mutex<HashMap<String, Option<oneshot::Sender<()>>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeView {
    pub last_dsh_request: u64,
    pub port: u16,
    pub active_requests: usize,
    pub calls: Vec<CallRecord>,
    pub test_results: BTreeMap<String, CallRecord>,
    pub history_version: u64,
    pub history_error: Option<String>,
}

pub struct Pause<'a>(&'a Gateway);
impl Drop for Pause<'_> {
    fn drop(&mut self) { if let Ok(mut paused)=self.0.admission.lock() { *paused=false; } }
}
impl Gateway {
    pub fn pause_if_idle(&self) -> Result<Pause<'_>, String> {
        let mut paused=self.admission.lock().map_err(|_| "请求状态锁异常")?;
        if self.active.load(Ordering::SeqCst)>0 { return Err("仍有请求进行中，请结束后再断开".into()); }
        *paused=true;
        Ok(Pause(self))
    }
    pub fn new(store: Arc<Mutex<Store>>) -> Result<Arc<Self>, String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(15))
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| "初始化 HTTPS 客户端失败")?;
        let history = crate::history::History::open(store.lock().map_err(|_| "配置锁异常")?.usage_path());
        Ok(Arc::new(Self {
            history: Mutex::new(history),
            admission: Mutex::new(false),
            last_dsh_request: AtomicU64::new(0),
            store,
            client,
            control: tokio::sync::Mutex::new(None),
            port: AtomicU16::new(0),
            active: AtomicUsize::new(0),
            slots: Arc::new(Semaphore::new(32)),
            body_slots: Semaphore::new(2),
            records: Mutex::new(VecDeque::new()),
            results: Mutex::new(BTreeMap::new()),
            tests: Mutex::new(HashMap::new()),
        }))
    }
    fn config(&self) -> Result<Config, String> {
        Ok(self.store.lock().map_err(|_| "配置锁异常")?.config.clone())
    }
    pub fn running(&self) -> bool {
        self.port.load(Ordering::SeqCst) != 0
    }
    pub async fn discover(
        &self,
        mut input: ModelInput,
        models_url: Option<String>,
    ) -> Result<Vec<String>, String> {
        // Resolve stored credentials only against the original upstream origin. No draft is saved.
        if input.name.trim().is_empty() {
            input.name = "Model discovery".into();
        }
        if input.model.trim().is_empty() {
            input.model = "discovery".into();
        }
        let model = self
            .store
            .lock()
            .map_err(|_| "配置锁异常")?
            .resolve_input(input)?;
        let request_url = protocol::upstream_url(&model)?;
        let base = request_url.strip_suffix(protocol::endpoint(&model.protocol));
        let target = match models_url.filter(|s| !s.trim().is_empty()) {
            Some(url) => url.trim().to_owned(),
            None => format!("{}models", base.ok_or("自定义完整路径请填写模型列表 URL")?),
        };
        let url = url::Url::parse(&target).map_err(|_| "模型列表 URL 无效")?;
        let original = url::Url::parse(&model.base_url).map_err(|_| "上游 URL 无效")?;
        if url.origin() != original.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("模型列表 URL 必须与上游同源，且不带凭据、查询或片段".into());
        }
        if self.running()
            && matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
            && url.port_or_known_default() == Some(self.port.load(Ordering::SeqCst))
        {
            return Err("模型列表地址指向网关自身".into());
        }
        let _slot = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| "请求繁忙")?;
        struct Active<'a>(&'a AtomicUsize);
        impl Drop for Active<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        {
            let paused=self.admission.lock().map_err(|_| "请求状态锁异常")?;
            if *paused { return Err("网关正在断开恢复，请稍后再试".into()); }
            self.active.fetch_add(1, Ordering::SeqCst);
        }
        let _active = Active(&self.active);
        let response = self
            .client
            .get(url)
            .headers(protocol::outgoing_headers(&model, &HeaderMap::new())?)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| "获取模型失败，请检查地址、网络和证书")?;
        if !response.status().is_success() {
            return Err(format!(
                "获取模型返回 HTTP {}；未跟随重定向，未显示上游错误正文",
                response.status().as_u16()
            ));
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "模型列表读取失败或超时")?;
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err("模型列表响应超过 1 MiB".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| "模型列表不是有效 JSON")?;
        let entries = value
            .get("data")
            .or_else(|| value.get("models"))
            .and_then(Value::as_array)
            .ok_or("响应缺少 data/models 数组")?;
        let ids: std::collections::BTreeSet<String> = entries
            .iter()
            .filter_map(|v| v.get("id").and_then(Value::as_str))
            .filter(|s| !s.is_empty() && s.len() <= 200 && !s.chars().any(char::is_control))
            .take(1000)
            .map(str::to_owned)
            .collect();
        if ids.is_empty() {
            return Err("接口未返回可用模型 ID；可手动填写服务商提供的模型名称".into());
        }
        Ok(ids.into_iter().collect())
    }
    pub fn statistics(&self) -> Result<Vec<crate::history::UsageRecord>, String> {
        Ok(self.history.lock().map_err(|_| "统计锁异常")?.records())
    }
    pub fn runtime(&self) -> Result<RuntimeView, String> {
        let (history_version, history_error) = { let h = self.history.lock().map_err(|_| "统计锁异常")?; (h.version, h.error.clone()) };
        Ok(RuntimeView {
            last_dsh_request: self.last_dsh_request.load(Ordering::SeqCst),
            history_version,
            history_error,
            port: self.port.load(Ordering::SeqCst),
            active_requests: self.active.load(Ordering::SeqCst),
            calls: self
                .records
                .lock()
                .map_err(|_| "记录锁异常")?
                .iter()
                .rev()
                .cloned()
                .collect(),
            test_results: self.results.lock().map_err(|_| "结果锁异常")?.clone(),
        })
    }
    pub async fn start(self: &Arc<Self>) -> Result<u16, String> {
        self.start_at(self.config()?.settings.port).await
    }
    pub(crate) async fn start_at(self: &Arc<Self>, port: u16) -> Result<u16, String> {
        let mut control = self.control.lock().await;
        if let Some(running) = control.as_ref() {
            if !running.task.is_finished() {
                return Ok(self.port.load(Ordering::SeqCst));
            }
        }
        if !self.config()?.models.iter().any(|m| m.enabled) {
            return Err("请先配置并启用至少一个模型".into());
        }
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .map_err(|_| format!("本机端口 {port} 已被占用或绑定失败；未改动其他进程"))?;
        let actual = listener
            .local_addr()
            .map_err(|_| "读取监听地址失败")?
            .port();
        let app = Router::new().fallback(proxy).with_state(self.clone());
        let (stop, stopped) = oneshot::channel();
        self.last_dsh_request.store(0, Ordering::SeqCst);
        self.port.store(actual, Ordering::SeqCst);
        let engine = self.clone();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await;
            engine.port.store(0, Ordering::SeqCst);
        });
        *control = Some(Running { stop, task });
        Ok(actual)
    }
    pub async fn stop(&self) -> Result<(), String> {
        let mut control = self.control.lock().await;
        if self.active.load(Ordering::SeqCst) > 0 {
            return Err("仍有请求或测试进行中，请等待结束或取消测试后关闭网关".into());
        }
        if let Some(running) = control.take() {
            let _ = running.stop.send(());
            running.task.await.map_err(|_| "等待网关关闭失败")?;
        }
        self.port.store(0, Ordering::SeqCst);
        Ok(())
    }
    pub fn clear_records(&self) -> Result<(), String> {
        if self.active.load(Ordering::SeqCst) > 0 {
            return Err("请求进行中，稍后再清空记录".into());
        }
        self.records.lock().map_err(|_| "记录锁异常")?.clear();
        Ok(())
    }
    pub fn save_model(&self, input: ModelInput) -> Result<String, String> {
        let tests = self.tests.lock().map_err(|_| "测试锁异常")?;
        if input.id.as_ref().is_some_and(|id| tests.contains_key(id)) {
            return Err("请先结束该模型的测试，再修改配置".into());
        }
        let id = self.store.lock().map_err(|_| "配置锁异常")?.save(input)?;
        self.invalidate_result(&id)?;
        Ok(id)
    }
    pub fn delete_model(&self, id: &str) -> Result<(), String> {
        let tests = self.tests.lock().map_err(|_| "测试锁异常")?;
        if tests.contains_key(id) {
            return Err("请先结束该模型的测试，再删除配置".into());
        }
        self.store.lock().map_err(|_| "配置锁异常")?.remove(id)?;
        self.invalidate_result(id)
    }
    pub fn invalidate_result(&self, id: &str) -> Result<(), String> {
        self.results.lock().map_err(|_| "结果锁异常")?.remove(id);
        Ok(())
    }
    pub fn cancel_test(&self, id: &str) -> Result<(), String> {
        if let Some(cancel) = self
            .tests
            .lock()
            .map_err(|_| "测试锁异常")?
            .get_mut(id)
            .and_then(Option::take)
        {
            let _ = cancel.send(());
        }
        Ok(())
    }
    pub async fn test(self: &Arc<Self>, id: &str) -> Result<CallRecord, String> {
        let (cancel, cancelled) = oneshot::channel();
        let (model, timeout) = {
            let mut tests = self.tests.lock().map_err(|_| "测试锁异常")?;
            if tests.contains_key(id) {
                return Err("这个模型正在测试".into());
            }
            let config = self.config()?;
            let model = config
                .models
                .iter()
                .find(|m| m.id == id)
                .ok_or("模型已不存在")?
                .clone();
            tests.insert(id.into(), Some(cancel));
            (model, config.settings.timeout_seconds)
        };
        let result = tokio::select! {
            result=self.test_inner(&model,timeout)=>result,
            _=cancelled=>Err("测试已取消".into()),
        };
        self.tests.lock().map_err(|_| "测试锁异常")?.remove(id);
        result
    }
    async fn test_inner(
        self: &Arc<Self>,
        model: &Model,
        timeout: u32,
    ) -> Result<CallRecord, String> {
        let mut guard = RecordGuard::new(self.clone(), model, "test", true)?;
        let incoming = match test_headers(model) {
            Ok(headers) => headers,
            Err(error) => {
                guard.finish("error", 0, &error);
                return Ok(guard.record.clone());
            }
        };
        let (response, zen_header) = match self
            .send(model, protocol::test_body(model), incoming, timeout)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                guard.finish("error", 0, &e);
                return Ok(guard.record.clone());
            }
        };
        guard.record.zen_header_sent = Some(zen_header);
        guard.record.status = response.status().as_u16();
        let success = response.status().is_success();
        guard.metrics = Metrics::new(is_sse(response.headers()));
        guard.record.streamed = is_sse(response.headers());
        if !success && !model.zen_test_enabled {
            guard.finish("error", guard.record.status, "上游返回非成功状态（错误正文未记录）");
            return Ok(guard.record.clone());
        }
        // Bounded, test-only buffer for terminal/error diagnostics. Proxy forwarding is unchanged.
        let mut response_body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    guard.feed(&bytes);
                    if guard.record.bytes > 2 * 1024 * 1024 {
                        guard.finish("error", if model.zen_test_enabled { guard.record.status } else { 200 }, "测试响应超过 2 MiB，已中止");
                        return Ok(guard.record.clone());
                    }
                    if model.zen_test_enabled { response_body.extend_from_slice(&bytes); }
                }
                Err(error) => {
                    guard.finish("error", guard.record.status, if !model.zen_test_enabled {
                        "读取上游响应失败或超时"
                    } else if error.is_timeout() {
                        "读取上游响应超时（尚未正常结束）"
                    } else {
                        "读取上游响应失败（连接中断或响应解码失败）"
                    });
                    return Ok(guard.record.clone());
                }
            }
        }
        guard.metrics.finish(guard.elapsed());
        let outcome = if model.zen_test_enabled {
            test_request::response_result(
                &response_body, guard.record.streamed, success, guard.record.status, &guard.metrics,
            )
        } else if guard.metrics.saw_text && !guard.metrics.api_error && guard.metrics.completed {
            Ok(())
        } else {
            Err("响应未包含可识别文本、缺少结束事件或上游返回流内错误".into())
        };
        let message = match &outcome {
            Ok(()) => "模型连接测试通过".to_owned(),
            Err(reason) => test_request::redact_reason(reason, &guard.secrets),
        };
        guard.finish(
            if outcome.is_ok() { "success" } else { "error" },
            guard.record.status,
            &message,
        );
        Ok(guard.record.clone())
    }
    async fn send(
        &self,
        model: &Model,
        body: Value,
        incoming: HeaderMap,
        timeout: u32,
    ) -> Result<(reqwest::Response, bool), String> {
        let url = protocol::upstream_url(model)?;
        let parsed = url::Url::parse(&url).map_err(|_| "上游地址无效")?;
        if self.running()
            && matches!(parsed.host_str(), Some("127.0.0.1") | Some("localhost"))
            && parsed.port_or_known_default() == Some(self.port.load(Ordering::SeqCst))
        {
            return Err("上游地址指向网关自身，请填写模型服务商的地址".into());
        }
        let body = protocol::prepare_body(model, body)?;
        let headers = protocol::outgoing_headers(model, &incoming)?;
        let zen_header = headers.contains_key("x-opencode-session");
        let response = self.client
            .post(url)
            .headers(headers)
            .json(&body)
            .timeout(Duration::from_secs(timeout.into()))
            .send()
            .await
            .map_err(|e| -> String {
                if e.is_timeout() {
                    "连接上游超时".into()
                } else if e.is_connect() {
                    "连接上游失败，请检查地址、网络和证书".into()
                } else {
                    "上游请求失败".into()
                }
            })?;
        Ok((response, zen_header))
    }
}

// Only the explicit model-test entry point calls this. Never synthesize DSH conversation headers.
fn test_headers(model: &Model) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    if model.zen_test_enabled {
        // Generated once per test_inner invocation; transport retries clone these same headers.
        let session = test_request::session_id()?;
        headers.insert("x-opencode-session", session.parse().map_err(|_| "测试用 Zen 会话头格式无效")?);
    }
    Ok(headers)
}

fn is_sse(headers: &HeaderMap) -> bool {
    headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"))
}
fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({"error":{"message":message,"type":"gateway_error"}})),
    )
        .into_response()
}
fn equal_secret(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.as_bytes()
        .iter()
        .zip(b.as_bytes())
        .fold(0u8, |r, (a, b)| r | (a ^ b))
        == 0
}
fn authenticate(headers: &HeaderMap, token: &str) -> bool {
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let api = headers.get("x-api-key").and_then(|v| v.to_str().ok());
    let values: Vec<_> = [bearer, api].into_iter().flatten().collect();
    !values.is_empty() && values.iter().all(|v| equal_secret(v, token))
}

async fn proxy(State(engine): State<Arc<Gateway>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let port = engine.port.load(Ordering::SeqCst);
    let expected_host = format!("127.0.0.1:{port}");
    if parts.headers.get("host").and_then(|v| v.to_str().ok()) != Some(expected_host.as_str())
        || parts.headers.contains_key("origin")
    {
        return error(StatusCode::FORBIDDEN, "仅接受本机客户端调用");
    }
    let config = match engine.config() {
        Ok(c) => c,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "配置读取失败"),
    };
    if !authenticate(&parts.headers, &config.gateway_token) {
        return error(StatusCode::UNAUTHORIZED, "本机网关鉴权失败");
    }
    if parts.uri.query().is_some() && !(parts.method == Method::POST && parts.uri.path()=="/v1/messages" && parts.uri.query()==Some("beta=true")) {
        return error(StatusCode::BAD_REQUEST, "本机 API 不接受查询参数");
    }
    let path = parts.uri.path();
    if parts.method == Method::GET && path == "/v1/models" {
        return Json(json!({"object":"list","data":config.models.iter().filter(|m|m.enabled).map(|m|json!({"id":m.id,"object":"model","owned_by":"dsh-patrick","name":m.name,"protocol":m.protocol})).collect::<Vec<_>>()})).into_response();
    }
    let protocol = match (parts.method.as_str(), path) {
        ("POST", "/v1/chat/completions") => "openai",
        ("POST", "/v1/responses") => "responses",
        ("POST", "/v1/messages") => "anthropic",
        _ => return error(StatusCode::NOT_FOUND, "未配置的 API 路径"),
    };
    if parts
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or("").trim())
        != Some("application/json")
        || parts.headers.contains_key("content-encoding")
    {
        return error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "请发送未压缩的 application/json 请求",
        );
    }
    // Bound simultaneous large-body buffering separately from long-lived SSE requests.
    let body_permit = match engine.body_slots.try_acquire() {
        Ok(permit) => permit,
        Err(_) => return error(StatusCode::TOO_MANY_REQUESTS, "请求体接收繁忙，请稍后重试"),
    };
    let bytes = match tokio::time::timeout(
        Duration::from_secs(15),
        to_bytes(body, config.settings.max_body_mb * 1024 * 1024),
    )
    .await
    {
        Ok(Ok(b)) => b,
        Ok(Err(_)) => {
            return error(
                StatusCode::PAYLOAD_TOO_LARGE,
                &format!("请求超过 {} MiB", config.settings.max_body_mb),
            )
        }
        Err(_) => return error(StatusCode::REQUEST_TIMEOUT, "读取请求超时"),
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return error(StatusCode::BAD_REQUEST, "JSON 格式无效"),
    };
    drop(bytes);
    drop(body_permit);
    let selected = match value.get("model").and_then(Value::as_str) {
        Some(m) => m,
        None => return error(StatusCode::BAD_REQUEST, "缺少模型 ID"),
    };
    let candidates: Vec<_> = config
        .models
        .iter()
        .filter(|m| m.enabled && (m.id == selected || m.name == selected || m.model == selected))
        .collect();
    if candidates.len() != 1 {
        return error(
            StatusCode::BAD_REQUEST,
            "模型不存在、未启用或存在同名歧义，请使用 /v1/models 返回的 ID",
        );
    }
    let model = candidates[0];
    if model.protocol != protocol {
        return error(StatusCode::BAD_REQUEST, "请求端点与该模型配置的协议不一致");
    }
    if let Err(message) = protocol::prepare_body(model, value.clone())
        .and_then(|_| protocol::outgoing_headers(model, &parts.headers))
    {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    let mut guard = match RecordGuard::new(engine.clone(), model, "gateway", false) {
        Ok(g) => g,
        Err(_) => {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "正在处理的请求过多，请稍后重试",
            )
        }
    };
    if parts.headers.get("x-dsh-patrick").and_then(|h|h.to_str().ok())==Some("1") {
        engine.last_dsh_request.store(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,Ordering::SeqCst);
    }
    guard.record.zen_header_received = Some(parts.headers.contains_key("x-opencode-session"));
    let (response, zen_header) = match engine
        .send(model, value, parts.headers, config.settings.timeout_seconds)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            guard.finish("error", 0, &e);
            return error(StatusCode::BAD_GATEWAY, &e);
        }
    };
    guard.record.zen_header_sent = Some(zen_header);
    let status = response.status();
    guard.record.status = status.as_u16();
    if !status.is_success() {
        guard.finish(
            "error",
            status.as_u16(),
            "上游返回非成功状态（错误正文未记录）",
        );
        return error(
            status,
            "上游返回非成功状态，请检查模型配置或查看上游服务状态",
        );
    }
    let content_type = response.headers().get("content-type").cloned();
    guard.record.streamed = is_sse(response.headers());
    guard.metrics = Metrics::new(guard.record.streamed);
    let upstream = response.bytes_stream();
    let stream = stream::unfold(
        (upstream, Some(guard)),
        |(mut upstream, mut guard)| async move {
            match upstream.next().await {
                Some(Ok(bytes)) => {
                    guard.as_mut().unwrap().feed(&bytes);
                    Some((Ok::<_, std::io::Error>(bytes), (upstream, guard)))
                }
                Some(Err(_)) => {
                    let g = guard.as_mut().unwrap();
                    g.finish("error", g.record.status, "上游流读取失败或超时");
                    Some((
                        Err(std::io::Error::other("upstream stream interrupted")),
                        (upstream, guard),
                    ))
                }
                None => {
                    let g = guard.as_mut().unwrap();
                    g.metrics.finish(g.elapsed());
                    let ok = !g.metrics.api_error && g.metrics.completed;
                    g.finish(
                        if ok { "success" } else { "error" },
                        g.record.status,
                        if ok {
                            "请求完成"
                        } else {
                            "上游流缺少结束事件或返回错误"
                        },
                    );
                    None
                }
            }
        },
    );
    let mut result = Response::new(Body::from_stream(stream));
    *result.status_mut() = status;
    if let Some(value) = content_type {
        result.headers_mut().insert("content-type", value);
    }
    result.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    result
}

struct RecordGuard {
    engine: Arc<Gateway>,
    record: CallRecord,
    start: Instant,
    metrics: Metrics,
    done: bool,
    secrets: Vec<String>,
    _permit: OwnedSemaphorePermit,
}
impl RecordGuard {
    fn new(
        engine: Arc<Gateway>,
        model: &Model,
        source: &str,
        streamed: bool,
    ) -> Result<Self, String> {
        let paused=engine.admission.lock().map_err(|_| "请求状态锁异常")?;
        if *paused { return Err("网关正在断开恢复".into()); }
        let permit = engine
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| "请求并发数达到上限")?;
        let config = engine.config()?;
        let record = CallRecord {
            id: new_id(),
            model_id: model.id.clone(),
            name: model.name.clone(),
            protocol: model.protocol.clone(),
            source: source.into(),
            host: url::Url::parse(&model.base_url)
                .map(|u| u.host_str().unwrap_or("").to_string())
                .unwrap_or_default(),
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            state: "running".into(),
            status: 0,
            zen_header_received: None,
            zen_header_sent: None,
            duration_ms: 0,
            first_token_ms: None,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            tokens_per_second: None,
            streamed,
            bytes: 0,
            preview: String::new(),
            message: "请求进行中".into(),
        };
        {
            let mut records = engine.records.lock().map_err(|_| "记录锁异常")?;
            records.push_back(record.clone());
            while records.len() > config.settings.log_limit {
                records.pop_front();
            }
        }
        engine.active.fetch_add(1, Ordering::SeqCst);
        drop(paused);
        let mut secrets: Vec<String> = model
            .headers
            .values()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect();
        if !model.api_key.is_empty() {
            secrets.push(model.api_key.clone());
        }
        secrets.push(config.gateway_token);
        Ok(Self {
            engine,
            record,
            start: Instant::now(),
            metrics: Metrics::new(streamed),
            done: false,
            secrets,
            _permit: permit,
        })
    }
    fn elapsed(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
    fn feed(&mut self, bytes: &[u8]) {
        self.record.bytes += bytes.len() as u64;
        self.metrics.feed(bytes, self.elapsed());
    }
    fn finish(&mut self, state: &str, status: u16, message: &str) {
        if self.done {
            return;
        }
        self.done = true;
        self.record.state = state.into();
        self.record.status = status;
        self.record.message = message.into();
        self.record.duration_ms = self.elapsed();
        self.record.first_token_ms = self.metrics.first_token_ms;
        self.record.input_tokens = self.metrics.input_tokens;
        self.record.output_tokens = self.metrics.output_tokens;
        self.record.cache_read_tokens = self.metrics.cache_read_tokens;
        self.record.cache_write_tokens = self.metrics.cache_write_tokens;
        if let Some(tokens) = self.metrics.output_tokens {
            let duration = self.record.duration_ms;
            if duration > 0 {
                self.record.tokens_per_second =
                    Some(((tokens as f64 * 1000.0 / duration as f64) * 10.0).round() / 10.0);
            }
        }
        if self.record.source == "test" {
            self.record.preview = protocol::redact_preview(&self.metrics.text, &self.secrets);
        }
        if let Ok(mut records) = self.engine.records.lock() {
            if let Some(record) = records.iter_mut().find(|r| r.id == self.record.id) {
                *record = self.record.clone();
            }
        }
        if let Ok(mut history) = self.engine.history.lock() { history.record(&self.record); }
        if self.record.source == "test" {
            if let Ok(mut results) = self.engine.results.lock() {
                results.insert(self.record.model_id.clone(), self.record.clone());
            }
        }
    }
}
impl Drop for RecordGuard {
    fn drop(&mut self) {
        if !self.done {
            self.finish("cancelled", self.record.status, "请求已取消或客户端已断开");
        }
        self.engine.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests;
