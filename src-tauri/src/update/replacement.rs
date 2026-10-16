//! Small Windows replacement process; fixed file whitelist, no shell commands.
use super::{check_file,digest_file,regular,verify,version,write_new,Manifest,FILES};
use serde::{Deserialize,Serialize};
use std::{collections::BTreeMap,fs,io::Write,path::{Path,PathBuf},process::{Child,Command},time::{Duration,Instant}};
use std::os::windows::{ffi::OsStrExt,process::CommandExt,fs::OpenOptionsExt};
const APPLY:&str="--patrick-apply-update";
const START:&str="--patrick-update-start";
const RECOVER:&str="--patrick-update-recover";
const NO_WINDOW:u32=0x08000000;
const PROCESS_WAIT:Duration=Duration::from_secs(30);
const READY_WAIT:Duration=Duration::from_secs(60);
#[link(name="Kernel32")]
extern "system" {
    fn MoveFileExW(from:*const u16,to:*const u16,flags:u32)->i32;
    fn OpenProcess(access:u32,inherit:i32,pid:u32)->*mut std::ffi::c_void;
    fn WaitForSingleObject(handle:*mut std::ffi::c_void,ms:u32)->u32;
    fn CloseHandle(handle:*mut std::ffi::c_void)->i32;
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Plan {
    pub target_name:String,pub current_version:String,pub old_pid:u32,pub nonce:String,
    pub data_directory:PathBuf,pub resume_gateway:bool,pub old_files:BTreeMap<String,Option<String>>,
}
pub struct Context {pub stage:PathBuf,pub root:PathBuf,pub target:PathBuf,pub manifest:Manifest,pub plan:Plan}
fn read(path:&Path)->Result<Vec<u8>,String>{regular(path)?;if fs::metadata(path).map_err(|_|"更新事务文件缺失")?.len()>65536 {return Err("更新事务文件过大".into());}fs::read(path).map_err(|_|"读取更新事务失败".into())}
fn target_path(root:&Path,name:&str,component:&str)->PathBuf{root.join(if component==FILES[0]{name}else{component})}
fn stage_root(stage:&Path)->Result<PathBuf,String>{
    regular(stage)?;
    let parent=stage.parent().ok_or("更新暂存路径无效")?;
    if !stage.is_absolute() || parent.file_name().and_then(|s|s.to_str())!=Some(".patrick-updates") || !stage.is_dir() {return Err("更新暂存目录无效".into());}
    Ok(parent.parent().ok_or("更新程序目录无效")?.to_path_buf())
}
pub fn load(stage:&Path)->Result<Context,String>{
    let root=stage_root(stage)?;let manifest=verify(&read(&stage.join("portable-latest.json"))?)?;
    let plan:Plan=serde_json::from_slice(&read(&stage.join("plan.json"))?).map_err(|_|"更新事务格式无效")?;
    if plan.target_name.is_empty() || plan.target_name.chars().any(|c|"\\/:\0".contains(c)) || !plan.target_name.ends_with(".exe")
        || plan.nonce.len()!=64 || !plan.nonce.bytes().all(|b|b.is_ascii_hexdigit()) || !plan.data_directory.is_absolute()
        || plan.old_files.len()!=FILES.len() || FILES.iter().any(|n|!plan.old_files.contains_key(*n))
        || plan.old_files.values().flatten().any(|h|h.len()!=64 || !h.bytes().all(|b|b.is_ascii_hexdigit()))
        || plan.old_files.get(FILES[0]).and_then(|v|v.as_ref()).is_none() {return Err("更新事务目标无效".into());}
    if version(&manifest.version)?<=version(&plan.current_version)? {return Err("更新事务拒绝降级或同版本替换".into());}
    regular(&plan.data_directory)?;
    let target=root.join(&plan.target_name);regular(&target)?;
    Ok(Context{stage:stage.to_path_buf(),root,target,manifest,plan})
}
pub fn prepare(stage:&Path,target:&Path,data:&Path,resume:bool)->Result<(),String>{
    let root=stage_root(stage)?;
    if target.parent()!=Some(root.as_path()){return Err("更新目标与暂存目录不一致".into());}
    let name=target.file_name().and_then(|v|v.to_str()).ok_or("程序文件名无效")?.to_owned();
    let mut old_files=BTreeMap::new();
    for file in FILES {let p=target_path(&root,&name,file);regular(&p)?;old_files.insert(file.into(),if p.exists(){Some(digest_file(&p)?)}else{None});}
    let mut random=[0u8;32];getrandom::fill(&mut random).map_err(|_|"生成更新握手失败")?;
    let plan=Plan{target_name:name,current_version:env!("CARGO_PKG_VERSION").into(),old_pid:std::process::id(),nonce:random.iter().map(|b|format!("{b:02x}")).collect(),data_directory:data.to_path_buf(),resume_gateway:resume,old_files};
    write_new(&stage.join("plan.json"),&serde_json::to_vec(&plan).map_err(|_|"编码更新事务失败")?)?;
    fs::copy(target,stage.join("helper.exe")).map_err(|_|"准备更新替换进程失败")?;
    let ctx=load(stage)?;verify_pending(&ctx)?;Ok(())
}
fn verify_pending(ctx:&Context)->Result<(),String>{
    if ctx.plan.current_version!=env!("CARGO_PKG_VERSION"){return Err("更新替换进程版本不匹配".into());}
    for file in &ctx.manifest.files {check_file(&ctx.stage.join(&file.path),file)?;}
    let expected=ctx.plan.old_files[FILES[0]].as_ref().unwrap();
    if digest_file(&ctx.stage.join("helper.exe"))?!=*expected {return Err("旧程序或更新替换进程已变化".into());}
    verify_old(ctx)
}
fn verify_old(ctx:&Context)->Result<(),String>{
    for (file,hash) in &ctx.plan.old_files {let p=target_path(&ctx.root,&ctx.plan.target_name,file);regular(&p)?;
        let actual=if p.exists(){Some(digest_file(&p)?)}else{None};
        if actual!=*hash{return Err("程序组件在更新期间被修改，已停止替换".into());}
    }Ok(())
}
pub fn spawn(stage:&Path)->Result<Child,String>{
    Command::new(stage.join("helper.exe")).arg(APPLY).arg(stage).creation_flags(NO_WINDOW).spawn().map_err(|_|"启动更新替换进程失败".into())
}
pub async fn handshake(child:&mut Child,stage:&Path)->Result<(),String>{
    let nonce=load(stage)?.plan.nonce;let started=Instant::now();
    loop {
        if fs::read_to_string(stage.join("started")).ok().as_deref()==Some(nonce.as_str()){return Ok(());}
        if child.try_wait().map_err(|_|"读取更新进程状态失败")?.is_some(){return Err("更新替换进程提前结束，原程序保持运行".into());}
        if started.elapsed()>Duration::from_secs(5){let _=child.kill();let _=child.wait();return Err("更新替换进程启动超时，原程序保持运行".into());}
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
pub fn atomic_copy(from:&Path,to:&Path,nonce:&str)->Result<(),String>{
    regular(from)?;regular(to)?;
    let temp=to.with_file_name(format!(".patrick-{nonce}-{}.tmp",to.file_name().unwrap().to_string_lossy()));
    let mut input=fs::File::open(from).map_err(|_|"读取替换源失败")?;
    let mut out=fs::OpenOptions::new().create_new(true).write(true).open(&temp).map_err(|_|"创建替换临时文件失败")?;
    std::io::copy(&mut input,&mut out).and_then(|_|out.flush()).and_then(|_|out.sync_all()).map_err(|_|"写入替换临时文件失败")?;
    drop(out);drop(input);
    let a:Vec<u16>=temp.as_os_str().encode_wide().chain(Some(0)).collect();let b:Vec<u16>=to.as_os_str().encode_wide().chain(Some(0)).collect();
    for attempt in 0..60 {
        regular(to)?;
        if unsafe{MoveFileExW(a.as_ptr(),b.as_ptr(),1|8)}!=0{return Ok(());}
        if attempt==59 {let _=fs::remove_file(&temp);return Err("替换程序文件失败，可能仍被其他进程占用".into());}
        std::thread::sleep(Duration::from_millis(100));
    }unreachable!()
}
fn wait_old(pid:u32)->Result<(),String>{
    let handle=unsafe{OpenProcess(0x100000,0,pid)};
    if handle.is_null(){
        if std::io::Error::last_os_error().raw_os_error()==Some(87){return Ok(());}
        return Err("核对旧进程失败，未替换程序".into());
    }
    let result=unsafe{WaitForSingleObject(handle,PROCESS_WAIT.as_millis() as u32)};unsafe{CloseHandle(handle);}
    if result==0{Ok(())}else{Err("等待旧程序退出超时，未强制结束旧程序".into())}
}
fn launch(ctx:&Context,recover:bool)->Result<Child,String>{
    Command::new(&ctx.target).arg(if recover{RECOVER}else{START}).arg(&ctx.stage)
        .env("DSH_PATRICK_DATA",&ctx.plan.data_directory).current_dir(&ctx.root).creation_flags(NO_WINDOW).spawn().map_err(|_|"启动更新后的程序失败".into())
}
fn ready_path(stage:&Path,recover:bool)->PathBuf{stage.join(if recover{"recovered"}else{"ready"})}
fn wait_ready(ctx:&Context,child:&mut Child,recover:bool)->Result<(),String>{
    let expected=serde_json::json!({"nonce":ctx.plan.nonce,"version":if recover{&ctx.plan.current_version}else{&ctx.manifest.version},"pid":child.id()});
    let deadline=Instant::now()+READY_WAIT;
    loop {
        if child.try_wait().map_err(|_|"读取新版进程状态失败")?.is_some(){return Err("新版在初始化完成前退出".into());}
        let marker=fs::read(ready_path(&ctx.stage,recover)).ok().filter(|b|b.len()<4096).and_then(|b|serde_json::from_slice::<serde_json::Value>(&b).ok());
        if marker.as_ref()==Some(&expected){return Ok(());}
        if Instant::now()>deadline{return Err("新版初始化超时".into());}
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn outcome(ctx:&Context,state:&str,message:&str){
    let bytes=serde_json::to_vec(&serde_json::json!({"state":state,"version":ctx.manifest.version,"message":message})).unwrap();
    let temp=ctx.stage.join(format!("result-{}",crate::config::new_id()));
    if write_new(&temp,&bytes).is_ok(){let _=atomic_copy(&temp,&ctx.root.join(".patrick-update-result.json"),&crate::config::new_id());}
}
fn restore(ctx:&Context,replaced:&[String])->Result<(),String>{
    for file in replaced.iter().rev(){
        let to=target_path(&ctx.root,&ctx.plan.target_name,file);regular(&to)?;
        let artifact=ctx.manifest.files.iter().find(|f|f.path==*file).unwrap();
        if digest_file(&to)?!=artifact.sha256{return Err("新版文件又被外部修改，保留备份待手动恢复".into());}
        match &ctx.plan.old_files[file] {
            Some(hash)=>{let from=ctx.stage.join("backup").join(file);if digest_file(&from)?!=*hash{return Err("旧版备份校验失败".into());}atomic_copy(&from,&to,&crate::config::new_id())?;},
            None=>fs::remove_file(to).map_err(|_|"撤销新增更新组件失败")?,
        }
    }verify_old(ctx)
}
fn run_helper(stage:&Path)->Result<(),String>{
    let ctx=load(stage)?;
    if std::env::current_exe().map_err(|_|"读取替换进程路径失败")?!=stage.join("helper.exe"){return Err("更新替换进程路径不匹配".into());}
    let lock_path=ctx.root.join(".patrick-update.lock");regular(&lock_path)?;
    let _lock=fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(0).open(lock_path).map_err(|_|"已有更新替换任务正在运行")?;
    verify_pending(&ctx)?;write_new(&stage.join("started"),ctx.plan.nonce.as_bytes())?;
    if let Err(error)=wait_old(ctx.plan.old_pid){outcome(&ctx,"error",&error);return Err(error);}
    let mut replaced=Vec::new();let mut child:Option<Child>=None;
    let install=(||{
        verify_old(&ctx)?;
        fs::create_dir(stage.join("backup")).map_err(|_|"创建旧版备份失败")?;
        fs::create_dir(stage.join("backup/runtime")).map_err(|_|"创建运行时备份失败")?;
        for file in &ctx.manifest.files {
            let to=target_path(&ctx.root,&ctx.plan.target_name,&file.path);let before=&ctx.plan.old_files[&file.path];
            if before.as_ref()==Some(&file.sha256){continue;}
            if let Some(hash)=before {let backup=stage.join("backup").join(&file.path);fs::copy(&to,&backup).map_err(|_|"备份旧版组件失败")?;if digest_file(&backup)?!=*hash{return Err("旧版备份校验失败".into());}}
            if file.path==FILES[1] {regular(&ctx.root.join("runtime"))?;fs::create_dir_all(ctx.root.join("runtime")).map_err(|_|"创建运行时目录失败")?;}
            check_file(&stage.join(&file.path),file)?;
            atomic_copy(&stage.join(&file.path),&to,&ctx.plan.nonce)?;replaced.push(file.path.clone());check_file(&to,file)?;
        }
        child=Some(launch(&ctx,false)?);wait_ready(&ctx,child.as_mut().unwrap(),false)?;Ok::<_,String>(())
    })();
    if let Err(error)=install {
        if let Some(mut c)=child {let _=c.kill();let _=c.wait();}
        if let Err(restore_error)=restore(&ctx,&replaced){let full=format!("{error}；{restore_error}");outcome(&ctx,"recovery_required",&full);return Err(full);}
        let message=format!("更新失败，已恢复旧版：{error}");outcome(&ctx,"rolled_back",&message);
        let mut old=launch(&ctx,true)?;
        if let Err(e)=wait_ready(&ctx,&mut old,true){outcome(&ctx,"recovery_required",&format!("旧版文件已恢复，但自动启动未完成：{e}"));return Err(e);}
        return Err(message);
    }
    outcome(&ctx,"success",&format!("已更新至 v{}",ctx.manifest.version));Ok(())
}
pub fn run_if_requested()->Option<i32>{
    let args:Vec<_>=std::env::args_os().collect();
    if args.get(1).is_some_and(|s|s==APPLY){
        let result=if args.len()==3 {run_helper(Path::new(&args[2]))}else{Err("更新参数无效".into())};
        if let Err(ref e)=result {eprintln!("UPDATE_REPLACEMENT_FAILED: {e}");}
        Some(if result.is_ok(){0}else{1})
    }else{None}
}
pub struct Startup {pub ctx:Context,pub recover:bool}
pub fn startup()->Result<Option<Startup>,String>{
    let args:Vec<_>=std::env::args_os().collect();
    let recover=args.get(1).is_some_and(|v|v==RECOVER);
    if !recover && !args.get(1).is_some_and(|v|v==START){return Ok(None);}
    if args.len()!=3{return Err("更新启动参数无效".into());}
    let ctx=load(Path::new(&args[2]))?;
    if std::env::current_exe().map_err(|_|"读取新版路径失败")?!=ctx.target {return Err("更新启动路径不匹配".into());}
    if env!("CARGO_PKG_VERSION")!=if recover{&ctx.plan.current_version}else{&ctx.manifest.version}{return Err("新版程序版本与签名清单不匹配".into());}
    if recover{verify_old(&ctx)?;}else{for file in &ctx.manifest.files {check_file(&target_path(&ctx.root,&ctx.plan.target_name,&file.path),file)?;}}
    Ok(Some(Startup{ctx,recover}))
}
pub fn signal_ready(start:&Startup)->Result<(),String>{
    let result=serde_json::json!({"nonce":start.ctx.plan.nonce,"version":env!("CARGO_PKG_VERSION"),"pid":std::process::id()});
    let path=ready_path(&start.ctx.stage,start.recover);
    if path.exists(){return Err("更新启动握手已存在".into());}
    write_new(&path,&serde_json::to_vec(&result).unwrap())
}
