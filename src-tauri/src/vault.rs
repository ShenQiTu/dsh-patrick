//! Windows DPAPI, current-user scope. The entire configuration is encrypted, including headers.
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};

#[repr(C)]
struct Blob {
    length: u32,
    data: *mut u8,
}

#[link(name = "Crypt32")]
extern "system" {
    fn CryptProtectData(
        input: *const Blob,
        description: *const u16,
        entropy: *const Blob,
        reserved: *mut std::ffi::c_void,
        prompt: *mut std::ffi::c_void,
        flags: u32,
        output: *mut Blob,
    ) -> i32;
    fn CryptUnprotectData(
        input: *const Blob,
        description: *mut *mut u16,
        entropy: *const Blob,
        reserved: *mut std::ffi::c_void,
        prompt: *mut std::ffi::c_void,
        flags: u32,
        output: *mut Blob,
    ) -> i32;
}
#[link(name = "Kernel32")]
extern "system" {
    fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn MoveFileExW(old: *const u16, new: *const u16, flags: u32) -> i32;
}

#[link(name = "User32")]
extern "system" {
    fn MessageBoxW(
        window: *mut std::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        flags: u32,
    ) -> i32;
}
pub fn show_error(message: &str) {
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let caption: Vec<u16> = "DSH Patrick".encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), 0x10);
    }
}

fn transform(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    let input = Blob {
        length: bytes.len().try_into().map_err(|_| "配置过大")?,
        data: bytes.as_ptr() as *mut _,
    };
    let mut output = Blob {
        length: 0,
        data: std::ptr::null_mut(),
    };
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err(format!(
            "Windows 凭据{}失败（{}）",
            if encrypt { "加密" } else { "解密" },
            std::io::Error::last_os_error()
        ));
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.data, output.length as usize).to_vec() };
    unsafe {
        std::ptr::write_bytes(output.data, 0, output.length as usize);
        LocalFree(output.data.cast());
    }
    Ok(result)
}

pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    let data = std::fs::read(path).map_err(|_| "读取配置失败")?;
    if data.len() > 8 * 1024 * 1024 {
        return Err("配置超过大小限制".into());
    }
    transform(&data, false)
}

pub fn write(path: &Path, plain: &[u8]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let data = transform(plain, true)?;
    let temporary = path.with_extension(format!("{}.tmp", crate::config::new_id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "创建配置临时文件失败")?;
        file.write_all(&data)
            .and_then(|_| file.sync_all())
            .map_err(|_| "写入配置失败")?;
        drop(file);
        let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // A short-lived reader can deny delete/rename sharing on Windows.
        // MoveFileEx can report ACCESS_DENIED as well as a sharing/lock violation.
        // Bound retries to 300 ms; permanent permissions errors still surface.
        for attempt in 0..=6 {
            if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } != 0 {
                return Ok(());
            }
            let error = std::io::Error::last_os_error();
            if !matches!(error.raw_os_error(), Some(5 | 32 | 33)) || attempt == 6 {
                return Err(format!("原子替换配置失败（{error}）；原配置保持不变，请稍后重试"));
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        unreachable!("replacement either succeeds or returns its last error")
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub fn lock(directory: &Path) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(directory.join("instance.lock"))
        .map_err(|_| "此数据目录已由另一个 DSH Patrick 窗口打开".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn temporary_reader_lock_is_retried() {
        let dir = std::env::temp_dir().join(crate::config::new_id());
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("config.dpapi");
        write(&path, b"ORIGINAL_FAKE_CONFIG").unwrap();
        let reader = OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(90));
            drop(reader);
        });
        write(&path, b"UPDATED_FAKE_CONFIG").unwrap();
        releaser.join().unwrap();
        assert_eq!(read(&path).unwrap(), b"UPDATED_FAKE_CONFIG");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn permanent_reader_lock_preserves_original() {
        let dir = std::env::temp_dir().join(crate::config::new_id());
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("config.dpapi");
        write(&path, b"ORIGINAL_FAKE_CONFIG").unwrap();
        let original = std::fs::read(&path).unwrap();
        let reader = OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        let error = write(&path, b"REJECTED_FAKE_CONFIG").unwrap_err();
        assert!(error.contains("原配置保持不变"));
        drop(reader);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(read(&path).unwrap(), b"ORIGINAL_FAKE_CONFIG");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn dpapi_roundtrip_and_tamper() {
        let plain = b"TEST_ONLY_SECRET_NOT_A_REAL_KEY";
        let mut cipher = transform(plain, true).unwrap();
        assert!(!cipher.windows(plain.len()).any(|x| x == plain));
        assert_eq!(transform(&cipher, false).unwrap(), plain);
        let middle = cipher.len() / 2;
        cipher[middle] ^= 0x55;
        assert!(transform(&cipher, false).is_err());
    }
}
