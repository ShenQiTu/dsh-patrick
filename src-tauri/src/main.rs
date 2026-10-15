#![windows_subsystem = "windows"]

mod config;
mod gateway;
mod protocol;
mod vault;
mod lifecycle;
mod history;
mod call_history;
mod desktop_settings;
mod dsh;
#[cfg(test)]
mod exit_tests;

use config::{ModelInput, ModelView, Settings, Store};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{Emitter, Manager};

struct AppState {
    dsh: Mutex<dsh::Status>,
    integration_control: tokio::sync::Mutex<()>,
    close_request: Mutex<Option<lifecycle::CloseRequest>>,
    store: Arc<Mutex<Store>>,
    gateway: Arc<gateway::Gateway>,
    directory: PathBuf,
    smoke: bool,
    autostart_key: String,
    smoke_finished: std::sync::atomic::AtomicBool,
}

impl AppState {
    async fn bridge(&self, action: &str) -> Result<(), String> {
        if self.smoke { return Err("原生 UI 自测不访问正式 DSH 配置".into()); }
        let directory=self.directory.clone();
        let config=self.store.lock().map_err(|_| "配置锁异常")?.config.clone();
        let port=self.gateway.runtime()?.port;
        let resolves_recovery=matches!(action,"sync"|"disconnect");
        let action=action.to_owned();
        let profile=config.settings.dsh_profile.clone();
        let result=tauri::async_runtime::spawn_blocking(move || dsh::execute_detailed(&directory,&config,&action,port)).await.unwrap_or_else(|_|Err(dsh::Failure::from("DSH 接入任务异常")));
        let mut status=self.dsh.lock().map_err(|_| "接入状态锁异常")?;
        match result {
            Ok(value)=>{
                *status=value;
                if resolves_recovery {
                    lifecycle::clear_recovery(&self.directory,&profile)?;
                    *self.close_request.lock().map_err(|_| "退出状态锁异常")?=None;
                }
                Ok(())
            }
            Err(error)=>{let message=error.message.clone();*status=dsh::Status::failed(profile,error);Err(message)}
        }
    }
    async fn sync_if_running(&self) -> Result<(), String> {
        if self.gateway.running() && !self.smoke { self.bridge("sync").await.map_err(|e|format!("模型已保存在 Patrick，但 DSH 同步失败：{e}"))?; }
        Ok(())
    }
    async fn disconnect(&self) -> Result<(), String> { self.request_close(false).await }
    async fn request_close(&self, exit: bool) -> Result<(), String> {
        let _control=self.integration_control.lock().await;
        *self.close_request.lock().map_err(|_| "退出状态锁异常")?=None;
        let _pause=self.gateway.pause_if_idle()?;
        if !self.smoke {
            let profile=self.store.lock().map_err(|_| "配置锁异常")?.config.settings.dsh_profile.clone();
            let restore=match dsh::owns_integration(&self.directory,&profile) {
                Ok(true)=>self.bridge("disconnect").await,
                Ok(false)=>{
                    *self.dsh.lock().map_err(|_| "接入状态锁异常")?=dsh::Status {profile:profile.clone(),message:"网关已断开；请点击检查状态重新核对配置".into(),..Default::default()};
                    Ok(())
                },
                Err(error)=>{
                    *self.dsh.lock().map_err(|_| "接入状态锁异常")?=dsh::Status::failed(profile.clone(),dsh::Failure::from(error.clone()));
                    Err(error)
                },
            };
            if let Err(message)=restore {
                self.offer_close(exit,profile,message.clone())?;
                return Err(message);
            }
        }
        self.gateway.stop().await
    }
    fn offer_close(&self, exit:bool, profile:String, message:String) -> Result<(),String> {
        *self.close_request.lock().map_err(|_| "退出状态锁异常")?=Some(lifecycle::CloseRequest {id:config::new_id(),exit,profile,message});
        Ok(())
    }
    async fn resolve_close_request(&self, id:&str, confirm:bool) -> Result<bool,String> {
        let _control=self.integration_control.lock().await;
        let request=self.close_request.lock().map_err(|_| "退出状态锁异常")?.clone()
            .filter(|r| r.id==id).ok_or("退出确认已失效，请重新操作")?;
        if !confirm { *self.close_request.lock().map_err(|_| "退出状态锁异常")?=None; return Ok(false); }
        let _pause=self.gateway.pause_if_idle()?;
        let profile=self.store.lock().map_err(|_| "配置锁异常")?.config.settings.dsh_profile.clone();
        if request.profile!=profile {return Err("DSH 路径已变化，请取消后重新操作".into());}
        // Only our encrypted reminder is written. No restore script or DSH file is touched.
        lifecycle::mark_recovery(&self.directory,&profile)?;
        self.gateway.stop().await?;
        let mut status=self.dsh.lock().map_err(|_| "接入状态锁异常")?;
        status.configured=false;status.sync_state="recovery_pending".into();
        status.message="网关已停止；DSH 配置、凭据及恢复记录已保留，接入恢复待处理".into();
        *self.close_request.lock().map_err(|_| "退出状态锁异常")?=None;
        Ok(request.exit)
    }
}
#[tauri::command]
async fn sync_dsh(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let _control=state.integration_control.lock().await;
    if !state.gateway.running() { return Err("请先在 DSH 模型页启动网关".into()); }
    state.bridge("sync").await
}
#[tauri::command]
async fn check_dsh(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let _control=state.integration_control.lock().await;
    state.bridge("status").await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    models: Vec<ModelView>,
    settings: Settings,
    pricing: config::TokenPricing,
    data_directory: String,
    app_version: String,
    test_mode: bool,
    gateway_running: bool,
    gateway_available: bool,
    dsh_connected: bool,
    dsh_recovery_pending: bool,
    close_request: Option<lifecycle::CloseRequest>,
    dsh: dsh::Status,
    tray_available: bool,
    runtime: gateway::RuntimeView,
    test_upstream: Option<String>,
    test_pause_seconds: u64,
}

#[tauri::command]
fn snapshot(app: tauri::AppHandle, state: tauri::State<AppState>) -> Result<Snapshot, String> {
    let dsh=state.dsh.lock().map_err(|_| "接入状态锁异常")?.clone();
    let store = state.store.lock().map_err(|_| "配置锁异常")?;
    Ok(Snapshot {
        models: store.config.models.iter().map(|m| m.view()).collect(),
        settings: store.config.settings.clone(),
        pricing: store.config.pricing.clone(),
        data_directory: state.directory.display().to_string(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        test_mode: state.smoke,
        gateway_running: state.gateway.running(),
        gateway_available: true,
        dsh_connected: dsh.configured,
        dsh_recovery_pending: lifecycle::recovery_pending(&state.directory,&store.config.settings.dsh_profile),
        close_request: state.close_request.lock().map_err(|_| "退出状态锁异常")?.clone(),
        dsh,
        tray_available: app.tray_by_id("patrick-tray").is_some(),
        runtime: state.gateway.runtime()?,
        test_upstream: if state.smoke {
            std::env::var("DSH_PATRICK_MOCK_URL").ok().filter(|s| {
                url::Url::parse(s)
                    .is_ok_and(|u| u.scheme() == "http" && u.host_str() == Some("127.0.0.1"))
            })
        } else {
            None
        },
        test_pause_seconds: if state.smoke {
            std::env::var("DSH_PATRICK_SMOKE_PAUSE")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
                .min(30)
        } else {
            0
        },
    })
}
#[tauri::command]
async fn save_model(state: tauri::State<'_, AppState>, input: ModelInput) -> Result<String, String> {
    let _control=state.integration_control.lock().await;
    let id=state.gateway.save_model(input)?;
    state.sync_if_running().await?;
    Ok(id)
}
#[tauri::command]
async fn delete_model(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let _control=state.integration_control.lock().await;
    state.gateway.delete_model(&id)?;
    state.sync_if_running().await
}
#[tauri::command]
async fn duplicate_model(state: tauri::State<'_, AppState>, id: String) -> Result<String, String> {
    let _control=state.integration_control.lock().await;
    let new_id=state.store.lock().map_err(|_| "配置锁异常")?.duplicate(&id)?;
    state.sync_if_running().await?;
    Ok(new_id)
}
#[tauri::command]
async fn save_settings(state: tauri::State<'_, AppState>, settings: Settings) -> Result<(), String> {
    let _control=state.integration_control.lock().await;
    let old=state.store.lock().map_err(|_| "配置锁异常")?.config.settings.clone();
    if old.dsh_profile!=settings.dsh_profile && (state.gateway.running() || std::path::Path::new(&old.dsh_profile).join(".dsh-patrick/journal.json").exists()) {
        return Err("更换 DSH profile 前请先断开并恢复当前接入".into());
    }
    if state.gateway.running()
        && state
            .store
            .lock()
            .map_err(|_| "配置锁异常")?
            .config
            .settings
            .port
            != settings.port
    {
        return Err("修改监听端口前请先关闭网关".into());
    }
    let changed_profile=old.dsh_profile!=settings.dsh_profile;
    let profile=settings.dsh_profile.clone();
    state.gateway.save_settings(settings)?;
    if changed_profile {*state.dsh.lock().map_err(|_| "接入状态锁异常")?=dsh::Status {profile,message:"路径已保存，尚未检查；请点击检查状态".into(),..Default::default()};}
    Ok(())
}

#[tauri::command]
async fn discover_models(
    state: tauri::State<'_, AppState>,
    input: ModelInput,
    models_url: Option<String>,
) -> Result<Vec<String>, String> {
    state.gateway.discover(input, models_url).await
}
#[tauri::command]
async fn start_gateway(state: tauri::State<'_, AppState>) -> Result<u16, String> {
    let _control=state.integration_control.lock().await;
    if state.smoke { return state.gateway.start_at(0).await; }
    state.bridge("inspect").await?;
    let port=state.gateway.start().await?;
    if let Err(e)=state.bridge("sync").await {
        let _=state.gateway.stop().await;
        return Err(e);
    }
    Ok(port)
}
#[tauri::command]
async fn stop_gateway(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.disconnect().await
}
#[tauri::command]
async fn resolve_close(app: tauri::AppHandle, state: tauri::State<'_,AppState>, id:String, confirm:bool) -> Result<(),String> {
    let exit=state.resolve_close_request(&id,confirm).await?;
    if exit && !state.smoke {app.exit(0);}
    Ok(())
}
#[tauri::command]
async fn test_model(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<gateway::CallRecord, String> {
    state.gateway.test(&id).await
}
#[tauri::command]
fn cancel_test(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    state.gateway.cancel_test(&id)
}
#[tauri::command]
fn call_detail(state: tauri::State<AppState>, id:String) -> Result<serde_json::Value,String> { state.gateway.call_detail(&id) }
#[tauri::command]
fn clear_call_details(state: tauri::State<AppState>) -> Result<(),String> { state.gateway.clear_call_details() }
#[tauri::command]
fn clear_records(state: tauri::State<AppState>) -> Result<(), String> {
    state.gateway.clear_records()
}
#[tauri::command]
fn save_pricing(state: tauri::State<AppState>, pricing: config::TokenPricing) -> Result<(), String> {
    state.store.lock().map_err(|_| "配置锁异常")?.pricing(pricing)
}
#[tauri::command]
fn statistics_storage(state: tauri::State<AppState>) -> Result<serde_json::Value, String> { state.gateway.statistics_storage() }
#[tauri::command]
fn clear_statistics(state: tauri::State<AppState>) -> Result<(), String> { state.gateway.clear_statistics() }
#[tauri::command]
fn autostart_status(state: tauri::State<AppState>) -> Result<desktop_settings::AutostartStatus, String> { desktop_settings::autostart_status(&state.autostart_key) }
#[tauri::command]
fn set_autostart(state: tauri::State<AppState>, enabled: bool) -> Result<desktop_settings::AutostartStatus, String> { desktop_settings::set_autostart(&state.autostart_key, enabled) }
#[tauri::command]
async fn check_update(state: tauri::State<'_, AppState>) -> Result<desktop_settings::UpdateInfo, String> {
    if state.smoke { return Err("自测模式不访问 GitHub；仓库未公开时将提示登录发布页查看".into()); }
    desktop_settings::check_update().await
}
#[tauri::command]
fn open_release_page(state: tauri::State<AppState>) -> Result<(), String> {
    if state.smoke { return Err("自测模式不打开外部浏览器".into()); }
    desktop_settings::open_release_page()
}
#[tauri::command]
fn gateway_token(state: tauri::State<AppState>) -> Result<String, String> {
    Ok(state
        .store
        .lock()
        .map_err(|_| "配置锁异常")?
        .config
        .gateway_token
        .clone())
}

#[tauri::command]
async fn smoke_gateway_probe(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<String, String> {
    if !state.smoke {
        return Err("测试通道未启用".into());
    }
    let token = state
        .store
        .lock()
        .map_err(|_| "配置锁异常")?
        .config
        .gateway_token
        .clone();
    let port = state.gateway.runtime()?.port;
    if port == 0 {
        return Err("测试网关未启动".into());
    }
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|_| "测试客户端初始化失败")?
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .bearer_auth(token)
        .header("x-opencode-session", "ses_123456789abc0123456789ABCD")
        .header("x-oc-upstream", "https://example.invalid/ignored")
        .header("cookie", "NATIVE_MOCK_COOKIE")
        .json(&serde_json::json!({"model":id,"messages":[{"role":"user","content":"mock-only"}]}))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|_| "本机代理测试请求失败")?;
    if response.status() != 200 {
        return Err("本机代理状态断言失败".into());
    }
    let result: serde_json::Value = response
        .json()
        .await
        .map_err(|_| "本机代理 JSON 断言失败")?;
    if result["choices"][0]["message"]["content"] != "Hello from native mock" {
        return Err("本机代理正文断言失败".into());
    }
    Ok("NATIVE_GATEWAY_GZIP_PROXY_PASS".into())
}

#[tauri::command]
fn statistics(state: tauri::State<AppState>) -> Result<Vec<history::UsageRecord>, String> { state.gateway.statistics() }

#[tauri::command]
async fn smoke_lifecycle(app: tauri::AppHandle, state: tauri::State<'_, AppState>, action: String) -> Result<bool, String> {
    if !state.smoke { return Err("测试通道未启用".into()); }
    match action.as_str() {
        "show" => { lifecycle::show(&app)?; Ok(true) }
        "visible" => app.get_webview_window("main").ok_or("窗口未就绪")?.is_visible().map_err(|e| e.to_string()),
        "exit-check" => { lifecycle::prepare_exit(&app).await?; Ok(true) }
        "recovery-offer-stop" | "recovery-offer-exit" => {
            state.gateway.start_at(0).await?;
            let profile=state.store.lock().map_err(|_| "配置锁异常")?.config.settings.dsh_profile.clone();
            {let mut status=state.dsh.lock().map_err(|_| "接入状态锁异常")?;status.configured=false;status.sync_state="error".into();}
            state.offer_close(action=="recovery-offer-exit",profile,"合成配置冲突；当前文件已保留".into())?;
            Ok(true)
        }
        "recovery-reset" => {
            let profile=state.store.lock().map_err(|_| "配置锁异常")?.config.settings.dsh_profile.clone();
            lifecycle::clear_recovery(&state.directory,&profile)?;Ok(true)
        }
        "bridge-self-contained" => {
            let directory=state.directory.clone();
            tauri::async_runtime::spawn_blocking(move || dsh::smoke_probe(&directory)).await.map_err(|_| "接入组件自测任务异常")??;
            Ok(true)
        }
        "diagnostics-invalid" | "diagnostics-valid" | "diagnostics-synced" => {
            let directory=state.directory.clone();let scenario=action.trim_start_matches("diagnostics-").to_owned();
            let result=tauri::async_runtime::spawn_blocking(move || dsh::smoke_diagnostics(&directory,&scenario)).await.map_err(|_| "诊断自测任务异常")??;
            let mut settings=state.store.lock().map_err(|_| "配置锁异常")?.config.settings.clone();
            settings.dsh_profile=result.profile.clone();state.gateway.save_settings(settings)?;
            *state.dsh.lock().map_err(|_| "接入状态锁异常")?=result;
            Ok(true)
        }
        _ => Err("未知测试动作".into()),
    }
}

// Explicit test launch only; no test output path exists in ordinary sessions.
#[tauri::command]
fn smoke_finish(state: tauri::State<AppState>, checks: Vec<String>) -> Result<(), String> {
    if !state.smoke {
        return Err("测试通道未启用".into());
    }
    if checks.len() < 23
        || state.gateway.running()
        || state.gateway.runtime()?.active_requests > 0
        || !state
            .store
            .lock()
            .map_err(|_| "配置锁异常")?
            .config
            .models
            .is_empty()
    {
        return Err("原生 UI 自测断言未完成".into());
    }
    let target = std::env::var("DSH_PATRICK_SMOKE_RESULT").map_err(|_| "缺少自测结果路径")?;
    let result = serde_json::json!({"status":"PASS","checks":checks,"dshTouched":false,"gatewayTested":true,"gatewayStopped":true,"upstreams":"loopback-only","testData":state.directory});
    std::fs::write(
        target,
        serde_json::to_vec_pretty(&result).map_err(|_| "自测序列化失败")?,
    )
    .map_err(|_| "自测报告写入失败")?;
    desktop_settings::cleanup_test_key(&state.autostart_key);
    state.smoke_finished.store(true, std::sync::atomic::Ordering::SeqCst);
    // The UI smoke then clicks the real custom close button. The process timeout
    // detects a missing handler instead of silently closing the window for it.
    Ok(())
}

#[tauri::command]
fn smoke_fail(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    message: String,
) -> Result<(), String> {
    if !state.smoke {
        return Err("测试通道未启用".into());
    }
    let target = std::env::var("DSH_PATRICK_SMOKE_RESULT").map_err(|_| "缺少自测结果路径")?;
    let result = serde_json::json!({"status":"FAIL","message":message});
    std::fs::write(target, result.to_string()).map_err(|_| "写入自测报告失败")?;
    desktop_settings::cleanup_test_key(&state.autostart_key);
    app.exit(1);
    Ok(())
}

#[tauri::command]
fn smoke_resize(app: tauri::AppHandle, state: tauri::State<AppState>, width: u32, height: u32) -> Result<(), String> {
    if !state.smoke || !matches!((width, height), (800, 520) | (820, 550) | (960, 640)) {
        return Err("测试窗口尺寸未启用".into());
    }
    app.get_webview_window("main").ok_or("测试窗口未就绪")?
        .set_size(tauri::LogicalSize::new(width, height)).map_err(|e| e.to_string())
}

fn main() {
    let smoke = std::env::args().any(|a| a == "--ui-smoke-test");
    let directory = std::env::var_os("DSH_PATRICK_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join("app-data")
        });
    let webview = directory.join("webview");
    if smoke
        && (directory.exists()
            || std::env::var_os("DSH_PATRICK_DATA").is_none()
            || std::env::var_os("DSH_PATRICK_SMOKE_RESULT").is_none())
    {
        eprintln!("SMOKE_ABORT: use a fresh, explicit test data directory and result path");
        std::process::exit(2);
    }
    let visual_review = smoke && std::env::var("DSH_PATRICK_SMOKE_SHOW").as_deref() == Ok("1");
    let result=tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![snapshot,sync_dsh,check_dsh,save_model,delete_model,duplicate_model,save_settings,discover_models,start_gateway,stop_gateway,resolve_close,test_model,cancel_test,clear_records,call_detail,clear_call_details,save_pricing,statistics_storage,clear_statistics,autostart_status,set_autostart,check_update,open_release_page,gateway_token,smoke_gateway_probe,smoke_finish,smoke_fail,smoke_resize,statistics,smoke_lifecycle])
        .setup(move|app|{
            let store=Arc::new(Mutex::new(Store::open(directory.clone()).map_err(std::io::Error::other)?));
            let gateway=gateway::Gateway::new(store.clone()).map_err(std::io::Error::other)?;
            app.manage(AppState{dsh:Mutex::new(dsh::Status::default()),integration_control:tokio::sync::Mutex::new(()),close_request:Mutex::new(None),store,gateway,directory,smoke,autostart_key:if smoke {desktop_settings::test_key()} else {desktop_settings::STARTUP_KEY.into()},smoke_finished: std::sync::atomic::AtomicBool::new(false)});
            tauri::WebviewWindowBuilder::new(app,"main",tauri::WebviewUrl::App("index.html".into()))
                .title(if visual_review{format!("DSH Patrick v{} — 本机模拟验证",env!("CARGO_PKG_VERSION"))}else{format!("DSH Patrick v{}",env!("CARGO_PKG_VERSION"))}).inner_size(960.0,640.0).min_inner_size(800.0,520.0)
                .decorations(false).visible(!smoke||visual_review).data_directory(webview)
                .additional_browser_args("--disable-background-networking --disable-component-update --disable-sync --no-first-run")
                .on_navigation(|url|((url.scheme()=="http"||url.scheme()=="https")&&url.host_str()==Some("tauri.localhost")) || (url.scheme()=="http"&&url.host_str()==Some("127.0.0.1")&&url.port()==Some(1427)) || (url.scheme()=="tauri"&&url.host_str()==Some("localhost")))
                .build()?;
            lifecycle::install(app.handle())?;
            if !smoke { let handle=app.handle().clone(); tauri::async_runtime::spawn(async move {
                let state=handle.state::<AppState>();
                loop {
                    { let _control=state.integration_control.lock().await; let _=state.bridge("status").await; }
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                }
            }); }
            if smoke {let handle=app.handle().clone();std::thread::spawn(move||{std::thread::sleep(std::time::Duration::from_secs(180));handle.exit(3);});}
            Ok(())
        })
        .on_window_event(|window,event|{
            if let tauri::WindowEvent::CloseRequested{api,..}=event {
                if let Some(state)=window.try_state::<AppState>() {
                    if !state.smoke_finished.load(std::sync::atomic::Ordering::SeqCst) {
                        api.prevent_close();
                        if let Err(error) = window.hide() { let _ = window.emit("gateway-busy-close",format!("收起窗口失败：{error}")); }
                    }
                }
            }
        })
        .run(tauri::generate_context!());
    if let Err(error) = result {
        if smoke {
            eprintln!("SMOKE_START_FAILED: {error}");
        } else {
            vault::show_error(&format!("DSH Patrick 启动失败：{error}"));
        }
        std::process::exit(1);
    }
}
