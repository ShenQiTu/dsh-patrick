use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

// A failed restore offers an explicit choice; it never silently overwrites DSH files.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseRequest {
    pub id: String,
    pub exit: bool,
    pub message: String,
    #[serde(skip_serializing)]
    pub profile: String,
}
fn recovery_path(directory: &std::path::Path) -> std::path::PathBuf { directory.join("dsh-recovery.dpapi") }
pub fn recovery_pending(directory: &std::path::Path, profile: &str) -> bool {
    let path=recovery_path(directory);
    path.exists() && crate::vault::read(&path).map(|bytes| bytes==profile.as_bytes()).unwrap_or(true)
}
pub fn mark_recovery(directory: &std::path::Path, profile: &str) -> Result<(),String> {
    crate::vault::write(&recovery_path(directory),profile.as_bytes())
        .map_err(|_| "保存待恢复提醒失败；尚未执行退出，请检查软件数据目录".into())
}
pub fn clear_recovery(directory: &std::path::Path, profile: &str) -> Result<(),String> {
    let path=recovery_path(directory);if !path.exists() {return Ok(());}
    let bytes=crate::vault::read(&path).map_err(|_| "接入已处理，但待恢复提醒读取失败；原提醒已保留")?;
    if bytes==profile.as_bytes() {std::fs::remove_file(path).map_err(|_| "接入已处理，但待恢复提醒清理失败")?;}
    Ok(())
}

pub fn show(app: &AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("主窗口未就绪")?;
    window.show().map_err(|e| e.to_string())?;
    window.unminimize().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

pub async fn prepare_exit(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    if state.gateway.runtime()?.active_requests > 0 {
        return Err("仍有请求进行中；窗口可以收起，请等待请求结束后再退出。".into());
    }
    state.request_close(true).await
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "显示 DSH Patrick", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出 DSH Patrick", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;
    let icon = app.default_window_icon().expect("bundled Patrick icon").clone();
    TrayIconBuilder::with_id("patrick-tray")
        .icon(icon)
        .tooltip("DSH Patrick · 点击打开，右键退出")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) {
                let _ = show(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => { let _ = show(app); }
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    match prepare_exit(&app).await {
                        Ok(()) => app.exit(0),
                        Err(error) => {
                            let _ = show(&app);
                            let _ = app.emit("gateway-busy-close", error);
                        }
                    }
                });
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}
