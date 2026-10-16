use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::{io::{Read, Write}, path::{Path, PathBuf}, process::{Command, Stdio}};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub profile: String,
    pub configured: bool,
    pub model_count: usize,
    pub zen_count: usize,
    #[serde(default)] pub model_vision: std::collections::BTreeMap<String,bool>,
    pub message: String,
    pub backup_directory: String,
    #[serde(default)] pub profile_valid: bool,
    #[serde(default)] pub checked_at: u64,
    #[serde(default)] pub sync_state: String,
    #[serde(default)] pub checks: Vec<Check>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Check { pub id: String, pub label: String, pub state: String, pub detail: String }
#[derive(Debug)]
pub struct Failure { pub message: String, pub diagnostics: Option<Status> }
impl From<&str> for Failure { fn from(message:&str)->Self {Self {message:message.into(),diagnostics:None}} }
impl From<String> for Failure { fn from(message:String)->Self {Self {message,diagnostics:None}} }
impl Status {
    pub fn failed(profile: String, error: Failure) -> Self {
        let mut status=error.diagnostics.unwrap_or_default();
        status.profile=profile;status.configured=false;status.model_count=0;status.zen_count=0;
        status.message=error.message;
        if status.checked_at==0 {status.checked_at=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;}
        status
    }
}

// Do not prefill a development-machine path or guess among multiple installations.
pub fn default_profile() -> String { String::new() }

// A profile may be attached by another portable copy. Exiting this copy must
// not restore that copy's routes. The bridge rechecks ownership under its locks
// before an owned restore, so a journal change here never bypasses that guard.
pub fn owns_integration(directory: &Path, profile: &str) -> Result<bool, String> {
    if profile.trim().is_empty() { return Ok(false); }
    let path=Path::new(profile).join(".dsh-patrick/journal.json");
    let bytes=match std::fs::read(path) {
        Ok(bytes)=>bytes,
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(false),
        Err(_)=>return Err("读取 DSH 接入归属失败；接入记录保持不变，请核对后重试".into()),
    };
    let journal:serde_json::Value=serde_json::from_slice(&bytes)
        .map_err(|_| "DSH 接入记录格式错误；原文件保持不变，请核对后重试")?;
    let owner=journal.get("owner").and_then(serde_json::Value::as_str).filter(|s| !s.is_empty())
        .ok_or("DSH 接入记录缺少归属；原文件保持不变，请核对后重试")?;
    Ok(owner==directory.to_string_lossy())
}

// Commands use stdin, not argv, for the random loopback token. No upstream
// credentials, prompts or arbitrary headers are sent to this helper.
pub fn execute(directory: &Path, config: &Config, action: &str, port: u16) -> Result<Status, String> {
    execute_detailed(directory,config,action,port).map_err(|error|error.message)
}

pub fn execute_detailed(directory: &Path, config: &Config, action: &str, port: u16) -> Result<Status, Failure> {
    let models: Vec<_> = config.models.iter().filter(|m| m.enabled).map(|m| serde_json::json!({
        "id":m.id,"name":m.name,"model":m.model,"protocol":m.protocol,"contextWindow":m.context_window,"maxOutput":m.max_output,"compactionPercent":m.compaction_percent,"multimodal":m.multimodal
    })).collect();
    let request=serde_json::json!({"action":action,"profile":config.settings.dsh_profile,"owner":directory.display().to_string(),"pid":std::process::id(),"port":port,"token":config.gateway_token,"models":models,"dshStreamIdleSeconds":config.settings.dsh_stream_idle_seconds});
    let mut status:Status=serde_json::from_value(call(directory,request)?).map_err(|_| "DSH 接入状态解析失败")?;
    status.profile=config.settings.dsh_profile.clone();
    Ok(status)
}

// Portable releases carry runtime/node.exe. The target DSH package is never a runtime source.
fn runtime_node() -> Result<PathBuf,String> {
    let exe=std::env::current_exe().map_err(|_| "读取接入运行时路径失败")?;
    let packaged=exe.parent().ok_or("读取软件目录失败")?.join("runtime/node.exe");
    if packaged.is_file() { return Ok(packaged); }
    #[cfg(debug_assertions)] {
        let dev=std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(||PathBuf::from("C:\\Program Files")).join("nodejs/node.exe");
        if dev.is_file() {return Ok(dev);}
    }
    Err("Patrick 接入运行时缺失，请完整解压交付包并保留 EXE 旁的 runtime 目录；无需重装 DSH".into())
}

fn call(directory:&Path, request:serde_json::Value) -> Result<serde_json::Value,Failure> {
    // Serialize script extraction and child execution, including periodic status checks.
    static SERIAL:std::sync::Mutex<()>=std::sync::Mutex::new(());
    let _serial=SERIAL.lock().map_err(|_| "接入组件锁异常")?;
    let script=directory.join("dsh-bridge.cjs");
    let bytes=include_bytes!("dsh_bridge.bundle.cjs");
    if std::fs::read(&script).ok().as_deref()!=Some(bytes.as_slice()) {
        std::fs::write(&script,bytes).map_err(|_| "接入组件写入失败")?;
    }
    let mut cmd=Command::new(runtime_node()?);
    // Do not allow inherited Node preload/module search hooks to load target code.
    cmd.env_remove("NODE_OPTIONS").env_remove("NODE_PATH").env_remove("ELECTRON_RUN_AS_NODE");
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child=cmd.arg(&script).current_dir(directory).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_| "Patrick 接入组件启动失败，请检查交付包 runtime 目录是否完整")?;
    // Drain stdout concurrently so status output never fills a pipe while waiting for exit.
    let stdout=child.stdout.take().ok_or("接入输出管道异常")?;
    let reader=std::thread::spawn(move || {let mut data=Vec::new();stdout.take(2*1024*1024).read_to_end(&mut data).map(|_|data)});
    let data=serde_json::to_vec(&request).map_err(|_| "接入参数序列化失败")?;
    let sent=child.stdin.take().ok_or("接入管道异常")?.write_all(&data);
    if sent.is_err() {let _=child.kill();let _=child.wait();let _=reader.join();return Err("接入参数发送失败".into());}
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(20);
    let status=loop {
        match child.try_wait() {
            Ok(Some(status))=>break status,
            Ok(None)=>{},
            Err(_)=>{let _=child.kill();let _=child.wait();let _=reader.join();return Err("接入进程状态异常".into());}
        }
        if std::time::Instant::now()>deadline {
            let _=child.kill();let _=child.wait();let _=reader.join();
            return Err("接入操作超时；请点击断开并恢复，软件会核对上次事务".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    };
    let output=reader.join().map_err(|_| "接入输出任务异常")?.map_err(|_| "读取接入状态失败")?;
    let result:serde_json::Value=serde_json::from_slice(&output).map_err(|_| "接入组件返回无效状态")?;
    if !status.success() || result["ok"]!=true {
        return Err(Failure {message:result["error"].as_str().unwrap_or("DSH 接入失败").into(),diagnostics:serde_json::from_value(result["diagnostics"].clone()).ok()});
    }
    Ok(result["value"].clone())
}

// Called only by the explicit, isolated native smoke route. Never reads a real profile.
pub fn smoke_probe(directory:&Path) -> Result<(),String> {
    let root=directory.join(format!("bridge-fixture-{}",crate::config::new_id()));
    let profile=root.join("profiles/desktop");let data=root.join("patrick-data");
    let io_error=|_| "接入组件夹具文件操作失败".to_string();
    std::fs::create_dir_all(profile.join("node_modules/yaml")).map_err(io_error)?;
    std::fs::create_dir_all(&data).map_err(io_error)?;
    std::fs::write(profile.join("package.json"),"{}").map_err(io_error)?;
    std::fs::write(profile.join("node_modules/yaml/index.js"),"throw new Error('TARGET_MODULE_MUST_NOT_RUN')").map_err(io_error)?;
    let original="# preserve comment\n- id: llm-pi-ai\n  config:\n    providers: {}\n- id: default-model\n  config: {model: untouched}\n";
    let credentials="version: 1\nrefs:\n  ORIGINAL_TEST: SYNTHETIC_ONLY\n";
    std::fs::write(profile.join("cordis.patch.yml"),original).map_err(io_error)?;
    std::fs::write(root.join(".credentials.yaml"),credentials).map_err(io_error)?;
    let mut config=Config {gateway_token:format!("patrick_{}","a".repeat(64)),..Config::default()};
    config.settings.dsh_profile=profile.display().to_string();
    config.models.push(serde_json::from_value(serde_json::json!({
        "id":"native_local","name":"Synthetic only","model":"synthetic","protocol":"openai",
        "baseUrl":"http://127.0.0.1:9/v1","apiKey":"SYNTHETIC_KEY","headers":{},
        "enabled":true,"reasoning":"default","prompt":""
    })).map_err(|_| "合成模型构造失败")?);
    if execute(&data,&config,"inspect",48787)?.configured {return Err("合成目录初始状态异常".into());}
    let sync=execute(&data,&config,"sync",48787)?;
    if !sync.configured || sync.model_count!=1 || !execute(&data,&config,"status",48787)?.configured {return Err("接入组件合成同步失败".into());}
    if execute(&data,&config,"disconnect",48787)?.configured {return Err("接入组件合成恢复失败".into());}
    if std::fs::read(profile.join("cordis.patch.yml")).map_err(io_error)?!=original.as_bytes()
        || std::fs::read(root.join(".credentials.yaml")).map_err(io_error)?!=credentials.as_bytes()
        || profile.join(".dsh-patrick/journal.json").exists() {return Err("接入夹具恢复字节不一致".into());}
    Ok(())
}

// Explicit smoke fixtures only; the caller's real DSH path is never accepted.
pub fn smoke_diagnostics(directory:&Path, scenario:&str) -> Result<Status,String> {
    let root=directory.join(format!("diagnostic-fixture-{}",crate::config::new_id()));
    let profile=root.join("profiles/desktop");let data=root.join("patrick-data");
    let io_error=|_| "诊断夹具文件操作失败".to_string();
    std::fs::create_dir_all(&profile).map_err(io_error)?;std::fs::create_dir_all(&data).map_err(io_error)?;
    std::fs::write(profile.join("package.json"),"{}").map_err(io_error)?;
    let original="- id: llm-pi-ai\n  config: {providers: {}}\n";
    std::fs::write(profile.join("cordis.patch.yml"),if scenario=="invalid" {"[SYNTHETIC_SECRET_NEVER_ECHO"} else {original}).map_err(io_error)?;
    let mut config=Config {gateway_token:format!("patrick_{}","a".repeat(64)),..Config::default()};
    config.settings.dsh_profile=profile.display().to_string();
    if scenario=="synced" {execute(&data,&config,"sync",48787)?;}
    let status=match execute_detailed(&data,&config,"status",48787) {Ok(status)=>status,Err(error)=>Status::failed(config.settings.dsh_profile.clone(),error)};
    if scenario=="synced" {
        execute(&data,&config,"disconnect",48787)?;
        if std::fs::read_to_string(profile.join("cordis.patch.yml")).map_err(io_error)?!=original {return Err("诊断夹具恢复不一致".into());}
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    #[test]
    fn structured_diagnostics_survive_subprocess_and_reset() {
        let directory=std::env::temp_dir().join(crate::config::new_id());std::fs::create_dir_all(&directory).unwrap();
        let invalid=super::smoke_diagnostics(&directory,"invalid").unwrap();
        assert!(!invalid.profile_valid&&!invalid.configured);
        assert!(invalid.checks.iter().any(|c|c.id=="yaml"&&c.state=="fail"));
        assert!(invalid.message.contains("cordis.patch.yml")&&invalid.message.contains("第 1 行"));
        assert!(!invalid.message.contains("SYNTHETIC_SECRET"));
        let valid=super::smoke_diagnostics(&directory,"valid").unwrap();
        assert!(valid.profile_valid&&!valid.configured);assert_eq!(valid.sync_state,"not_synced");
        let synced=super::smoke_diagnostics(&directory,"synced").unwrap();
        assert!(synced.profile_valid&&synced.configured);assert_eq!(synced.sync_state,"synced");
        let runtime=super::Status::failed("new-profile".into(),super::Failure::from("运行时测试失败"));
        assert!(!runtime.profile_valid&&!runtime.configured&&runtime.checks.is_empty());
        assert_eq!(runtime.profile,"new-profile");assert!(runtime.checked_at>0);
        println!("RUST_DIAGNOSTICS_PASS stages=PRESERVED errors=REDACTED runtime_failure=NO_STALE_STATE restore=EXACT");
    }

    #[test]
    fn legacy_settings_include_profile() {
        let value: crate::config::Settings=serde_json::from_str(r#"{"port":8787,"timeoutSeconds":120,"theme":"light","logLimit":200}"#).unwrap();
        assert!(value.dsh_profile.is_empty());
        let saved:crate::config::Settings=serde_json::from_str(r#"{"dshProfile":"D:/Chosen/profiles/custom","port":8787,"timeoutSeconds":120,"theme":"light","logLimit":200}"#).unwrap();
        assert_eq!(saved.dsh_profile,"D:/Chosen/profiles/custom");
    }

    #[test]
    fn rust_bridge_subprocess_sync_restore() {
        use super::*;
        let root=std::env::temp_dir().join(crate::config::new_id());
        let profile=root.join("profiles/web");
        let directory=root.join("patrick-data");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(profile.join("package.json"),"{}").unwrap();
        let original="- id: llm-pi-ai\n  config:\n    providers: {}\n";
        std::fs::write(profile.join("cordis.patch.yml"),original).unwrap();
        let mut config=Config { gateway_token:format!("patrick_{}","a".repeat(64)),..Config::default() };
        config.settings.dsh_profile=profile.display().to_string();
        assert!(!execute(&directory,&config,"inspect",48787).unwrap().configured);
        assert!(execute(&directory,&config,"sync",48787).unwrap().configured);
        assert!(execute(&directory,&config,"status",48787).unwrap().configured);
        assert!(!execute(&directory,&config,"disconnect",48787).unwrap().configured);
        assert_eq!(std::fs::read_to_string(profile.join("cordis.patch.yml")).unwrap(),original);
        assert!(!root.join(".credentials.yaml").exists());
        println!("RUST_BRIDGE PASS stdin_transport=YES sync=YES exact_restore=YES isolated_fixture={}",root.display());
        // Keep this bounded fixture for the integration evidence, not in a release package.
    }
}
