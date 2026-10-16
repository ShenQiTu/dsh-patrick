//! Current-user startup entry and release-page link. Signed updates live in update.rs.
use serde::Serialize;
use std::{ffi::c_void, ptr::null};

pub const STARTUP_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "DSH Patrick";
const TEST_ROOT: &str = r"Software\DSHPatrickTests\";
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
}
