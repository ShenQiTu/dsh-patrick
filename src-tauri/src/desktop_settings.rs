//! Explicit user actions only: current-user startup entry and public release checks.
use serde::Serialize;
use std::{ffi::c_void, ptr::null, time::Duration};
use futures_util::StreamExt;

pub const STARTUP_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "DSH Patrick";
const TEST_ROOT: &str = r"Software\DSHPatrickTests\";
const RELEASE_API: &str = "https://api.github.com/repos/ShenQiTu/dsh-patrick/releases/latest";
const RELEASE_PAGE: &str = "https://github.com/ShenQiTu/dsh-patrick/releases";
type Hkey = *mut c_void;
const HKCU: Hkey = 0x80000001u32 as i32 as isize as Hkey;

#[link(name = "Advapi32")]
extern "system" {
    fn RegGetValueW(key: Hkey, subkey: *const u16, value: *const u16, flags: u32, kind: *mut u32, data: *mut c_void, size: *mut u32) -> i32;
    fn RegCreateKeyExW(key: Hkey, subkey: *const u16, reserved: u32, class: *mut u16, options: u32, access: u32, security: *const c_void, result: *mut Hkey, disposition: *mut u32) -> i32;
    fn RegSetValueExW(key: Hkey, value: *const u16, reserved: u32, kind: u32, data: *const u8, size: u32) -> i32;
    fn RegDeleteKeyValueW(key: Hkey, subkey: *const u16, value: *const u16) -> i32;
    fn RegCloseKey(key: Hkey) -> i32;
    fn RegDeleteKeyW(key: Hkey, subkey: *const u16) -> i32;
}
#[link(name = "Shell32")]
extern "system" {
    fn ShellExecuteW(window: *mut c_void, operation: *const u16, file: *const u16, parameters: *const u16, directory: *const u16, show: i32) -> isize;
}
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn command() -> Result<String, String> {
    let path = std::env::current_exe().map_err(|_| "读取程序路径失败")?;
    Ok(format!("\"{}\" --autostart", path.display()))
}
pub fn test_key() -> String { format!("{TEST_ROOT}{}", crate::config::new_id()) }
pub fn cleanup_test_key(key: &str) {
    if key.starts_with(TEST_ROOT) && !key[TEST_ROOT.len()..].contains('\\') {
        unsafe { RegDeleteKeyW(HKCU, wide(key).as_ptr()); }
    }
}
fn read_value(key: &str) -> Result<Option<String>, String> {
    let mut bytes = 0u32;
    let (key, name) = (wide(key), wide(VALUE_NAME));
    let result = unsafe { RegGetValueW(HKCU, key.as_ptr(), name.as_ptr(), 2, std::ptr::null_mut(), std::ptr::null_mut(), &mut bytes) };
    if result == 2 { return Ok(None); }
    if result != 0 || bytes > 65536 { return Err("读取开机启动项失败".into()); }
    let mut buffer = vec![0u16; (bytes as usize).div_ceil(2)];
    let result = unsafe { RegGetValueW(HKCU, key.as_ptr(), name.as_ptr(), 2, std::ptr::null_mut(), buffer.as_mut_ptr().cast(), &mut bytes) };
    if result != 0 { return Err("读取开机启动项失败".into()); }
    buffer.truncate(bytes as usize / 2);
    while buffer.last() == Some(&0) { buffer.pop(); }
    String::from_utf16(&buffer).map(Some).map_err(|_| "开机启动项编码错误".into())
}
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AutostartStatus { pub enabled: bool, pub current_executable: bool }
pub fn autostart_status(key: &str) -> Result<AutostartStatus, String> {
    let value = read_value(key)?;
    Ok(AutostartStatus { enabled: value.is_some(), current_executable: value.as_deref() == Some(&command()?) })
}
pub fn set_autostart(key: &str, enabled: bool) -> Result<AutostartStatus, String> {
    if enabled {
        let value = wide(&command()?); let mut handle: Hkey = std::ptr::null_mut();
        let created = unsafe { RegCreateKeyExW(HKCU, wide(key).as_ptr(), 0, std::ptr::null_mut(), 0, 2, null(), &mut handle, std::ptr::null_mut()) };
        if created != 0 { return Err("写入开机启动项失败".into()); }
        let saved = unsafe { RegSetValueExW(handle, wide(VALUE_NAME).as_ptr(), 0, 1, value.as_ptr().cast(), (value.len()*2) as u32) };
        unsafe { RegCloseKey(handle); }
        if saved != 0 { return Err("保存开机启动项失败".into()); }
    } else {
        let result = unsafe { RegDeleteKeyValueW(HKCU, wide(key).as_ptr(), wide(VALUE_NAME).as_ptr()) };
        if result != 0 && result != 2 { return Err("关闭开机启动失败".into()); }
    }
    let status = autostart_status(key)?;
    if status.enabled != enabled || (enabled && !status.current_executable) { return Err("开机启动项核验失败".into()); }
    Ok(status)
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo { pub version: String, pub state: String }
fn version(s: &str) -> Result<(u32,u32,u32), String> {
    let parts: Vec<_> = s.strip_prefix('v').unwrap_or(s).split('.').collect();
    if parts.len()!=3 || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) { return Err("发布版本号格式错误".into()); }
    Ok((parts[0].parse().map_err(|_| "版本号超出范围")?, parts[1].parse().map_err(|_| "版本号超出范围")?, parts[2].parse().map_err(|_| "版本号超出范围")?))
}
fn release_info(body: &[u8], current: &str) -> Result<UpdateInfo, String> {
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| "发布信息格式错误")?;
    if value.get("draft").and_then(|v|v.as_bool()) != Some(false) || value.get("prerelease").and_then(|v|v.as_bool()) != Some(false) {
        return Err("发布信息不是正式版本".into());
    }
    let tag = value.get("tag_name").and_then(|v|v.as_str()).ok_or("发布信息缺少版本号")?;
    let ordering = version(tag)?.cmp(&version(current)?);
    Ok(UpdateInfo { version: tag.into(), state: match ordering { std::cmp::Ordering::Greater=>"available",std::cmp::Ordering::Equal=>"current",std::cmp::Ordering::Less=>"ahead" }.into() })
}
async fn check_at(url: &str, current: &str) -> Result<UpdateInfo, String> {
    let response = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15)).build().map_err(|_| "更新检查初始化失败")?
        .get(url).header("user-agent", "DSH-Patrick-update-check").header("accept", "application/vnd.github+json")
        .send().await.map_err(|_| "连接 GitHub 失败或超时，请稍后重试")?;
    match response.status().as_u16() {
        200 => {},
        404 => return Err("仓库未公开、不可访问或尚无正式发布；可打开发布页登录后查看".into()),
        403 | 429 => return Err("GitHub 暂时限制更新检查频率，请稍后重试".into()),
        _ => return Err(format!("检查更新返回 HTTP {}，请稍后重试", response.status().as_u16())),
    }
    let mut data=Vec::new();let mut stream=response.bytes_stream();
    while let Some(chunk)=stream.next().await {
        let bytes=chunk.map_err(|_| "读取更新信息失败或超时")?;
        if data.len()+bytes.len()>256*1024 {return Err("发布信息超过大小限制".into());}
        data.extend_from_slice(&bytes);
    }
    release_info(&data,current)
}
pub async fn check_update() -> Result<UpdateInfo, String> { check_at(RELEASE_API,env!("CARGO_PKG_VERSION")).await }
pub fn open_release_page() -> Result<(), String> {
    let result=unsafe { ShellExecuteW(std::ptr::null_mut(),wide("open").as_ptr(),wide(RELEASE_PAGE).as_ptr(),null(),null(),1) };
    if result<=32 { Err("打开默认浏览器失败".into()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestKey(String);
    impl Drop for TestKey {fn drop(&mut self){cleanup_test_key(&self.0);}}
    #[test]
    fn autostart_roundtrip_in_non_startup_test_key() {
        let key=TestKey(test_key());assert!(!key.0.contains("CurrentVersion\\Run"));
        assert!(!autostart_status(&key.0).unwrap().enabled);
        assert!(set_autostart(&key.0,true).unwrap().current_executable);
        assert_eq!(read_value(&key.0).unwrap().unwrap(),command().unwrap());
        assert!(!set_autostart(&key.0,false).unwrap().enabled);
        assert!(!set_autostart(&key.0,false).unwrap().enabled);
        println!("AUTOSTART_PASS isolated_non_run_registry=YES enabled_verified=YES disabled_verified=YES real_run_key_untouched=YES");
    }
    #[tokio::test]
    async fn release_check_local_http_and_failure_paths() {
        use axum::{Router, routing::get, response::IntoResponse};
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let port=listener.local_addr().unwrap().port();
        let app=Router::new().route("/latest", get(|headers:axum::http::HeaderMap|async move {
            assert!(!headers.contains_key("authorization") && !headers.contains_key("cookie"));
            axum::Json(serde_json::json!({"tag_name":"v0.1.9","draft":false,"prerelease":false,"html_url":"https://example.invalid/untrusted"}))
        })).route("/private",get(||async{(axum::http::StatusCode::NOT_FOUND,"ignored").into_response()}))
        .route("/redirect",get(||async{(axum::http::StatusCode::FOUND,[("location","/latest")],"").into_response()}));
        let task=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let base=format!("http://127.0.0.1:{port}");
        for (v,state) in [("0.1.8","available"),("0.1.9","current"),("0.1.10","ahead")] {
            assert_eq!(check_at(&format!("{base}/latest"),v).await.unwrap().state,state);
        }
        assert!(check_at(&format!("{base}/private"),"0.1.6").await.unwrap_err().contains("仓库未公开"));
        assert!(check_at(&format!("{base}/redirect"),"0.1.6").await.unwrap_err().contains("302"));
        for bad in ["-1.2.3","1.2","1.2.3;start","1.2.3-beta","1.2.999999999999999"] {assert!(version(bad).is_err());}
        assert!(release_info(br#"{"tag_name":"v9.0.0","draft":true,"prerelease":false}"#,"0.1.6").is_err());
        task.abort();println!("UPDATES_PASS version_compare=YES private404=ERROR redirects=BLOCKED credentials=NONE loopback_only=YES");
    }
}
