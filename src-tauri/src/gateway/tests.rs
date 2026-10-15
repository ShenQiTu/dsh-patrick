use super::*;
use crate::config::ModelInput;

#[tokio::test]
async fn call_history_opt_in_restart_and_actual_wire_details() {
    for protocol in ["openai","responses","anthropic"] {
        let f=Fixture::new(protocol).await;
        let first=f.engine.test(&f.id()).await.unwrap();
        assert!(!first.info.detailed);
        assert!(f.engine.call_detail(&first.id).unwrap()["payload"].is_null());
        let mut settings=f.engine.config().unwrap().settings;settings.detailed_logging=true;
        f.engine.save_settings(settings).unwrap();
        let r=f.engine.test(&f.id()).await.unwrap();
        assert_eq!(r.state,"success");assert!(r.info.ttfb_ms.is_some());
        assert_eq!(r.info.model.as_deref(),Some("mock-model"));
        assert!(r.info.request_url.as_ref().unwrap().ends_with(crate::protocol::endpoint(protocol)));
        let detail=f.engine.call_detail(&r.id).unwrap();
        assert_eq!(detail["payload"]["body"]["model"],"mock-model");
        assert!(detail["payload"]["response"].as_str().unwrap().contains("data:"));
        assert!(detail["payload"]["responseChunks"].as_u64().unwrap()>0);
        let text=detail.to_string();assert!(!text.contains("UNIT_TEST_UPSTREAM_KEY") && !text.contains("fixture-value"));
        let actual=f.mock.seen.lock().unwrap().last().unwrap().clone();
        assert_eq!(actual.body["model"],detail["payload"]["body"]["model"]);
        let reopened=Gateway::new(f.engine.store.clone()).unwrap();
        assert_eq!(reopened.runtime().unwrap().calls.len(),2);
        assert_eq!(reopened.call_detail(&r.id).unwrap()["payload"],detail["payload"]);
        assert!(reopened.runtime().unwrap().calls.iter().all(|r|r.preview.is_empty()));
        drop(reopened);
        let stats=f.engine.statistics().unwrap().len();
        f.engine.clear_call_details().unwrap();
        assert_eq!(f.engine.runtime().unwrap().calls.len(),2);
        assert_eq!(f.engine.statistics().unwrap().len(),stats);
        assert!(f.engine.call_detail(&r.id).unwrap()["payload"].is_null());
        f.engine.clear_records().unwrap();
        assert_eq!(f.engine.statistics().unwrap().len(),stats);
        assert!(Gateway::new(f.engine.store.clone()).unwrap().runtime().unwrap().calls.is_empty());
        f.cleanup().await;
    }
    println!("CALL_WIRE_PASS protocols=3 opt_in=YES actual_model=YES actual_request=YES keys=MASKED restart=PRESERVED details_clear=SUMMARY_UNCHANGED calls_clear=STATS_UNCHANGED");
}

#[tokio::test]
async fn statistics_clear_is_idle_only_atomic_and_preserves_configuration() {
    use std::os::windows::fs::OpenOptionsExt;
    let f=Fixture::new("openai").await;
    assert_eq!(f.engine.test(&f.id()).await.unwrap().state,"success");
    let config=std::fs::read(f.directory.join("config.dpapi")).unwrap();
    let path=f.directory.join("usage.dpapi");
    let history=std::fs::read(&path).unwrap();
    let before=f.engine.statistics_storage().unwrap();
    assert_eq!(before["calls"],1); assert!(before["bytes"].as_u64().unwrap()>0);
    for pending in [false,true] {
        let count=if pending {&f.engine.pending} else {&f.engine.active};
        count.fetch_add(1,Ordering::SeqCst);
        assert!(f.engine.clear_statistics().is_err());
        count.fetch_sub(1,Ordering::SeqCst);
        assert_eq!(std::fs::read(&path).unwrap(),history);
    }
    let reader=std::fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
    assert!(f.engine.clear_statistics().is_err()); drop(reader);
    assert_eq!(f.engine.statistics_storage().unwrap()["calls"],1);
    assert_eq!(f.engine.runtime().unwrap().calls.len(),1);
    assert_eq!(std::fs::read(&path).unwrap(),history);
    let version=f.engine.runtime().unwrap().history_version;
    f.engine.clear_statistics().unwrap();
    assert_eq!(f.engine.statistics_storage().unwrap()["calls"],0);
    assert_eq!(crate::history::History::open(path).records().len(),0);
    let runtime=f.engine.runtime().unwrap();
    assert!(runtime.calls.is_empty() && runtime.test_results.is_empty());
    assert!(runtime.history_version>version);
    assert_eq!(std::fs::read(f.directory.join("config.dpapi")).unwrap(),config);
    assert_eq!(f.engine.test(&f.id()).await.unwrap().state,"success");
    assert_eq!(f.engine.statistics_storage().unwrap()["calls"],1);
    f.cleanup().await;
    println!("STORAGE_PASS active_pending=REJECTED write_failure=PRESERVED cleared_on_disk=YES config_bytes=UNCHANGED next_request=SUCCESS");
}

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
    assert_eq!(r.status(),503);assert!(f.mock.seen.lock().unwrap().is_empty());
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
    if let Some(response) = idle_mock(model, parts.uri.path()) { return response; }
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
        self.engine.clear_records().unwrap();
        drop(self);
        if directory.join("call-details").exists() {std::fs::remove_dir(directory.join("call-details")).unwrap();}
        for name in ["config.dpapi", "instance.lock", "usage.dpapi", "calls.dpapi"] {
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
    let permits = fixture.engine.body_slots.clone().acquire_many_owned(16).await.unwrap();
    tokio::spawn(async move { tokio::time::sleep(Duration::from_millis(30)).await; drop(permits); });
    let response = client
        .post(&url)
        .bearer_auth(fixture.token())
        .json(&json!({"model":fixture.id(),"messages":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response.text().await.unwrap();
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
        assert_eq!(seen[0].headers["x-opencode-session-id"], seen[1].headers["x-opencode-session-id"]);
        assert_eq!(seen[0].headers["x-opencode-session"], seen[0].headers["x-opencode-session-id"]);
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
        assert_eq!(request.headers["x-opencode-session-id"], id);
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
        ("timeout-headers", 0, "本机等待响应头超时"),
        ("timeout-body", 200, "本机流式空闲超时"),
    ] {
        {
            let mut store = f.engine.store.lock().unwrap();
            store.config.models[0].model = name.into();
            store.config.models[0].zen_test_enabled = true;
            store.config.settings.timeout_seconds = 1;
            store.config.settings.stream_idle_seconds = 1;
            store.config.settings.stream_grace_seconds = 0;
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

async fn wait_queue_condition(check: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !check() { tokio::time::sleep(Duration::from_millis(5)).await; }
    }).await.expect("queue state reached within five seconds");
}

fn local_request(f: &Fixture) -> tokio::task::JoinHandle<(StatusCode, String)> {
    let port = f.engine.port.load(Ordering::SeqCst);
    let token = f.token();
    let body = json!({"model":f.id(),"messages":[{"role":"user","content":"SYNTHETIC_QUEUE_TEST"}],"stream":true});
    tokio::spawn(async move {
        let response = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(15)).build().unwrap()
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .bearer_auth(token).json(&body).send().await.unwrap();
        (response.status(), response.text().await.unwrap())
    })
}

#[tokio::test]
async fn configurable_concurrency_bounds_migration_persistence_and_idle_apply() {
    let f = Fixture::new("openai").await;
    let settings = f.engine.config().unwrap().settings;
    assert_eq!((settings.body_concurrency, settings.upstream_concurrency), (16,32));
    let mut legacy = serde_json::to_value(&settings).unwrap();
    legacy.as_object_mut().unwrap().remove("bodyConcurrency");
    legacy.as_object_mut().unwrap().remove("upstreamConcurrency");
    let migrated: crate::config::Settings = serde_json::from_value(legacy).unwrap();
    assert_eq!((migrated.body_concurrency, migrated.upstream_concurrency), (16,32));
    for (body,upstream,valid) in [(0,32,false),(17,32,false),(16,0,false),(16,33,false),(1,1,true),(8,20,true),(16,32,true)] {
        let mut input = settings.clone();input.body_concurrency=body;input.upstream_concurrency=upstream;
        assert_eq!(input.validate().is_ok(),valid);
        if valid {
            f.engine.save_settings(input).unwrap();
            assert_eq!((f.engine.body_slots.available_permits(),f.engine.slots.available_permits()),(body,upstream));
            let saved: Config = serde_json::from_slice(&crate::vault::read(&f.directory.join("config.dpapi")).unwrap()).unwrap();
            assert_eq!((saved.settings.body_concurrency,saved.settings.upstream_concurrency),(body,upstream));
            let reopened=Gateway::new(f.engine.store.clone()).unwrap();
            assert_eq!((reopened.body_slots.available_permits(),reopened.slots.available_permits()),(body,upstream));
        } else { assert!(f.engine.save_settings(input).is_err()); }
    }
    let pending=PendingRequest::new(&f.engine).unwrap();
    let mut changed=settings.clone();changed.body_concurrency=8;
    assert!(f.engine.save_settings(changed).unwrap_err().contains("空闲"));
    assert!(f.engine.pause_if_idle().is_err());
    assert!(f.engine.stop().await.is_err());
    drop(pending);
    assert_eq!(f.engine.config().unwrap().settings.body_concurrency,16);
    println!("SETTINGS_PASS ranges=1..16/1..32 defaults=16/32 legacy=PASS encrypted_persistence=PASS idle_apply=PASS busy_rejected=PASS");
    f.cleanup().await;
}

#[tokio::test]
async fn sixteen_slow_bodies_seventeenth_waits_then_resumes() {
    let f=Fixture::new("openai").await;
    let port=f.engine.start_at(0).await.unwrap();
    let mut releases=vec![];let mut tasks=vec![];
    for _ in 0..16 {
        let (release,wait)=oneshot::channel::<()>();releases.push(release);
        let body=json!({"model":f.id(),"messages":[],"stream":true}).to_string();
        let rest=body.as_bytes()[1..].to_vec();
        let token=f.token();
        tasks.push(tokio::spawn(async move {
            let chunks=stream::once(async { Ok::<_,std::io::Error>(vec![b'{']) }).chain(stream::once(async move {
                wait.await.unwrap();Ok::<_,std::io::Error>(rest)
            }));
            let r=reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().unwrap()
                .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
                .bearer_auth(token).header("content-type","application/json")
                .body(reqwest::Body::wrap_stream(chunks)).send().await.unwrap();
            assert_eq!(r.status(),200);assert!(r.text().await.unwrap().contains("Hello mock"));
        }));
    }
    wait_queue_condition(||f.engine.body_slots.available_permits()==0).await;
    assert_eq!(f.engine.slots.available_permits(),32);
    let seventeenth=local_request(&f);
    wait_queue_condition(||f.engine.body_queue.available_permits()==127).await;
    assert!(!seventeenth.is_finished());assert!(f.mock.seen.lock().unwrap().is_empty());
    releases.pop().unwrap().send(()).unwrap();
    let (status,body)=seventeenth.await.unwrap();assert_eq!(status,200);assert!(body.contains("Hello mock"));
    for release in releases {release.send(()).unwrap();}
    for task in tasks {task.await.unwrap();}
    wait_queue_condition(||f.engine.pending.load(Ordering::SeqCst)==0 && f.engine.active.load(Ordering::SeqCst)==0).await;
    assert_eq!((f.engine.body_slots.available_permits(),f.engine.body_queue.available_permits()),(16,128));
    println!("BODY_QUEUE_PASS simultaneous_uploads=16 request17=WAIT_THEN_200 released=16/128 upstream_unchanged=32");
    f.cleanup().await;
}

#[tokio::test]
async fn upstream_queue_shared_by_proxy_tests_discovery_and_cancel() {
    let f=Fixture::new("openai").await;f.engine.start_at(0).await.unwrap();
    let held=f.engine.slots.clone().acquire_many_owned(32).await.unwrap();
    let proxy=local_request(&f);
    wait_queue_condition(||f.engine.upstream_queue.available_permits()==127).await;
    assert_eq!(f.engine.body_slots.available_permits(),15,"parsed body retains its memory-admission permit");
    let id=f.id();let e=f.engine.clone();
    let test=tokio::spawn(async move {e.test(&id).await});
    let model=f.engine.config().unwrap().models[0].clone();
    let input:ModelInput=serde_json::from_value(serde_json::to_value(model).unwrap()).unwrap();
    let e=f.engine.clone();let discover=tokio::spawn(async move {e.discover(input,None).await});
    wait_queue_condition(||f.engine.upstream_queue.available_permits()==125).await;
    assert!(!proxy.is_finished() && !test.is_finished() && !discover.is_finished());
    assert!(f.engine.pause_if_idle().is_err());assert!(f.engine.stop().await.is_err());
    f.engine.cancel_test(&f.id()).unwrap();assert!(test.await.unwrap().err().unwrap().contains("取消"));
    assert_eq!(f.engine.upstream_queue.available_permits(),126);
    drop(held);
    assert_eq!(proxy.await.unwrap().0,200);assert_eq!(discover.await.unwrap().unwrap().len(),2);
    wait_queue_condition(||f.engine.pending.load(Ordering::SeqCst)==0 && f.engine.active.load(Ordering::SeqCst)==0).await;
    assert_eq!((f.engine.body_slots.available_permits(),f.engine.slots.available_permits(),f.engine.upstream_queue.available_permits()),(16,32,128));
    println!("UPSTREAM_QUEUE_PASS limit=32 proxy=WAIT_THEN_200 discovery=WAIT_THEN_OK cancelled_test=RELEASED memory_bound=PASS");
    f.cleanup().await;
}

#[tokio::test]
async fn queue_fifo_capacity_timeout_and_aborted_future_release() {
    let slots=Arc::new(Semaphore::new(1));let queue=Arc::new(Semaphore::new(128));
    let held=slots.clone().acquire_owned().await.unwrap();
    let s=slots.clone();let q=queue.clone();
    let cancelled=tokio::spawn(async move {queued_permit(&s,&q,tokio::time::Instant::now()+Duration::from_secs(5),"模型转发").await});
    wait_queue_condition(||queue.available_permits()==127).await;cancelled.abort();assert!(cancelled.await.unwrap_err().is_cancelled());
    assert_eq!(queue.available_permits(),128);
    let error=queued_permit(&slots,&queue,tokio::time::Instant::now()+Duration::from_millis(20),"请求体处理").await.unwrap_err();
    assert_eq!(error.0,504);assert!(error.1.contains("排队等待超时"));assert_eq!(queue.available_permits(),128);
    let all_waiters=queue.acquire_many(128).await.unwrap();
    assert_eq!(queued_permit(&slots,&queue,tokio::time::Instant::now()+Duration::from_secs(5),"请求体处理").await.unwrap_err().0,429);
    drop(all_waiters);
    let order=Arc::new(Mutex::new(vec![]));let mut tasks=vec![];
    for index in 0..3 {
        let s=slots.clone();let q=queue.clone();let o=order.clone();
        tasks.push(tokio::spawn(async move {
            let _p=queued_permit(&s,&q,tokio::time::Instant::now()+Duration::from_secs(5),"模型转发").await.unwrap();o.lock().unwrap().push(index);
        }));
        wait_queue_condition(||queue.available_permits()==127-index).await;
    }
    drop(held);for t in tasks {t.await.unwrap();}
    assert_eq!(*order.lock().unwrap(),vec![0,1,2]);assert_eq!((slots.available_permits(),queue.available_permits()),(1,128));
    println!("QUEUE_LIFECYCLE_PASS fifo=0,1,2 full=429 timeout=504 cancellation=RELEASED all_permits_restored=YES");
}

#[tokio::test]
async fn http_queue_timeout_releases_body_and_pending_at_both_stages() {
    let f=Fixture::new("openai").await;f.engine.start_at(0).await.unwrap();
    // Accelerate only this in-memory fixture; persistent settings still enforce 5..600 seconds.
    f.engine.store.lock().unwrap().config.settings.timeout_seconds=1;
    for body_stage in [true,false] {
        let held=if body_stage {f.engine.body_slots.clone().acquire_many_owned(16).await.unwrap()}
            else {f.engine.slots.clone().acquire_many_owned(32).await.unwrap()};
        let (status,message)=local_request(&f).await.unwrap();
        assert_eq!(status,504);assert!(message.contains(if body_stage {"请求体处理排队等待超时"} else {"模型转发排队等待超时"}));
        assert_eq!(f.engine.pending.load(Ordering::SeqCst),0);assert!(f.mock.seen.lock().unwrap().is_empty());
        drop(held);
        assert_eq!((f.engine.body_slots.available_permits(),f.engine.slots.available_permits()),(16,32));
    }
    assert_eq!(local_request(&f).await.unwrap().0,200);
    println!("HTTP_TIMEOUT_PASS body=504 upstream=504 no_upstream_sent=YES next_request=200 released=16/32");
    f.cleanup().await;
}

#[tokio::test]
async fn synthetic_bursts_respect_configured_upstream_limit_and_queue() {
    let f=Fixture::new("openai").await;f.engine.start_at(0).await.unwrap();
    f.engine.store.lock().unwrap().config.models[0].model="slow".into();
    for (count,body_limit,upstream_limit) in [(10,16,32),(20,16,32),(30,16,32),(99,16,32),(30,8,20)] {
        let mut settings=f.engine.config().unwrap().settings;settings.body_concurrency=body_limit;settings.upstream_concurrency=upstream_limit;
        f.engine.save_settings(settings).unwrap();
        let tasks:Vec<_>=(0..count).map(|_|local_request(&f)).collect();let mut maximum=0;
        while tasks.iter().any(|t|!t.is_finished()) {
            let active=f.engine.active.load(Ordering::SeqCst);maximum=maximum.max(active);assert!(active<=upstream_limit);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        for task in tasks {let (status,body)=task.await.unwrap();assert_eq!(status,200);assert!(body.contains("Hello mock"));assert!(body.contains("[DONE]"));}
        assert_eq!(maximum,upstream_limit.min(count));
        assert_eq!(f.engine.pending.load(Ordering::SeqCst),0);
        println!("BURST_PASS input={count} body_limit={body_limit} upstream_limit={upstream_limit} completed={count} max_active={maximum} rejected=0 synthetic_loopback=YES");
    }
    f.cleanup().await;
}

#[tokio::test]
async fn client_disconnect_removes_waiters_at_both_stages() {
    let f=Fixture::new("openai").await;f.engine.start_at(0).await.unwrap();
    for body_stage in [true,false] {
        let held=if body_stage {f.engine.body_slots.clone().acquire_many_owned(16).await.unwrap()}
            else {f.engine.slots.clone().acquire_many_owned(32).await.unwrap()};
        let request=local_request(&f);
        let queue=if body_stage {&f.engine.body_queue} else {&f.engine.upstream_queue};
        wait_queue_condition(||queue.available_permits()==127).await;
        request.abort();assert!(request.await.unwrap_err().is_cancelled());
        wait_queue_condition(||f.engine.pending.load(Ordering::SeqCst)==0).await;
        assert_eq!(queue.available_permits(),128);assert!(f.mock.seen.lock().unwrap().is_empty());
        drop(held);
    }
    assert_eq!(local_request(&f).await.unwrap().0,200);
    println!("HTTP_DISCONNECT_PASS body_waiter=REMOVED upstream_waiter=REMOVED no_replay=YES next_request=200");
    f.cleanup().await;
}

// Synthetic upstream: regular chunks, heartbeat-only waits, silence and a real 615-second stream.
fn idle_mock(model: &str, path: &str) -> Option<Response> {
    let (count, interval, heartbeat, terminal) = match model {
        "idle-active" => (8, 250, false, true),
        "idle-heartbeat" => (8, 250, true, true),
        "idle-empty-heartbeat" => (8, 250, true, false),
        "idle-long" => (124, 5000, false, true),
        _ => return None,
    };
    let (chunk, end) = if path.ends_with("/messages") {
        ("data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"OK\"}}\n\n", "data: {\"type\":\"message_stop\"}\n\n")
    } else if path.ends_with("/responses") {
        ("data: {\"type\":\"response.output_text.delta\",\"delta\":\"OK\"}\n\n", "data: {\"type\":\"response.completed\",\"response\":{}}\n\n")
    } else {
        ("data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\n", "data: [DONE]\n\n")
    };
    let stream = stream::unfold(0, move |i| async move {
        if i > count { return None; }
        if i > 0 && i < count {tokio::time::sleep(Duration::from_millis(interval)).await;}
        let text = if i == count {
            if terminal {format!("{chunk}{end}")} else {String::new()}
        } else if heartbeat {": keep-alive\n\n".to_owned()} else {chunk.to_owned()};
        Some((Ok::<_,std::io::Error>(text),(i+1)))
    });
    Some(([("content-type","text/event-stream")],Body::from_stream(stream)).into_response())
}

#[tokio::test]
async fn stream_idle_repro_active_stream_outlives_old_total() {
    let f=Fixture::new("openai").await;
    {let mut store=f.engine.store.lock().unwrap();store.config.settings.timeout_seconds=1;store.config.models[0].model="idle-active".into();}
    let start=Instant::now();let result=f.engine.test(&f.id()).await.unwrap();
    let elapsed=start.elapsed().as_millis();
    println!("ACTIVE_STREAM_OBSERVED state={} elapsed_ms={} message={}",result.state,elapsed,result.message);
    assert_eq!(result.state,"success","ACTIVE_STREAM_WAS_CUT_BY_TOTAL_TIMEOUT");
    assert!(elapsed>1500);assert_eq!(f.mock.seen.lock().unwrap().len(),1);
    f.cleanup().await;
    println!("ACTIVE_STREAM_PASS continuous_output_exceeds_old_total=YES requests=1");
}

#[tokio::test]
async fn stream_idle_proxy_heartbeat_three_protocols_and_nonstream_preserved() {
    for proto in ["openai","responses","anthropic"] {
        let f=Fixture::new(proto).await;let port=f.engine.start_at(0).await.unwrap();
        {let mut s=f.engine.store.lock().unwrap();s.config.settings.timeout_seconds=1;s.config.settings.stream_idle_seconds=1;s.config.settings.stream_grace_seconds=0;s.config.models[0].model="idle-heartbeat".into();}
        let model=f.engine.config().unwrap().models[0].clone();
        let client=reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
        let request=protocol::test_body(&model);
        let url=format!("http://127.0.0.1:{port}/v1/{}",protocol::endpoint(proto));
        let r=client.post(&url).bearer_auth(f.token()).json(&request).send().await.unwrap();
        assert_eq!(r.status(),200);let text=r.text().await.unwrap();assert!(text.contains("keep-alive") && text.contains("OK"));
        wait_queue_condition(||f.engine.active.load(Ordering::SeqCst)==0).await;
        assert_eq!(f.engine.runtime().unwrap().calls[0].state,"success");
        assert_eq!(f.mock.seen.lock().unwrap().len(),1);
        // A real non-stream request retains its old total response budget, even if a server returns SSE.
        let mut request=request;request["stream"]=json!(false);
        let r=client.post(&url).bearer_auth(f.token()).json(&request).send().await.unwrap();
        assert!(r.text().await.is_err());
        wait_queue_condition(||f.engine.active.load(Ordering::SeqCst)==0).await;
        assert_eq!(f.engine.runtime().unwrap().calls[0].state,"error");
        assert_eq!(f.engine.slots.available_permits(),32);
        f.cleanup().await;
    }
    println!("STREAM_POLICY_PASS protocols=3 heartbeat_resets=YES nonstream_total=PRESERVED no_retries=YES slots=RELEASED");
}

#[tokio::test]
async fn stream_idle_grace_errors_new_requests_and_cancel() {
    let f=Fixture::new("openai").await;
    {let mut s=f.engine.store.lock().unwrap();s.config.settings.timeout_seconds=1;s.config.settings.stream_idle_seconds=1;s.config.settings.stream_grace_seconds=2;}
    // Two-second header wait succeeds with 1+2 seconds grace, despite the old 1-second total.
    f.engine.store.lock().unwrap().config.models[0].model="timeout-headers".into();
    assert_eq!(f.engine.test(&f.id()).await.unwrap().state,"success");
    {let mut s=f.engine.store.lock().unwrap();s.config.settings.stream_grace_seconds=0;s.config.models[0].model="timeout-body".into();}
    let start=Instant::now();let r=f.engine.test(&f.id()).await.unwrap();
    assert_eq!(r.state,"error");assert_eq!(r.info.error_kind.as_deref(),Some("stream_idle_timeout"));
    assert!(start.elapsed()>=Duration::from_millis(900) && start.elapsed()<Duration::from_secs(2));
    assert_eq!(f.engine.slots.available_permits(),32);
    f.engine.store.lock().unwrap().config.models[0].model="idle-empty-heartbeat".into();
    let r=f.engine.test(&f.id()).await.unwrap();assert_eq!(r.state,"error");assert!(!r.message.starts_with("本机流式空闲超时"));
    f.engine.store.lock().unwrap().config.models[0].model="stream-error".into();
    let seen=f.mock.seen.lock().unwrap().len();let r=f.engine.test(&f.id()).await.unwrap();
    assert_eq!(r.state,"error");assert_eq!(r.status,200);assert_eq!(f.mock.seen.lock().unwrap().len(),seen+1);
    // A new independent call is not bound to any deadline from the previous error.
    f.engine.store.lock().unwrap().config.models[0].model="idle-active".into();
    assert_eq!(f.engine.test(&f.id()).await.unwrap().state,"success");
    f.engine.store.lock().unwrap().config.models[0].model="idle-long".into();
    let engine=f.engine.clone();let id=f.id();let task=tokio::spawn(async move {engine.test(&id).await});
    wait_queue_condition(||f.engine.active.load(Ordering::SeqCst)==1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;f.engine.cancel_test(&f.id()).unwrap();
    assert!(task.await.unwrap().err().unwrap().contains("取消"));
    assert!(f.engine.tests.lock().unwrap().is_empty());assert_eq!(f.engine.slots.available_permits(),32);
    assert_eq!(f.engine.active.load(Ordering::SeqCst),0);
    f.cleanup().await;
    println!("STREAM_ERRORS_PASS grace=HONORED silence=TIMEOUT heartbeat_only=NOT_SUCCESS sse_error=FAILED requests=NO_REPLAY new_timer=YES cancellation=RELEASED");
}

#[tokio::test]
async fn stream_idle_empty_chunks_do_not_extend_deadline() {
    let stream=stream::unfold(0,|i|async move {tokio::time::sleep(Duration::from_millis(50)).await;Some((Ok::<_,reqwest::Error>(Bytes::new()),i+1))});
    let mut stream=Box::pin(stream);let start=Instant::now();
    assert!(next_response_chunk(&mut stream,Some(1)).await.unwrap_err().contains("本机流式空闲超时"));
    assert!(start.elapsed()<Duration::from_secs(2));
    println!("EMPTY_CHUNKS_PASS idle_deadline=NOT_REFRESHED");
}

#[tokio::test]
#[ignore = "explicit 615-second loopback-only soak; no real upstream"]
async fn stream_idle_long_615_seconds() {
    let f=Fixture::new("openai").await;let port=f.engine.start_at(0).await.unwrap();
    {let mut s=f.engine.store.lock().unwrap();s.config.settings.timeout_seconds=610;s.config.models[0].model="idle-long".into();}
    let model=f.engine.config().unwrap().models[0].clone();
    let start=Instant::now();
    let client=reqwest::Client::builder().timeout(Duration::from_secs(660)).build().unwrap();
    let r=client.post(format!("http://127.0.0.1:{port}/v1/chat/completions")).bearer_auth(f.token()).json(&protocol::test_body(&model)).send().await.unwrap();
    assert_eq!(r.status(),200);let text=r.text().await.unwrap();
    assert!(text.ends_with("data: [DONE]\n\n"));
    wait_queue_condition(||f.engine.active.load(Ordering::SeqCst)==0).await;
    assert_eq!(f.engine.runtime().unwrap().calls[0].state,"success");assert_eq!(f.mock.seen.lock().unwrap().len(),1);
    let elapsed=start.elapsed().as_millis();assert!(elapsed>=615000);
    assert_eq!(f.engine.slots.available_permits(),32);
    f.cleanup().await;
    println!("LONG_STREAM_PASS elapsed_ms={elapsed} old_total_seconds=610 actual_output_over_610=YES completed=DONE requests=1 slots=RELEASED loopback_only=YES");
}

#[test]
fn zen_compat_alias_and_static_conflict() {
    let mut model: Model = serde_json::from_value(json!({"id":"fixture","name":"fixture","model":"fixture","protocol":"openai","baseUrl":"https://example.invalid/v1","apiKey":"RELAY_SYNTHETIC_KEY","headers":{"X-OpenCode-Session":"ses_111111111111STATIC12345678","X-OpenCode-Session-Id":"ses_222222222222STATIC12345678"},"enabled":true,"reasoning":"default","prompt":""})).unwrap();
    let session="ses_ecbe1a376ffe0123456789ABCD";
    let mut incoming=HeaderMap::new();incoming.insert("X-OPENCODE-SESSION-ID",session.parse().unwrap());
    incoming.insert("authorization","Bearer LOCAL_SYNTHETIC_TOKEN".parse().unwrap());
    let out=protocol::outgoing_headers(&model,&incoming).unwrap();
    assert_eq!(out["x-opencode-session"],session,"alias-only dynamic id must win");
    assert_eq!(out["x-opencode-session-id"],session);
    assert_eq!(out["authorization"],"Bearer RELAY_SYNTHETIC_KEY");
    model.headers.clear();model.zen_test_enabled=true;
    let test=test_headers(&model).unwrap();let out=protocol::outgoing_headers(&model,&test).unwrap();
    assert_eq!(out["x-opencode-session"],out["x-opencode-session-id"]);
    assert!(!protocol::outgoing_headers(&model,&HeaderMap::new()).unwrap().contains_key("x-opencode-session"));
}

#[test]
fn zen_compat_header_matrix_and_completion() {
    let a="ses_ecbe1a376ffe0123456789ABCD";let b="ses_ecbe1a375ffeABCDEFGHIJKLMN";
    for (primary,alias,want,state) in [(Some(a),None,Some(a),"primary"),(None,Some(a),Some(a),"alias"),(Some(a),Some(a),Some(a),"both"),(Some(a),Some(b),Some(a),"conflict_primary"),(Some(" "),Some(b),Some(b),"alias"),(None,None,None,"absent"),(Some(""),Some(""),None,"absent")] {
        let mut h=HeaderMap::new();if let Some(v)=primary {h.insert("X-OpenCode-Session",v.parse().unwrap());}if let Some(v)=alias{h.insert("X-OpenCode-Session-ID",v.parse().unwrap());}
        let selected=protocol::zen_session(&h).unwrap();assert_eq!(selected.state,state);assert_eq!(selected.value.as_ref().and_then(|v|v.to_str().ok()),want);
    }
    for name in protocol::ZEN_SESSION_HEADERS {
        for bad in ["bad","ses_ABCDEF0123450123456789ABCD","ses_123456789abc0123456789ABCD,other"] {
            let mut h=HeaderMap::new();h.insert(name,bad.parse().unwrap());let error=protocol::zen_session(&h).err().unwrap();assert!(error.contains("格式无效"));assert!(!error.contains(bad));
        }
        let mut h=HeaderMap::new();h.append(name,a.parse().unwrap());h.append(name,b.parse().unwrap());assert!(protocol::zen_session(&h).err().unwrap().contains("冲突"));
        h.clear();h.append(name,a.parse().unwrap());h.append(name,a.parse().unwrap());assert!(protocol::zen_session(&h).is_ok());
    }
    for (body,success) in [("data: [DONE]\n\n",false),(": heartbeat\n\n",false),("data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\nevent: error\ndata: {\"message\":\"error\"}\n\ndata: [DONE]\n\n",false),("data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]",false),("data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n",true)] {
        let mut metrics=Metrics::new(true);metrics.feed(body.as_bytes(),1);metrics.finish(2);assert_eq!(metrics.zen_success(),success);
    }
    println!("ZEN_HEADERS_PASS primary_alias_conflict_case_invalid=OK completion=REAL_OUTPUT no_raw_headers=YES");
}

#[tokio::test]
async fn zen_compat_real_http_three_protocols_and_diagnostics() {
    let a="ses_ecbe1a376ffe0123456789ABCD";let b="ses_ecbe1a375ffeABCDEFGHIJKLMN";
    for protocol_name in ["openai","responses","anthropic"] {
        let f=Fixture::new(protocol_name).await;let port=f.engine.start_at(0).await.unwrap();let client=reqwest::Client::new();
        let model=f.engine.config().unwrap().models[0].clone();
        for (primary,alias,want,state) in [(Some(a),None,Some(a),"primary"),(None,Some(a),Some(a),"alias"),(Some(a),Some(a),Some(a),"both"),(Some(a),Some(b),Some(a),"conflict_primary"),(Some(""),Some(b),Some(b),"alias"),(None,None,None,"absent")] {
            let mut req=client.post(format!("http://127.0.0.1:{port}/v1/{}",protocol::endpoint(protocol_name))).bearer_auth(f.token()).header("x-dsh-patrick","1").header("x-opencode-project","DO_NOT_FORWARD").json(&protocol::test_body(&model));
            if let Some(v)=primary{req=req.header("X-OpenCode-Session",v);}if let Some(v)=alias{req=req.header("X-OPENCODE-SESSION-ID",v);}
            let response=req.send().await.unwrap();assert_eq!(response.status(),200);let _=response.bytes().await.unwrap();
            let seen=f.mock.seen.lock().unwrap().last().unwrap().clone();
            for name in protocol::ZEN_SESSION_HEADERS {assert_eq!(seen.headers.get(name).and_then(|v|v.to_str().ok()),want);assert!(seen.headers.get_all(name).iter().count()<=1);}
            assert!(!seen.headers.contains_key("x-dsh-patrick")&&!seen.headers.contains_key("x-opencode-project"));
            assert_eq!(seen.headers[if protocol_name=="anthropic"{"x-api-key"}else{"authorization"}],if protocol_name=="anthropic"{"UNIT_TEST_UPSTREAM_KEY"}else{"Bearer UNIT_TEST_UPSTREAM_KEY"});
            assert_ne!(seen.headers.get("authorization").and_then(|v|v.to_str().ok()),Some(f.token().as_str()));
            let record=f.engine.runtime().unwrap().calls[0].clone();assert_eq!(record.state,"success");assert_eq!(record.zen_header_received,Some(primary.is_some()||alias.is_some()));assert_eq!(record.zen_header_sent,Some(want.is_some()));assert_eq!(record.zen_session_state.as_deref(),Some(state));
            let json=serde_json::to_string(&record).unwrap();assert!(!json.contains(a)&&!json.contains(b)&&!json.contains(&f.token()));
        }
        let before=f.mock.seen.lock().unwrap().len();
        for name in protocol::ZEN_SESSION_HEADERS {
            let response=client.post(format!("http://127.0.0.1:{port}/v1/{}",protocol::endpoint(protocol_name))).bearer_auth(f.token()).header(name,"SYNTHETIC_INVALID").json(&protocol::test_body(&model)).send().await.unwrap();
            assert_eq!(response.status(),400);let body=response.text().await.unwrap();assert!(body.contains("Zen")&&!body.contains("SYNTHETIC_INVALID"));
        }
        assert_eq!(f.mock.seen.lock().unwrap().len(),before);assert_eq!(f.engine.active.load(Ordering::SeqCst),0);
        f.cleanup().await;
    }
    println!("ZEN_HTTP_PASS protocols=3 aliases=2 session_stable=YES auth_layers=SEPARATE invalid=400 diagnostics=REDACTED non_zen=UNCHANGED");
}

#[tokio::test]
async fn zen_compat_proxy_errors_do_not_report_success() {
    let f=Fixture::new("openai").await;let port=f.engine.start_at(0).await.unwrap();let client=reqwest::Client::new();
    for (upstream,reason) in [("diag-error","错误"),("diag-empty","空响应"),("diag-malformed","格式"),("diag-length","长度"),("truncated","提前结束")] {
        f.engine.store.lock().unwrap().config.models[0].model=upstream.into();
        let response=client.post(format!("http://127.0.0.1:{port}/v1/chat/completions")).bearer_auth(f.token()).header("x-opencode-session-id","ses_ecbe1a376ffe0123456789ABCD").json(&protocol::test_body(&f.engine.config().unwrap().models[0])).send().await.unwrap();
        assert_eq!(response.status(),200);let _=response.bytes().await.unwrap();
        let record=f.engine.runtime().unwrap().calls[0].clone();assert_eq!(record.state,"error");assert!(record.message.contains(reason),"{}",record.message);assert_eq!(record.zen_header_sent,Some(true));
        assert_eq!(f.engine.active.load(Ordering::SeqCst),0);assert_eq!(f.engine.slots.available_permits(),32);
    }
    f.cleanup().await;
    println!("ZEN_PROXY_RESULTS_PASS http200_not_success=YES named_error=FAIL empty=FAIL malformed=FAIL length=FAIL truncated=FAIL slots=RELEASED");
}

#[test]
fn zen_compat_structured_protocol_results() {
    for (body,success) in [
        (json!({"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}],"error":null}),true),
        (json!({"status":"completed","error":null,"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}),true),
        (json!({"status":"failed","error":{"message":"synthetic"},"output":[{"type":"message","content":[{"text":"partial"}]}]}),false),
        (json!({"status":"incomplete","error":null,"output":[{"type":"message","content":[{"text":"partial"}]}]}),false),
        (json!({"content":[{"type":"text","text":"partial"}],"stop_reason":"max_tokens"}),false),
        (json!({"content":[{"type":"tool_use","id":"synthetic","name":"local_tool","input":{}}],"stop_reason":"tool_use"}),true),
        (json!({"choices":[{"message":{"reasoning_content":"thought"},"finish_reason":"stop"}]}),true),
    ] {
        let mut metrics=Metrics::new(false);metrics.feed(&serde_json::to_vec(&body).unwrap(),1);metrics.finish(2);
        assert_eq!(metrics.zen_success(),success,"structured Zen result: {}",body);
    }
    for (body,success) in [
        ("data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"error\":null}}\n\n",true),
        ("data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"incomplete\",\"error\":null}}\n\n",false),
        ("data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{}\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"error\":null}}\n\n",true),
    ] {
        let mut metrics=Metrics::new(true);metrics.feed(body.as_bytes(),1);metrics.finish(2);assert_eq!(metrics.zen_success(),success);
    }
    println!("ZEN_STRUCTURED_PASS null_error=SUCCESS partial_failed=ERROR nonstream_max_tokens=ERROR reasoning_and_tools=SUCCESS");
}
