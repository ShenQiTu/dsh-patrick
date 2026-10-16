//! Signed, component-whitelisted portable updates. Never reads model credentials.
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use ring::{digest, signature};
use serde::{Deserialize, Serialize};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, sync::{Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, SystemTime, UNIX_EPOCH}};
pub mod replacement;
#[cfg(test)] mod tests;

const ENDPOINT: &str = "https://github.com/ShenQiTu/dsh-patrick/releases/latest/download/portable-latest.json";
const DOWNLOAD_PREFIX: &str = "https://github.com/ShenQiTu/dsh-patrick/releases/download/";
const PUBLIC_KEY: &str = include_str!("../update-public-key.txt");
const MAX_MANIFEST: usize = 64 * 1024;
const CHECK_INTERVAL: u64 = 6 * 60 * 60;
pub const FILES: [&str; 3] = ["DSH Patrick.exe", "runtime/node.exe", "THIRD_PARTY_NOTICES.md"];
fn now() -> u64 {SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Artifact { pub path: String, pub url: String, pub size: u64, pub sha256: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Manifest { pub schema: u32, pub version: String, pub platform: String, pub data_schema: u32, pub files: Vec<Artifact> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope { payload: String, signature: String }

pub fn version(value: &str) -> Result<semver::Version,String> {
    let parsed=semver::Version::parse(value).map_err(|_|"更新版本号格式错误")?;
    if !parsed.pre.is_empty() || !parsed.build.is_empty() {return Err("仅接受正式更新版本".into());}
    Ok(parsed)
}
fn validate_manifest(m:&Manifest) -> Result<(),String> {
    version(&m.version)?;
    if m.schema!=1 || m.data_schema!=1 || m.platform!="windows-x86_64" {return Err("更新包的平台或数据格式不兼容".into());}
    if m.files.len()!=FILES.len() {return Err("更新组件列表不完整".into());}
    for (file,path) in m.files.iter().zip(FILES) {
        let asset=match path {"DSH Patrick.exe"=>format!("DSH-Patrick-v{}-windows-x64.exe",m.version),"runtime/node.exe"=>"patrick-node-windows-x64.exe".into(),_=>"THIRD_PARTY_NOTICES.md".into()};
        let maximum=if path.ends_with(".exe") {256*1024*1024} else {1024*1024};
        if file.path!=path || file.url!=format!("{DOWNLOAD_PREFIX}v{}/{asset}",m.version) || file.size==0 || file.size>maximum
            || file.sha256.len()!=64 || !file.sha256.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)) {
            return Err("更新文件路径、下载地址、大小或摘要无效".into());
        }
    }
    Ok(())
}
fn verify_with_key(bytes:&[u8], key:&[u8]) -> Result<Manifest,String> {
    if bytes.len()>MAX_MANIFEST {return Err("更新清单过大".into());}
    let envelope:Envelope=serde_json::from_slice(bytes).map_err(|_|"更新清单格式错误")?;
    let payload=STANDARD.decode(envelope.payload).map_err(|_|"更新清单编码错误")?;
    let sig=STANDARD.decode(envelope.signature).map_err(|_|"更新签名编码错误")?;
    signature::UnparsedPublicKey::new(&signature::ED25519,key).verify(&payload,&sig).map_err(|_|"更新签名验证失败，未安装")?;
    let m:Manifest=serde_json::from_slice(&payload).map_err(|_|"已签名更新清单格式错误")?;
    validate_manifest(&m)?;Ok(m)
}
pub fn verify(bytes:&[u8]) -> Result<Manifest,String> {
    let key=STANDARD.decode(PUBLIC_KEY.trim()).map_err(|_|"更新公钥格式错误")?;
    verify_with_key(bytes,&key)
}
pub fn digest_file(file:&Path) -> Result<String,String> {
    let mut f=fs::File::open(file).map_err(|_|"读取更新文件失败")?;
    let mut h=digest::Context::new(&digest::SHA256);let mut buffer=[0u8;65536];
    loop {let n=f.read(&mut buffer).map_err(|_|"读取更新文件失败")?;if n==0 {break;}h.update(&buffer[..n]);}
    Ok(h.finish().as_ref().iter().map(|b|format!("{b:02x}")).collect())
}
pub fn regular(path:&Path) -> Result<(),String> {
    use std::os::windows::fs::MetadataExt;
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(m) if m.file_attributes()&0x400!=0=>return Err("更新路径包含目录链接，请使用普通便携目录".into()),
            Ok(_)=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(_)=>return Err("检查更新路径失败".into()),
        }
    }
    Ok(())
}
pub fn check_file(path:&Path, file:&Artifact) -> Result<(),String> {
    regular(path)?;
    if fs::metadata(path).map_err(|_|"更新文件缺失")?.len()!=file.size || digest_file(path)?!=file.sha256 {return Err("更新文件大小或 SHA256 不匹配，未安装".into());}
    if file.path.ends_with(".exe") {
        use std::io::{Seek,SeekFrom};
        let mut f=fs::File::open(path).map_err(|_|"读取程序头失败")?;let mut dos=[0;64];
        f.read_exact(&mut dos).map_err(|_|"更新程序头不完整")?;
        let offset=u32::from_le_bytes(dos[60..64].try_into().unwrap()) as u64;
        if &dos[..2]!=b"MZ" || offset>file.size.saturating_sub(26) {return Err("更新文件不是 Windows 程序".into());}
        f.seek(SeekFrom::Start(offset)).map_err(|_|"读取程序头失败")?;let mut pe=[0;26];f.read_exact(&mut pe).map_err(|_|"读取程序头失败")?;
        if &pe[..6]!=b"PE\0\0\x64\x86" || pe[24..26]!=[0x0b,0x02] {return Err("更新程序不是 Windows x64 格式".into());}
    }
    Ok(())
}
pub fn write_new(path:&Path, bytes:&[u8]) -> Result<(),String> {
    regular(path)?;
    let mut f=fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|_|"创建更新临时文件失败")?;
    f.write_all(bytes).and_then(|_|f.sync_all()).map_err(|_|"保存更新文件失败".into())
}
fn client() -> Result<reqwest::Client,String> {
    reqwest::Client::builder().no_proxy().connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600)).redirect(reqwest::redirect::Policy::custom(|a| {
            let u=a.url();
            if a.previous().len()<5 && u.scheme()=="https" && u.username().is_empty() && u.password().is_none()
                && matches!(u.host_str(),Some("github.com"|"release-assets.githubusercontent.com"|"objects.githubusercontent.com")) {a.follow()} else {a.stop()}
        })).build().map_err(|_|"更新客户端初始化失败".into())
}
async fn response(client:&reqwest::Client,url:&str) -> Result<reqwest::Response,String> {
    let r=client.get(url).header("user-agent","DSH-Patrick-Updater").header("cache-control","no-cache")
        .send().await.map_err(|_|"连接更新服务器失败或超时")?;
    match r.status().as_u16() {200=>Ok(r),404=>Err("尚未发布应用内更新清单或更新文件；可打开发布页查看".into()),403|429=>Err("GitHub 暂时限制更新检查频率，请稍后重试".into()),s=>Err(format!("更新服务器返回 HTTP {s}"))}
}
async fn fetch_manifest(client:&reqwest::Client,url:&str) -> Result<(Vec<u8>,Manifest),String> {
    let key=STANDARD.decode(PUBLIC_KEY.trim()).map_err(|_|"更新公钥格式错误")?;
    fetch_manifest_key(client,url,&key).await
}
async fn fetch_manifest_key(client:&reqwest::Client,url:&str,key:&[u8]) -> Result<(Vec<u8>,Manifest),String> {
    let r=tokio::time::timeout(Duration::from_secs(20),response(client,url)).await.map_err(|_|"检查更新超时")??;
    let read=async {
        let mut stream=r.bytes_stream();let mut bytes=Vec::new();
        while let Some(c)=stream.next().await {let c=c.map_err(|_|"读取更新清单失败")?;if bytes.len()+c.len()>MAX_MANIFEST{return Err("更新清单过大".into());}bytes.extend_from_slice(&c);}
        let m=verify_with_key(&bytes,key)?;Ok((bytes,m))
    };
    tokio::time::timeout(Duration::from_secs(20),read).await.map_err(|_|"读取更新清单超时")?
}

#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct View { pub state:String, pub version:String, pub available:bool, pub busy:bool, pub downloaded:u64, pub total:u64, pub message:String, pub last_checked_at:u64, pub last_result:String }
impl Default for View {fn default()->Self {Self{state:"idle".into(),version:String::new(),available:false,busy:false,downloaded:0,total:0,message:String::new(),last_checked_at:0,last_result:String::new()}}}
pub struct Service {view:Mutex<View>,checking:tokio::sync::Mutex<()>,pub installing:AtomicBool,cancel:AtomicBool}
impl Default for Service {fn default()->Self{Self{view:Mutex::new(View::default()),checking:tokio::sync::Mutex::new(()),installing:AtomicBool::new(false),cancel:AtomicBool::new(false)}}}
impl Service {
    pub fn read_last_result(&self){
        let Ok(exe)=std::env::current_exe() else{return};let Some(root)=exe.parent() else{return};let path=root.join(".patrick-update-result.json");
        if regular(&path).is_err() || !fs::metadata(&path).is_ok_and(|m|m.len()<4096){return;}
        let Some(value)=fs::read(&path).ok().and_then(|b|serde_json::from_slice::<serde_json::Value>(&b).ok()) else{return};
        if let (Some(state),Some(message))=(value["state"].as_str(),value["message"].as_str()) {
            if ["success","error","rolled_back","recovery_required"].contains(&state)&&message.chars().count()<=512 {
                self.change(|v|v.last_result=message.into());
            }
        }
    }
    pub fn view(&self)->View {self.view.lock().unwrap_or_else(|e|e.into_inner()).clone()}
    fn change(&self,f:impl FnOnce(&mut View)){f(&mut self.view.lock().unwrap_or_else(|e|e.into_inner()));}
    pub fn phase(&self,state:&str,message:&str){self.change(|v|{v.state=state.into();v.message=message.into();});}
    pub async fn check(&self,background:bool)->Result<View,String>{
        let Ok(_guard)=self.checking.try_lock() else{return Ok(self.view());};
        if self.installing.load(Ordering::SeqCst) || (background && now().saturating_sub(self.view().last_checked_at)<CHECK_INTERVAL){return Ok(self.view());}
        self.change(|v|{v.state="checking".into();v.last_checked_at=now();});
        let result=async {fetch_manifest(&client()?,ENDPOINT).await}.await;
        match result {
            Ok((_,m))=>{
                let ordering=version(&m.version)?.cmp(&version(env!("CARGO_PKG_VERSION"))?);
                self.change(|v|{v.available=ordering.is_gt();v.version=m.version;v.state=if ordering.is_gt(){"available"}else if ordering.is_eq(){"current"}else{"ahead"}.into();v.message=if v.available {format!("发现新版本 v{}",v.version)}else if ordering.is_eq(){"当前已是最新正式版".into()}else{"当前版本高于已发布正式版".into()};});
                Ok(self.view())
            },Err(e)=>{self.phase("error",&e);Err(e)}
        }
    }
    pub fn begin(&self)->Result<(),String>{
        let _checking=self.checking.try_lock().map_err(|_|"正在检查更新，请稍后再试")?;
        self.installing.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).map_err(|_|"更新任务正在进行")?;
        self.cancel.store(false,Ordering::SeqCst);self.change(|v|{v.busy=true;v.downloaded=0;v.total=0;v.message="正在核对更新版本…".into();v.state="downloading".into();});Ok(())
    }
    pub fn cancelled(&self)->Result<(),String>{if self.cancel.load(Ordering::SeqCst){Err("更新已取消，原版本保持不变".into())}else{Ok(())}}
    pub fn cancel(&self)->Result<(),String>{let view=self.view.lock().unwrap_or_else(|e|e.into_inner());if view.state=="installing" {return Err("已开始替换，请等待程序重新打开".into());}self.cancel.store(true,Ordering::SeqCst);Ok(())}
    pub fn enter_installing(&self)->Result<(),String>{
        let mut v=self.view.lock().unwrap_or_else(|e|e.into_inner());self.cancelled()?;v.state="installing".into();v.message="正在安装；完成后将自动打开新版…".into();Ok(())
    }
    pub fn finish_error(&self,e:&str){self.phase("error",e);self.change(|v|v.busy=false);self.installing.store(false,Ordering::SeqCst);}
    pub async fn stage(&self,expected:&str,target:&Path)->Result<PathBuf,String>{
        let c=client()?;let (bytes,m)=fetch_manifest(&c,ENDPOINT).await?;
        if m.version!=expected || version(&m.version)?<=version(env!("CARGO_PKG_VERSION"))? {return Err("可用版本已变化，请重新检查更新".into());}
        self.cancelled()?;regular(target)?;
        let root=target.parent().ok_or("程序目录无效")?;let parent=root.join(".patrick-updates");regular(&parent)?;fs::create_dir_all(&parent).map_err(|_|"程序目录不可写，请将便携包放入可写目录")?;
        let stage=parent.join(crate::config::new_id());fs::create_dir(&stage).map_err(|_|"创建更新暂存目录失败")?;
        fs::create_dir(stage.join("runtime")).map_err(|_|"创建运行时暂存目录失败")?;
        write_new(&stage.join("portable-latest.json"),&bytes)?;
        self.change(|v|v.total=m.files.iter().map(|f|f.size).sum());
        for file in &m.files {
            self.cancelled()?;
            let from=if file.path==FILES[0] {target.to_path_buf()}else{root.join(&file.path)};
            let dest=stage.join(&file.path);regular(&from)?;
            if from.is_file() && check_file(&from,file).is_ok() {
                fs::copy(&from,&dest).map_err(|_|"复用已验证组件失败")?;self.change(|v|v.downloaded+=file.size);
            } else {
                self.download(&c,file,&dest).await?;
            }
            check_file(&dest,file)?;
        }
        self.cancelled()?;Ok(stage)
    }
    async fn download(&self,c:&reqwest::Client,file:&Artifact,dest:&Path)->Result<(),String>{
        self.cancelled()?;regular(dest)?;
        let r=response(c,&file.url).await?;
        if r.content_length().is_some_and(|size|size!=file.size){return Err("更新下载长度不匹配".into());}
        let mut out=fs::OpenOptions::new().write(true).create_new(true).open(dest).map_err(|_|"创建更新下载文件失败")?;
        let mut stream=r.bytes_stream();let mut count=0u64;
        loop {
            self.cancelled()?;
            let chunk=tokio::select!{c=stream.next()=>c,_=tokio::time::sleep(Duration::from_millis(250))=>continue};
            let Some(chunk)=chunk else {break};let chunk=chunk.map_err(|_|"更新下载中断或超时")?;count+=chunk.len() as u64;
            if count>file.size {return Err("更新文件超过已签名大小".into());}
            out.write_all(&chunk).map_err(|_|"保存更新下载文件失败")?;self.change(|v|v.downloaded+=chunk.len() as u64);
        }
        out.sync_all().map_err(|_|"保存更新下载文件失败")?;drop(out);
        check_file(dest,file)
    }
}
