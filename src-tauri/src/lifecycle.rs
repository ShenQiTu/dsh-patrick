use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

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
    state.disconnect().await
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
