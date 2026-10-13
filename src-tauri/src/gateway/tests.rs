use super::*;
use crate::config::ModelInput;

#[tokio::test]
async fn zen_presence_is_reported_even_when_upstream_rejects() {
    let f = Fixture::new("openai").await;
    let port = f.engine.start_at(0).await.unwrap();
    let client = reqwest::Client::new();
    for (upstream_model, status) in [("failure", 401), ("stream-error", 200)] {
        let mut model = f.engine.config().unwrap().models[0].clone();
        model.model = upstream_model.into();
        model.zen_test_enabled = true;
        f.engine.save_model(serde_json::from_value(serde_json::to_value(&model).unwrap()).unwrap()).unwrap();
        let result = f.engine.test(&f.id()).await.unwrap();
        assert_eq!(result.state, "error");
        assert_eq!(result.status, status);
        assert_eq!(result.zen_header_received, None);
        assert_eq!(result.zen_header_sent, Some(true));
        let response = client.post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .bearer_auth(f.token()).header("x-opencode-session", "ses_123456789abc0123456789ABCD")
            .json(&protocol::test_body(&model)).send().await.unwrap();
        assert_eq!(response.status(), status); response.text().await.unwrap();
        let result = f.engine.runtime().unwrap().calls[0].clone();
        assert_eq!(result.state, "error");
        assert_eq!(result.zen_header_received, Some(true));
        assert_eq!(result.zen_header_sent, Some(true));
        assert!(!serde_json::to_string(&result).unwrap().contains("ses_123456789abc0123456789ABCD"));
    }
    f.cleanup().await;
}

#[tokio::test]
async fn zen_test_switch_isolated_from_dsh_forwarding_and_discovery() {
    for protocol_name in ["openai", "responses", "anthropic"] {
        let f = Fixture::new(protocol_name).await;
        let port = f.engine.start_at(0).await.unwrap();
        let model = f.engine.config().unwrap().models[0].clone();
        let mut input: ModelInput = serde_json::from_value(serde_json::to_value(&model).unwrap()).unwrap();
        let client = reqwest::Client::new();
        let dsh_session = "ses_123456789abc0123456789ABCD";
        let static_session = "ses_abcdef1234560123456789EFGH";
        let mut generated = std::collections::HashSet::new();
        for enabled in [false, true, false] {
            input.zen_test_enabled = enabled;
            input.headers.remove("x-opencode-session");
            f.engine.save_model(input.clone()).unwrap();
            for _ in 0..2 {
                let result = f.engine.test(&f.id()).await.unwrap();
                assert_eq!(result.state, "success");
                assert_eq!(result.zen_header_received, None);
                assert_eq!(result.zen_header_sent, Some(enabled));
                let seen = f.mock.seen.lock().unwrap().last().unwrap().clone();
                if enabled {
                    let header = seen.headers["x-opencode-session"].to_str().unwrap();
                    assert!(protocol::valid_zen_session(header));
                    assert!(generated.insert(header.to_owned()), "each test gets a distinct session");
                } else {
                    assert!(!seen.headers.contains_key("x-opencode-session"));
                }
            }
            // Normal DSH requests are unchanged with either switch position.
            for session in [None, Some(dsh_session)] {
                let mut request = client.post(format!("http://127.0.0.1:{port}/v1/{}", protocol::endpoint(protocol_name)))
                    .bearer_auth(f.token()).json(&protocol::test_body(&model));
                if let Some(session) = session { request = request.header("x-opencode-session", session); }
                let response = request.send().await.unwrap();
                assert_eq!(response.status(), 200);
                response.text().await.unwrap();
                let seen = f.mock.seen.lock().unwrap().last().unwrap().clone();
                assert_eq!(seen.headers.get("x-opencode-session").map(|v| v.to_str().unwrap()), session);
                let record = f.engine.runtime().unwrap().calls[0].clone();
                assert_eq!(record.zen_header_received, Some(session.is_some()));
                assert_eq!(record.zen_header_sent, Some(session.is_some()));
            }
            f.engine.discover(input.clone(), None).await.unwrap();
            assert!(!f.mock.seen.lock().unwrap().last().unwrap().headers.contains_key("x-opencode-session"));

            // Custom headers retain existing semantics; the incoming DSH session wins.
            input.headers.insert("x-opencode-session".into(), static_session.into());
            f.engine.save_model(input.clone()).unwrap();
            assert_eq!(f.engine.test(&f.id()).await.unwrap().state, "success");
            let test_header = f.mock.seen.lock().unwrap().last().unwrap().headers["x-opencode-session"].clone();
            if enabled { assert_ne!(test_header, static_session); } else { assert_eq!(test_header, static_session); }
            let response = client.post(format!("http://127.0.0.1:{port}/v1/{}", protocol::endpoint(protocol_name)))
                .bearer_auth(f.token()).header("x-opencode-session", dsh_session).json(&protocol::test_body(&model)).send().await.unwrap();
            assert_eq!(response.status(), 200); response.text().await.unwrap();
            let seen = f.mock.seen.lock().unwrap().last().unwrap().clone();
            assert_eq!(seen.headers.get_all("x-opencode-session").iter().count(), 1);
            assert_eq!(seen.headers["x-opencode-session"], dsh_session);
        }
        f.cleanup().await;
    }
}

#[tokio::test]
async fn default_reasoning_and_client_output_limits_survive_gateway_on_wire() {
    for (protocol_name, request, reasoning_fields, output_field) in [
        ("openai", json!({"messages":[],"reasoning_effort":"max","max_completion_tokens":2345}), vec!["reasoning_effort"], "max_completion_tokens"),
        ("responses", json!({"input":"test","reasoning":{"effort":"high","summary":"auto"},"max_output_tokens":2345}), vec!["reasoning"], "max_output_tokens"),
        ("anthropic", json!({"messages":[],"thinking":{"type":"adaptive"},"output_config":{"effort":"high"},"max_tokens":2345}), vec!["thinking","output_config"], "max_tokens"),
    ] {
        let f = Fixture::new(protocol_name).await;
        let port = f.engine.start_at(0).await.unwrap();
        let mut model = f.engine.config().unwrap().models[0].clone();
        model.context_window = None; model.max_output = None;
        model.zen_test_enabled = true;
        f.engine.save_model(serde_json::from_value(serde_json::to_value(&model).unwrap()).unwrap()).unwrap();
        let mut body = request.clone(); body["model"] = json!(f.id());
        let response = reqwest::Client::new().post(format!("http://127.0.0.1:{port}/v1/{}", protocol::endpoint(protocol_name)))
            .bearer_auth(f.token()).json(&body).send().await.unwrap();
        assert_eq!(response.status(), 200); response.text().await.unwrap();
        let seen = f.mock.seen.lock().unwrap().last().unwrap().clone();
        for field in reasoning_fields { assert_eq!(seen.body[field], request[field]); }
        assert_eq!(seen.body[output_field], 2345);
        assert!(!seen.headers.contains_key("x-opencode-session"));
        model.reasoning = "low".into();
        let overridden = protocol::prepare_body(&model, request.clone()).unwrap();
        match protocol_name {
            "openai" => assert_eq!(overridden["reasoning_effort"], "low"),
            "responses" => { assert_eq!(overridden["reasoning"]["effort"], "low"); assert_eq!(overridden["reasoning"]["summary"], "auto"); }
            _ => assert_eq!(overridden["output_config"]["effort"], "low"),
        }
        model.reasoning = "default".into(); model.extra_enabled = true;
        let (key, override_value) = match protocol_name {
            "openai" => ("reasoning_effort", json!("medium")),
            "responses" => ("reasoning", json!({"effort":"medium"})),
            _ => ("thinking", json!({"type":"enabled","budget_tokens":1024})),
        };
        model.extra_params.insert(key.into(), override_value.clone());
        assert_eq!(protocol::prepare_body(&model, request).unwrap()[key], override_value);
        f.cleanup().await;
    }
}

#[tokio::test]
async fn disconnect_admission_gate_and_anthropic_sdk_query() {
    let f=Fixture::new("anthropic").await;
    let port=f.engine.start_at(0).await.unwrap();
    let body=protocol::test_body(&f.engine.config().unwrap().models[0]);
    let client=reqwest::Client::new();
    let pause=f.engine.pause_if_idle().unwrap();
    let r=client.post(format!("http://127.0.0.1:{port}/v1/messages?beta=true")).bearer_auth(f.token()).json(&body).send().await.unwrap();
    assert_eq!(r.status(),429);assert!(f.mock.seen.lock().unwrap().is_empty());
    drop(pause);
    let r=client.post(format!("http://127.0.0.1:{port}/v1/messages?beta=true")).bearer_auth(f.token()).header("x-dsh-patrick","1").json(&body).send().await.unwrap();
    assert_eq!(r.status(),200);r.text().await.unwrap();
    assert!(f.engine.runtime().unwrap().last_dsh_request>0);
    assert!(!f.mock.seen.lock().unwrap()[0].headers.contains_key("x-dsh-patrick"));
    for query in ["beta=true&key=secret","beta=false","url=http://example.invalid"] {
        let r=client.post(format!("http://127.0.0.1:{port}/v1/messages?{query}")).bearer_auth(f.token()).json(&body).send().await.unwrap();
        assert_eq!(r.status(),400);
    }
    assert_eq!(f.mock.seen.lock().unwrap().len(),1);
    f.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Requires installed DSH source; run integration runner with explicit source path"]
async fn installed_dsh_to_rust_gateway_roundtrip() {
    assert!(std::env::var_os("DSH_PATRICK_DSH_SOURCE").is_some());
    let f=Fixture::new("openai").await;
    let port=f.engine.start_at(0).await.unwrap();
    let args=json!({"port":port,"token":f.token(),"models":[{"id":f.id(),"name":"Mock","model":"zen-free","protocol":"openai"}]}).to_string();
    let output=tokio::task::spawn_blocking(move|| {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("C:\\Program Files\\nodejs\\node.exe")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"),"/../tools/dsh-runtime.test.mjs")).arg(args).creation_flags(0x08000000).output().unwrap()
    }).await.unwrap();
    assert!(output.status.success(),"{} {}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
    let seen=f.mock.seen.lock().unwrap().clone();assert_eq!(seen.len(),3);
    assert!(seen.iter().all(|r|r.body["model"]=="mock-model" && r.headers["authorization"]=="Bearer UNIT_TEST_UPSTREAM_KEY"));
    assert_eq!(seen[0].headers["x-opencode-session"],seen[1].headers["x-opencode-session"]);
    assert_ne!(seen[0].headers["x-opencode-session"],seen[2].headers["x-opencode-session"]);
    assert!(seen.iter().all(|r|r.body.to_string().contains("INJECTED_SYSTEM_PROMPT")&&!r.headers.contains_key("x-dsh-patrick")));
    assert!(f.engine.runtime().unwrap().last_dsh_request>0);
    println!("DSH_RUST_ROUNDTRIP PASS real_adapter=YES zen_stable=YES upstream_key_isolated=YES requests=3 loopback_only=YES");
    f.cleanup().await;
}

#[tokio::test]
async fn per_model_prompt_switch_persists_without_clearing_text() {
    for protocol in ["openai", "responses", "anthropic"] {
        let fixture = Fixture::new(protocol).await;
        let id = fixture.id();
        let model = fixture.engine.config().unwrap().models[0].clone();
        let mut legacy = serde_json::to_value(&model).unwrap();
        legacy.as_object_mut().unwrap().remove("promptEnabled");
        let migrated: crate::config::Model = serde_json::from_value(legacy).unwrap();
        assert!(migrated.prompt_enabled, "old nonempty prompt retains its behavior");
        let mut input: ModelInput = serde_json::from_value(serde_json::to_value(&model).unwrap()).unwrap();
        input.prompt_enabled = false;
        fixture.engine.save_model(input.clone()).unwrap();
        assert_eq!(fixture.engine.test(&id).await.unwrap().state, "success");
        assert!(!fixture.mock.seen.lock().unwrap().last().unwrap().body.to_string().contains("INJECTED_SYSTEM_PROMPT"));
        let saved = fixture.engine.config().unwrap().models[0].clone();
        assert_eq!(saved.prompt, "INJECTED_SYSTEM_PROMPT");
        assert!(!saved.prompt_enabled);
        input.prompt_enabled = true;
        fixture.engine.save_model(input).unwrap();
        assert_eq!(fixture.engine.test(&id).await.unwrap().state, "success");
        assert!(fixture.mock.seen.lock().unwrap().last().unwrap().body.to_string().contains("INJECTED_SYSTEM_PROMPT"));
        let reopened = Gateway::new(fixture.engine.store.clone()).unwrap();
        assert_eq!(reopened.statistics().unwrap().len(), 2);
        assert!(reopened.statistics().unwrap().iter().all(|r| r.source == "test"));
        drop(reopened);
        fixture.cleanup().await;
    }
}

#[test]
fn cached_tokens_are_normalized_without_double_counting() {
    for (body, input, read, write) in [
        (json!({"usage":{"prompt_tokens":100,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":40}}}), 100, Some(40), None),
        (json!({"response":{"usage":{"input_tokens":100,"output_tokens":20,"input_tokens_details":{"cached_tokens":40}}}}), 100, Some(40), None),
        (json!({"usage":{"input_tokens":60,"output_tokens":20,"cache_read_input_tokens":30,"cache_creation_input_tokens":10}}), 100, Some(30), Some(10)),
        (json!({"usage":{"prompt_tokens":100,"completion_tokens":20,"prompt_cache_hit_tokens":40}}), 100, Some(40), None),
        (json!({"usage":{"prompt_tokens":100,"completion_tokens":20}}), 100, None, None),
    ] {
        let mut metrics = Metrics::new(false);
        metrics.feed(&serde_json::to_vec(&body).unwrap(), 10); metrics.finish(10);
        assert_eq!(metrics.input_tokens, Some(input));
        assert_eq!(metrics.output_tokens, Some(20));
        assert_eq!(metrics.cache_read_tokens, read);
        assert_eq!(metrics.cache_write_tokens, write);
    }
}

#[derive(Clone)]
struct Seen {
    path: String,
    headers: HeaderMap,
    body: Value,
}
#[derive(Clone)]
struct Mock {
    seen: Arc<Mutex<Vec<Seen>>>,
}

async fn mock_upstream(State(state): State<Mock>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 3 * 1024 * 1024).await.unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(json!({}));
    state.seen.lock().unwrap().push(Seen {
        path: parts.uri.path().into(),
        headers: parts.headers.clone(),
        body: value.clone(),
    });
    if parts.method == Method::GET {
        if parts.uri.path() == "/redirect/models" {
            return (StatusCode::TEMPORARY_REDIRECT, [("location", "/trap")], "").into_response();
        }
        return Json(
            json!({"data":[{"id":"mock-b"},{"id":"mock-a"},{"id":"mock-a"},{"id":"bad\nheader"}]}),
        )
        .into_response();
    }
    let model = value.get("model").and_then(Value::as_str).unwrap_or("");
    if model == "retry-once" && state.seen.lock().unwrap().iter().filter(|r| r.body["model"] == "retry-once").count() == 1 {
        return (StatusCode::SERVICE_UNAVAILABLE, "synthetic transient failure").into_response();
    }
    if model == "timeout-headers" { tokio::time::sleep(Duration::from_secs(2)).await; }
    if model == "timeout-body" {
        let body = stream::once(async {
            tokio::time::sleep(Duration::from_secs(2)).await;
            Ok::<_, std::io::Error>("data: [DONE]\n\n")
        });
        return ([ ("content-type", "text/event-stream") ], Body::from_stream(body)).into_response();
    }
    if model == "json-error" {
        return (StatusCode::FORBIDDEN, Json(json!({"error":{"message":"Access forbidden UNIT_TEST_UPSTREAM_KEY"}}))).into_response();
    }
    let diagnostic = match model {
        "diag-error" => Some("data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\nevent: error\ndata: {\"message\":\"Access forbidden UNIT_TEST_UPSTREAM_KEY\"}\n\ndata: [DONE]\n\n"),
        "diag-empty" => Some("data: [DONE]\n\n"),
        "diag-length" => Some("data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n"),
        "diag-malformed" => Some("data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\ndata: invalid\n\ndata: [DONE]\n\n"),
        _ => None,
    };
    if let Some(data) = diagnostic { return ([("content-type", "text/event-stream")], data).into_response(); }
    if model == "slow" {
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    if model == "redirect" {
        return (
            StatusCode::TEMPORARY_REDIRECT,
            [(
                "location",
                format!("http://{}/trap", parts.headers["host"].to_str().unwrap()),
            )],
            "redirect",
        )
            .into_response();
    }
    if model == "failure" {
        return (StatusCode::UNAUTHORIZED, "UNIT_TEST_UPSTREAM_KEY").into_response();
    }
    if model == "truncated" {
        return (
            [("content-type", "text/event-stream")],
            "data: {\"choices\":[{\"delta\":{\"content\":\"incomplete\"}}]}\n\n",
        )
            .into_response();
    }
    if model == "stream-error" {
        return (
            [("content-type", "text/event-stream")],
            "data: {\"error\":{\"message\":\"UNIT_TEST_UPSTREAM_KEY\"}}\n\ndata: [DONE]\n\n",
        )
            .into_response();
    }
    if value.get("stream") == Some(&json!(true)) {
        let data=match parts.uri.path(){
            "/v1/messages"=>"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":11,\"output_tokens\":0}}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello mock\"}}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":7}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            "/v1/responses"=>"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello mock\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":11,\"output_tokens\":7}}}\n\n",
            _=>"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello mock\"},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7}}\n\ndata: [DONE]\n\n",
        };
        let chunks = data
            .as_bytes()
            .chunks(7)
            .map(|c| Ok::<_, std::io::Error>(axum::body::Bytes::copy_from_slice(c)))
            .collect::<Vec<_>>();
        let mut response = Response::new(Body::from_stream(stream::iter(chunks)));
        response
            .headers_mut()
            .insert("content-type", "text/event-stream".parse().unwrap());
        return response;
    }
    Json(json!({"choices":[{"message":{"role":"assistant","content":"Hello mock"}}],"usage":{"prompt_tokens":11,"completion_tokens":7}})).into_response()
}

#[tokio::test]
async fn discovery_uses_saved_key_without_saving_or_cross_origin_leaks() {
    let fixture = Fixture::new("openai").await;
    let model = fixture.engine.config().unwrap().models[0].clone();
    let input = |url: String| -> ModelInput {
        let mut value = serde_json::to_value(&model).unwrap();
        value["apiKey"] = Value::Null;
        value["baseUrl"] = json!(url);
        value["name"] = json!("");
        value["model"] = json!("");
        serde_json::from_value(value).unwrap()
    };
    let ids = fixture
        .engine
        .discover(input(model.base_url.clone()), None)
        .await
        .unwrap();
    assert_eq!(ids, vec!["mock-a", "mock-b"]);
    assert_eq!(
        fixture.mock.seen.lock().unwrap()[0].headers["authorization"],
        "Bearer UNIT_TEST_UPSTREAM_KEY"
    );
    assert_eq!(
        fixture.engine.config().unwrap().models[0].name,
        "Mock model"
    );
    assert!(fixture
        .engine
        .discover(input("https://different.invalid/v1".into()), None)
        .await
        .is_err());
    assert!(fixture
        .engine
        .discover(
            input(model.base_url.clone()),
            Some("https://different.invalid/models".into())
        )
        .await
        .is_err());
    assert_eq!(fixture.mock.seen.lock().unwrap().len(), 1);
    assert!(fixture
        .engine
        .discover(
            input(model.base_url.clone()),
            Some(format!(
                "http://127.0.0.1:{}/redirect/models",
                fixture.upstream
            ))
        )
        .await
        .is_err());
    assert_eq!(fixture.mock.seen.lock().unwrap().len(), 2);
    assert_eq!(fixture.engine.runtime().unwrap().active_requests, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn full_url_headers_switch_extra_params_and_thinking_budget() {
    let fixture = Fixture::new("anthropic").await;
    let mut m = fixture.engine.config().unwrap().models[0].clone();
    m.use_full_url = true;
    m.base_url = format!("http://127.0.0.1:{}/custom/messages", fixture.upstream);
    assert_eq!(protocol::upstream_url(&m).unwrap(), m.base_url);
    m.headers_enabled = false;
    assert!(!protocol::outgoing_headers(&m, &HeaderMap::new())
        .unwrap()
        .contains_key("x-custom"));
    m.extra_enabled = true;
    m.extra_params.insert("temperature".into(), json!(0.25));
    m.thinking_budget = Some(1024);
    let body = protocol::prepare_body(&m, protocol::test_body(&m)).unwrap();
    assert_eq!(body["temperature"], 0.25);
    assert_eq!(body["thinking"]["budget_tokens"], 1024);
    assert!(body["max_tokens"].as_u64().unwrap() > 1024);
    m.extra_enabled = false;
    assert!(protocol::prepare_body(&m, protocol::test_body(&m))
        .unwrap()
        .get("temperature")
        .is_none());
    m.extra_params.insert("model".into(), json!("forbidden"));
    assert!(crate::config::validate_model(&m).is_err());
    fixture.cleanup().await;
}

#[tokio::test]
async fn optional_output_limits_do_not_send_null_or_override_clients() {
    let fixture = Fixture::new("openai").await;
    let mut m = fixture.engine.config().unwrap().models[0].clone();
    m.context_window = None;
    m.max_output = None;
    for (protocol_name, key, request) in [
        ("openai", "max_completion_tokens", json!({"messages":[]})),
        ("responses", "max_output_tokens", json!({"input":"test"})),
        ("anthropic", "max_tokens", json!({"messages":[]})),
    ] {
        m.protocol = protocol_name.into();
        m.max_output = None;
        let body = protocol::prepare_body(&m, request.clone()).unwrap();
        if protocol_name == "anthropic" {
            assert_eq!(body[key], 8192);
        } else {
            assert!(body.get(key).is_none());
        }
        let mut client = request.clone();
        client[key] = json!(2048);
        assert_eq!(
            protocol::prepare_body(&m, client.clone()).unwrap()[key],
            2048
        );
        m.max_output = Some(4096);
        assert_eq!(protocol::prepare_body(&m, request).unwrap()[key], 4096);
        assert_eq!(protocol::prepare_body(&m, client).unwrap()[key], 2048);
    }
    m.protocol = "openai".into();
    let chat = protocol::prepare_body(&m, json!({"messages":[],"max_tokens":123})).unwrap();
    assert_eq!(chat["max_tokens"], 123);
    assert!(chat.get("max_completion_tokens").is_none());
    m.protocol = "anthropic".into();
    m.max_output = None;
    m.thinking_budget = Some(1024);
    let body = protocol::prepare_body(&m, protocol::test_body(&m)).unwrap();
    assert_eq!(body["max_tokens"], 1536);
    assert!(protocol::prepare_body(&m, json!({"messages":[],"max_tokens":512})).is_err());
    fixture.cleanup().await;
}

#[tokio::test]
async fn legacy_models_migrate_and_extended_fields_persist() {
    let fixture = Fixture::new("openai").await;
    let mut value = serde_json::to_value(&fixture.engine.config().unwrap().models[0]).unwrap();
    for key in [
        "useFullUrl",
        "notes",
        "headersEnabled",
        "extraEnabled",
        "extraParams",
        "thinkingBudget",
    ] {
        value.as_object_mut().unwrap().remove(key);
    }
    let old: Model = serde_json::from_value(value.clone()).unwrap();
    assert!(old.headers_enabled);
    assert!(!old.extra_enabled && !old.use_full_url);
    value["notes"] = json!("saved note");
    value["extraEnabled"] = json!(true);
    value["extraParams"] = json!({"temperature":0.4});
    let input: ModelInput = serde_json::from_value(value).unwrap();
    let id = fixture.engine.save_model(input).unwrap();
    let copy = fixture.engine.store.lock().unwrap().duplicate(&id).unwrap();
    let config = fixture.engine.config().unwrap();
    assert_eq!(config.models[1].notes, "saved note");
    assert_eq!(config.models[1].extra_params["temperature"], 0.4);
    fixture.engine.delete_model(&copy).unwrap();
    fixture.cleanup().await;
}

struct Fixture {
    engine: Arc<Gateway>,
    mock: Mock,
    server: tokio::task::JoinHandle<()>,
    directory: std::path::PathBuf,
    upstream: u16,
}
impl Fixture {
    async fn new(protocol: &str) -> Self {
        let directory = std::env::temp_dir().join(format!("gateway-{}", new_id()));
        let store = Arc::new(Mutex::new(Store::open(directory.clone()).unwrap()));
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let upstream = listener.local_addr().unwrap().port();
        let mock = Mock {
            seen: Arc::new(Mutex::new(vec![])),
        };
        let app = Router::new()
            .fallback(mock_upstream)
            .with_state(mock.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let input = ModelInput {
            id: None,
            name: "Mock model".into(),
            model: "mock-model".into(),
            protocol: protocol.into(),
            base_url: format!("http://127.0.0.1:{upstream}/v1"),
            api_key: Some("UNIT_TEST_UPSTREAM_KEY".into()),
            headers: BTreeMap::from([("x-custom".into(), "fixture-value".into())]),
            enabled: true,
            context_window: Some(128000),
            max_output: Some(4096),
            reasoning: "default".into(),
            prompt: "INJECTED_SYSTEM_PROMPT".into(),
            prompt_enabled: true,
            use_full_url: false,
            notes: String::new(),
            headers_enabled: true,
            zen_test_enabled: false,
            extra_enabled: false,
            extra_params: BTreeMap::new(),
            thinking_budget: None,
        };
        store.lock().unwrap().save(input).unwrap();
        let engine = Gateway::new(store).unwrap();
        Self {
            engine,
            mock,
            server,
            directory,
            upstream,
        }
    }
    fn id(&self) -> String {
        self.engine.config().unwrap().models[0].id.clone()
    }
    fn token(&self) -> String {
        self.engine.config().unwrap().gateway_token
    }
    async fn cleanup(self) {
        self.engine.stop().await.unwrap();
        self.server.abort();
        let directory = self.directory.clone();
        drop(self);
        for name in ["config.dpapi", "instance.lock", "usage.dpapi"] {
            if directory.join(name).exists() { std::fs::remove_file(directory.join(name)).unwrap(); }
        }
        std::fs::remove_dir(directory).unwrap();
    }
}

#[tokio::test]
async fn three_protocols_stream_and_zen_headers() {
    for protocol in ["openai", "responses", "anthropic"] {
        let fixture = Fixture::new(protocol).await;
        let port = fixture.engine.start_at(0).await.unwrap();
        let model = fixture.engine.config().unwrap().models[0].clone();
        let mut body = protocol::test_body(&model);
        if protocol == "openai" {
            body["messages"] = json!([{"role":"system","content":"ORIGINAL_SYSTEM"},{"role":"user","content":"question"},{"role":"assistant","tool_calls":[{"id":"tool1","type":"function","function":{"name":"lookup","arguments":"{}"}}]},{"role":"tool","tool_call_id":"tool1","content":"answer"}]);
            body["tools"] = json!([{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}]);
        }
        if protocol == "responses" {
            body["instructions"] = json!("ORIGINAL_SYSTEM");
        }
        if protocol == "anthropic" {
            body["system"] = json!([{"type":"text","text":"ORIGINAL_SYSTEM","cache_control":{"type":"ephemeral"}}]);
        }
        let zen = "ses_123456789abc0123456789ABCD";
        let response = reqwest::Client::new()
            .post(format!(
                "http://127.0.0.1:{port}/v1/{}",
                protocol::endpoint(protocol)
            ))
            .bearer_auth(fixture.token())
            .header("x-opencode-session", zen)
            .header("x-oc-upstream", "https://example.invalid/trap")
            .header("cookie", "SHOULD_NOT_FORWARD")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let output = response.text().await.unwrap();
        assert!(output.contains("Hello mock"));
        let seen = fixture.mock.seen.lock().unwrap()[0].clone();
        assert_eq!(seen.path, format!("/v1/{}", protocol::endpoint(protocol)));
        assert_eq!(seen.headers["x-opencode-session"], zen);
        assert_eq!(seen.headers["x-custom"], "fixture-value");
        assert!(
            !seen.headers.contains_key("cookie") && !seen.headers.contains_key("x-oc-upstream")
        );
        assert_eq!(seen.body["model"], "mock-model");
        if protocol == "anthropic" {
            assert_eq!(seen.headers["x-api-key"], "UNIT_TEST_UPSTREAM_KEY");
            assert!(!seen.headers.contains_key("authorization"));
            assert_eq!(seen.body["system"][1]["text"], "ORIGINAL_SYSTEM");
        } else {
            assert_eq!(
                seen.headers["authorization"],
                "Bearer UNIT_TEST_UPSTREAM_KEY"
            );
        }
        if protocol == "openai" {
            assert_eq!(
                seen.body["messages"][0]["content"],
                "INJECTED_SYSTEM_PROMPT"
            );
            assert_eq!(seen.body["messages"][1]["content"], "ORIGINAL_SYSTEM");
            assert_eq!(
                seen.body["messages"][3]["tool_calls"],
                body["messages"][2]["tool_calls"]
            );
            assert_eq!(seen.body["tools"], body["tools"]);
        }
        if protocol == "responses" {
            assert_eq!(
                seen.body["instructions"],
                "INJECTED_SYSTEM_PROMPT\n\nORIGINAL_SYSTEM"
            );
        }
        let record = fixture.engine.runtime().unwrap().calls[0].clone();
        assert_eq!(record.state, "success");
        assert_eq!(record.input_tokens, Some(11));
        assert_eq!(record.output_tokens, Some(7));
        assert!(record.preview.is_empty());
        fixture.cleanup().await;
    }
}

#[tokio::test]
async fn authentication_paths_origins_and_model_routing() {
    let fixture = Fixture::new("openai").await;
    let port = fixture.engine.start_at(0).await.unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    let token = fixture.token();
    assert_eq!(
        client
            .get(format!("{base}/v1/models"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{base}/v1/models"))
            .bearer_auth("wrong")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{base}/v1/models"))
            .bearer_auth(&token)
            .header("origin", "https://evil.invalid")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{base}/v1/models"))
            .bearer_auth(&token)
            .header("host", "evil.invalid")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let models: Value = client
        .get(format!("{base}/v1/models"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(models["data"][0]["id"], fixture.id());
    assert!(!models.to_string().contains("UNIT_TEST"));
    for (path, body, expected) in [
        (
            "/v1/chat/completions",
            json!({"model":"unknown","messages":[]}),
            400,
        ),
        (
            "/v1/responses",
            json!({"model":fixture.id(),"input":"a"}),
            400,
        ),
        (
            "/sq/v1/chat/completions",
            json!({"model":fixture.id()}),
            404,
        ),
        ("/v1/chat/completions?upstream=evil", json!({}), 400),
    ] {
        assert_eq!(
            client
                .post(format!("{base}{path}"))
                .bearer_auth(&token)
                .json(&body)
                .send()
                .await
                .unwrap()
                .status()
                .as_u16(),
            expected
        );
    }
    assert_eq!(
        client
            .post(format!("{base}/v1/chat/completions"))
            .bearer_auth(&token)
            .header("x-opencode-session", "bad")
            .json(&json!({"model":fixture.id(),"messages":[]}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert!(fixture.mock.seen.lock().unwrap().is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn tests_record_details_and_block_redirects() {
    let fixture = Fixture::new("openai").await;
    let result = fixture.engine.test(&fixture.id()).await.unwrap();
    assert_eq!(result.state, "success");
    assert_eq!(result.preview, "Hello mock");
    assert_eq!(result.output_tokens, Some(7));
    assert!(result.first_token_ms.is_some());
    fixture.engine.store.lock().unwrap().config.models[0].model = "redirect".into();
    let result = fixture.engine.test(&fixture.id()).await.unwrap();
    assert_eq!(result.state, "error");
    assert_eq!(result.status, 307);
    assert_eq!(fixture.mock.seen.lock().unwrap().len(), 2);
    assert!(!fixture
        .mock
        .seen
        .lock()
        .unwrap()
        .iter()
        .any(|s| s.path == "/trap"));
    fixture.engine.store.lock().unwrap().config.models[0].model = "failure".into();
    let result = fixture.engine.test(&fixture.id()).await.unwrap();
    assert_eq!(result.status, 401);
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("UNIT_TEST_UPSTREAM_KEY"));
    fixture.cleanup().await;
}

#[tokio::test]
async fn cancellation_and_busy_shutdown() {
    let fixture = Fixture::new("openai").await;
    fixture.engine.store.lock().unwrap().config.models[0].model = "slow".into();
    fixture.engine.start_at(0).await.unwrap();
    let engine = fixture.engine.clone();
    let id = fixture.id();
    let handle = tokio::spawn(async move { engine.test(&id).await });
    for _ in 0..100 {
        if fixture.engine.active.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(fixture.engine.stop().await.is_err());
    assert!(fixture.engine.delete_model(&fixture.id()).is_err());
    fixture.engine.cancel_test(&fixture.id()).unwrap();
    assert!(handle.await.unwrap().is_err());
    assert_eq!(
        fixture.engine.runtime().unwrap().calls[0].state,
        "cancelled"
    );
    assert_eq!(fixture.engine.active.load(Ordering::SeqCst), 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn nonstream_usage_port_conflict_and_self_route() {
    let fixture = Fixture::new("openai").await;
    assert!(fixture.engine.start_at(fixture.upstream).await.is_err());
    let port = fixture.engine.start_at(0).await.unwrap();
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .bearer_auth(fixture.token())
        .json(&json!({"model":fixture.id(),"messages":[{"role":"user","content":"a"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let _: Value = response.json().await.unwrap();
    let record = fixture.engine.runtime().unwrap().calls[0].clone();
    assert_eq!(record.state, "success");
    assert_eq!(record.output_tokens, Some(7));
    assert_eq!(record.first_token_ms, None);
    fixture.engine.store.lock().unwrap().config.models[0].base_url =
        format!("http://127.0.0.1:{port}/v1");
    assert_eq!(
        fixture.engine.test(&fixture.id()).await.unwrap().state,
        "error"
    );
    fixture.cleanup().await;
}

#[test]
fn metrics_utf8_fragments_and_url_join() {
    let data="data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\ndata: {\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n";
    let mut metrics = Metrics::new(true);
    for byte in data.as_bytes() {
        metrics.feed(&[*byte], 12);
    }
    metrics.finish(22);
    assert_eq!(metrics.text, "你好");
    assert_eq!(metrics.output_tokens, Some(2));
    assert_eq!(metrics.first_token_ms, Some(12));
    assert!(protocol::valid_zen_session(
        "ses_123456789abc0123456789ABCD"
    ));
    assert!(!protocol::valid_zen_session(
        "ses_ABCDEF0123450123456789ABCD"
    ));
}

#[test]
fn multiline_sse_and_preview_redaction() {
    let mut metrics = Metrics::new(true);
    metrics.feed(b"data: {\"choices\":\ndata: [{\"delta\":{\"content\":\"MULTILINE\"}}]}\n\ndata: [DONE]\n\n",12);
    metrics.finish(20);
    assert_eq!(metrics.text, "MULTILINE");
    assert!(metrics.completed);
    assert_eq!(
        protocol::redact_preview("key=SECRET_FULL", &["SECRET_FULL".into()]),
        "key=[已脱敏]"
    );
    assert_eq!(
        protocol::redact_preview("key=SECRET_", &["SECRET_FULL".into()]),
        "key=[已脱敏]"
    );
    assert!(
        protocol::redact_preview(&"a".repeat(500), &["a".into()])
            .chars()
            .count()
            <= 500
    );
}

#[tokio::test]
async fn incomplete_streams_and_log_retention() {
    let fixture = Fixture::new("openai").await;
    for model in ["truncated", "stream-error"] {
        fixture.engine.store.lock().unwrap().config.models[0].model = model.into();
        let record = fixture.engine.test(&fixture.id()).await.unwrap();
        assert_eq!(record.state, "error");
        assert!(!serde_json::to_string(&record)
            .unwrap()
            .contains("UNIT_TEST_UPSTREAM_KEY"));
    }
    fixture.engine.store.lock().unwrap().config.models[0].model = "mock-model".into();
    fixture
        .engine
        .store
        .lock()
        .unwrap()
        .config
        .settings
        .log_limit = 10;
    for _ in 0..11 {
        fixture.engine.test(&fixture.id()).await.unwrap();
    }
    assert_eq!(fixture.engine.runtime().unwrap().calls.len(), 10);
    fixture.engine.clear_records().unwrap();
    assert!(fixture.engine.runtime().unwrap().calls.is_empty());
    fixture.engine.delete_model(&fixture.id()).unwrap();
    assert!(fixture.engine.runtime().unwrap().test_results.is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn payload_limits_disabled_and_ambiguous_models() {
    let fixture = Fixture::new("openai").await;
    fixture
        .engine
        .store
        .lock()
        .unwrap()
        .config
        .settings
        .max_body_mb = 1;
    let port = fixture.engine.start_at(0).await.unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
    let response = client
        .post(&url)
        .bearer_auth(fixture.token())
        .header("content-type", "application/json")
        .body("x".repeat(1024 * 1024 + 10))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 413);
    fixture.engine.store.lock().unwrap().config.models[0].enabled = false;
    let response = client
        .post(&url)
        .bearer_auth(fixture.token())
        .json(&json!({"model":fixture.id(),"messages":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    fixture.engine.store.lock().unwrap().config.models[0].enabled = true;
    let model_id = fixture.id();
    fixture
        .engine
        .store
        .lock()
        .unwrap()
        .duplicate(&model_id)
        .unwrap();
    let response = client
        .post(&url)
        .bearer_auth(fixture.token())
        .json(&json!({"model":"mock-model","messages":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    assert!(fixture.mock.seen.lock().unwrap().is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn long_context_over_two_mib_and_configurable_limit() {
    let fixture = Fixture::new("openai").await;
    assert_eq!(fixture.engine.config().unwrap().settings.max_body_mb, 64);
    let port = fixture.engine.start_at(0).await.unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let body = json!({"model":fixture.id(), "messages":[{"role":"user","content":"x".repeat(2*1024*1024+10)}]});
    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
    let response = client
        .post(&url)
        .bearer_auth(fixture.token())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let _: Value = response.json().await.unwrap();
    assert_eq!(
        fixture.mock.seen.lock().unwrap()[0].body["messages"][1]["content"],
        body["messages"][0]["content"]
    );
    fixture
        .engine
        .store
        .lock()
        .unwrap()
        .config
        .settings
        .max_body_mb = 1;
    // Test the exact rejection at the HTTP handler boundary. On Windows, rejecting
    // a still-uploading multi-MiB TCP body can surface as WSAECONNABORTED client-side.
    let response = proxy(
        State(fixture.engine.clone()),
        Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("host", format!("127.0.0.1:{port}"))
            .header("authorization", format!("Bearer {}", fixture.token()))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), 413);
    assert_eq!(fixture.mock.seen.lock().unwrap().len(), 1);
    let permits = fixture.engine.body_slots.acquire_many(2).await.unwrap();
    let response = client
        .post(&url)
        .bearer_auth(fixture.token())
        .json(&json!({"model":fixture.id(),"messages":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 429);
    drop(permits);
    fixture.cleanup().await;
}

#[tokio::test]
async fn zen_timestamp_prefix_matches_reference() {
    let f = Fixture::new("openai").await;
    let mut model = f.engine.config().unwrap().models[0].clone();
    model.zen_test_enabled = true;
    let before = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
    let headers = test_headers(&model).unwrap();
    let after = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
    let session = headers["x-opencode-session"].to_str().unwrap();
    assert!(protocol::valid_zen_session(session));
    let encoded = u64::from_str_radix(&session[4..16], 16).unwrap();
    let decoded_ms = ((!encoded) & 0xffffffffffff) >> 12;
    assert!((before & 0xfffffffff..=after & 0xfffffffff).contains(&decoded_ms),
        "Zen session prefix must encode current milliseconds modulo 36 bits, not random/hash bytes: decoded={decoded_ms} before={} after={}", before & 0xfffffffff, after & 0xfffffffff);
    f.cleanup().await;
}

#[tokio::test]
async fn zen_test_transport_retry_keeps_id_without_new_retry_policy_in_production() {
    let mut f = Fixture::new("openai").await;
    // Induce exactly one transport-layer replay on loopback. Production keeps its defaults.
    Arc::get_mut(&mut f.engine).unwrap().client = reqwest::Client::builder().no_proxy()
        .retry(reqwest::retry::for_host("127.0.0.1").no_budget().max_retries_per_request(1)
            .classify_fn(|r| if r.status() == Some(StatusCode::SERVICE_UNAVAILABLE) { r.retryable() } else { r.success() }))
        .build().unwrap();
    {
        let mut store = f.engine.store.lock().unwrap();
        store.config.models[0].model = "retry-once".into();
        store.config.models[0].zen_test_enabled = true;
    }
    let id = f.id();
    assert_eq!(f.engine.test(&id).await.unwrap().state, "success");
    {
        let seen = f.mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].headers["x-opencode-session"], seen[1].headers["x-opencode-session"]);
        assert_eq!(seen[0].body, seen[1].body);
        assert_eq!(seen[0].headers["authorization"], seen[1].headers["authorization"]);
    }
    assert_eq!(f.engine.test(&id).await.unwrap().state, "success");
    let seen = f.mock.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 3);
    assert_ne!(seen[0].headers["x-opencode-session"], seen[2].headers["x-opencode-session"]);
    f.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn zen_independent_concurrent_tests_and_case_insensitive_overrides() {
    let f = Fixture::new("openai").await;
    let mut model = f.engine.config().unwrap().models[0].clone();
    model.zen_test_enabled = true;
    model.headers.insert("X-OpenCode-Session".into(), "ses_111111111111STATIC12345678".into());
    model.headers.insert("User-Agent".into(), "custom-agent".into());
    f.engine.save_model(serde_json::from_value(serde_json::to_value(&model).unwrap()).unwrap()).unwrap();
    let original_id = f.id();
    let mut ids = vec![original_id.clone()];
    for _ in 0..7 { ids.push(f.engine.store.lock().unwrap().duplicate(&original_id).unwrap()); }
    let mut tasks = Vec::new();
    for id in ids {
        let engine = f.engine.clone();
        tasks.push(tokio::spawn(async move { engine.test(&id).await.unwrap() }));
    }
    for task in tasks { assert_eq!(task.await.unwrap().state, "success"); }
    let seen = f.mock.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 8);
    let mut ids = std::collections::HashSet::new();
    for request in seen {
        assert_eq!(request.headers.get_all("X-OPENCODE-SESSION").iter().count(), 1);
        let id = request.headers["x-opencode-session"].to_str().unwrap();
        assert!(protocol::valid_zen_session(id));
        assert!(ids.insert(id.to_owned()));
        assert_ne!(id, "ses_111111111111STATIC12345678");
        assert_eq!(request.headers["authorization"], "Bearer UNIT_TEST_UPSTREAM_KEY");
        assert_eq!(request.headers["user-agent"], "custom-agent");
        assert_eq!(request.headers["x-custom"], "fixture-value");
        assert!(!request.headers.contains_key("x-session-affinity"));
    }
    // Legacy/static duplicates are collapsed by HeaderMap; user-input validation also rejects them.
    model.headers.insert("x-opencode-session".into(), "ses_222222222222STATIC12345678".into());
    let incoming = test_headers(&model).unwrap();
    let merged = protocol::outgoing_headers(&model, &incoming).unwrap();
    assert_eq!(merged.get_all("x-opencode-session").iter().count(), 1);
    assert_eq!(merged["X-OpenCode-Session"], incoming["x-opencode-session"]);
    f.cleanup().await;
}

#[tokio::test]
async fn test_failure_reasons_and_timeout_release_test_slot() {
    let f = Fixture::new("openai").await;
    for (name, expected_status, reason) in [
        ("diag-error", 200, "Access forbidden [已脱敏]"),
        ("json-error", 403, "HTTP 403"),
        ("diag-empty", 200, "空响应"),
        ("diag-length", 200, "length"),
        ("diag-malformed", 200, "数据格式无效"),
        ("truncated", 200, "缺少模型结束事件"),
        ("timeout-headers", 0, "连接上游超时"),
        ("timeout-body", 200, "读取上游响应超时"),
    ] {
        {
            let mut store = f.engine.store.lock().unwrap();
            store.config.models[0].model = name.into();
            store.config.models[0].zen_test_enabled = true;
            store.config.settings.timeout_seconds = 1;
        }
        let result = f.engine.test(&f.id()).await.unwrap();
        assert_eq!(result.state, "error", "{name}");
        assert_eq!(result.status, expected_status, "{name}");
        assert!(result.message.contains(reason), "{name}: {}", result.message);
        assert!(!serde_json::to_string(&result).unwrap().contains("UNIT_TEST_UPSTREAM_KEY"));
        assert_eq!(f.engine.active.load(Ordering::SeqCst), 0);
        assert!(f.engine.tests.lock().unwrap().is_empty());
        f.engine.store.lock().unwrap().config.models[0].model = "mock-model".into();
        assert_eq!(f.engine.test(&f.id()).await.unwrap().state, "success", "slot must be reusable after {name}");
    }
    f.cleanup().await;
}

#[test]
fn test_stream_terminal_cases_and_auth_redaction() {
    use super::test_request::{response_result, redact_reason};
    let text = "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\n";
    for (tail, expected) in [
        ("data: [DONE]\n\n", None),
        ("data: {\"choices\":[{\"finish_reason\":\"stop\"}]}\n\n", None),
        ("data: {\"choices\":[{\"finish_reason\":\"error\"}]}\n\n", Some("error")),
        ("data: {\"choices\":[{\"finish_reason\":\"content_filter\"}]}\n\n", Some("content_filter")),
        ("event: response.failed\ndata: {\"response\":{\"error\":{\"message\":\"Forbidden\"}}}\n\n", Some("Forbidden")),
        ("data: {\"type\":\"response.incomplete\",\"response\":{\"incomplete_details\":{\"reason\":\"max_output_tokens\"}}}\n\n", Some("max_output_tokens")),
        ("data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n", Some("max_tokens")),
        ("data: [DONE]", Some("截断")),
        ("data: [DONE]\n\ndata: {\"error\":{\"message\":\"Late error\"}}\n\n", Some("Late error")),
    ] {
        let body = format!("{text}{tail}");
        let mut metrics = Metrics::new(true);
        for b in body.as_bytes() { metrics.feed(&[*b], 1); }
        metrics.finish(2);
        let result = response_result(body.as_bytes(), true, true, 200, &metrics);
        match expected { None => assert!(result.is_ok()), Some(reason) => assert!(result.unwrap_err().contains(reason)) }
    }
    assert_eq!(redact_reason("Forbidden SECRET", &["SECRET".into()]), "Forbidden [已脱敏]");
    for message in ["Authorization: Bearer UNKNOWN", "api_key=UNKNOWN", "x-api-key: UNKNOWN"] {
        assert!(!redact_reason(message, &[]).contains("UNKNOWN"));
    }
    assert!(redact_reason(&"x".repeat(900), &[]).len() <= 500);
}
