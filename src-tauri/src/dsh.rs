use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::{io::Write, path::{Path, PathBuf}, process::{Command, Stdio}};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub profile: String,
    pub configured: bool,
    pub model_count: usize,
    pub zen_count: usize,
    pub message: String,
    pub backup_directory: String,
}

pub fn default_profile() -> String {
    std::env::var_os("DSH_HOME").map(PathBuf::from)
        .or_else(|| Path::new("F:\\dsh-home").is_dir().then(|| PathBuf::from("F:\\dsh-home")))
        .map(|home| home.join("profiles").join("web").display().to_string()).unwrap_or_default()
}

// Commands use stdin, not argv, for the random loopback token. No upstream
// credentials, prompts or arbitrary headers are sent to this helper.
pub fn execute(directory: &Path, config: &Config, action: &str, port: u16) -> Result<Status, String> {
    if config.settings.dsh_profile.trim().is_empty() { return Err("请在 DSH 接入页设置 profile 目录".into()); }
    let script = directory.join("dsh-bridge.cjs");
    std::fs::write(&script, include_str!("dsh_bridge.cjs")).map_err(|_| "接入组件写入失败")?;
    let node = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Program Files")).join("nodejs/node.exe");
    let mut cmd=Command::new(node);
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child=cmd.arg(script).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_| "启动 DSH Node 接入组件失败，请检查 Node.js 安装")?;
    let models: Vec<_> = config.models.iter().filter(|m| m.enabled).map(|m| serde_json::json!({
        "id":m.id,"name":m.name,"model":m.model,"protocol":m.protocol,"contextWindow":m.context_window,"maxOutput":m.max_output
    })).collect();
    let request=serde_json::json!({"action":action,"profile":config.settings.dsh_profile,"owner":directory.display().to_string(),"pid":std::process::id(),"port":port,"token":config.gateway_token,"models":models});
    let data=serde_json::to_vec(&request).map_err(|_| "接入参数序列化失败")?;
    child.stdin.take().ok_or("接入管道异常")?.write_all(&data).map_err(|_| "接入参数发送失败")?;
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(20);
    loop {
        if child.try_wait().map_err(|_| "接入进程状态异常")?.is_some() { break; }
        if std::time::Instant::now()>deadline { let _=child.kill(); let _=child.wait(); return Err("接入操作超时；请点击断开并恢复，软件会核对上次事务".into()); }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    let output=child.wait_with_output().map_err(|_| "读取接入状态失败")?;
    let result: serde_json::Value=serde_json::from_slice(&output.stdout).map_err(|_| "接入组件返回无效状态")?;
    if !output.status.success() || result["ok"]!=true { return Err(result["error"].as_str().unwrap_or("DSH 接入失败").into()); }
    serde_json::from_value(result["value"].clone()).map_err(|_| "DSH 接入状态解析失败".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn legacy_settings_include_profile() {
        let value: crate::config::Settings=serde_json::from_str(r#"{"port":8787,"timeoutSeconds":120,"theme":"light","logLimit":200}"#).unwrap();
        assert_eq!(value.dsh_profile, super::default_profile());
    }

    #[test]
    #[ignore = "Requires installed YAML parser anchor; integration runner only"]
    fn rust_bridge_subprocess_sync_restore() {
        use super::*;
        let anchor=std::env::var("DSH_PATRICK_YAML_ANCHOR").unwrap();
        let root=std::env::temp_dir().join(crate::config::new_id());
        let profile=root.join("profiles/web");
        let directory=root.join("patrick-data");
        std::fs::create_dir_all(profile.join("node_modules/yaml")).unwrap();
        std::fs::create_dir_all(&directory).unwrap();
        // A parser-only shim; no DSH application or genuine credentials are loaded.
        let shim=format!("module.exports=require('node:module').createRequire({})('yaml');",serde_json::to_string(&anchor).unwrap());
        std::fs::write(profile.join("node_modules/yaml/index.js"),shim).unwrap();
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
