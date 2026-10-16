use super::*;

fn root() -> PathBuf {
    let root=std::env::temp_dir().join(config::new_id());
    std::fs::create_dir(&root).unwrap();root
}

#[test]
fn exit_ownership_missing_foreign_owned_and_invalid() {
    let root=root();let profile=root.join("profiles/web");
    let own=root.join("new-data");let foreign=root.join("old-data");
    let profile_text=profile.to_string_lossy();
    assert!(!dsh::owns_integration(&own,"").unwrap());
    assert!(!dsh::owns_integration(&own,&profile_text).unwrap());
    let directory=profile.join(".dsh-patrick");std::fs::create_dir_all(&directory).unwrap();
    let journal=directory.join("journal.json");
    for (owner,expected) in [(&foreign,false),(&own,true)] {
        let bytes=serde_json::to_vec(&serde_json::json!({"owner":owner})).unwrap();
        std::fs::write(&journal,&bytes).unwrap();
        assert_eq!(dsh::owns_integration(&own,&profile_text).unwrap(),expected);
        assert_eq!(std::fs::read(&journal).unwrap(),bytes);
    }
    for malformed in ["{", "{}", "{\"owner\":null}", "{\"owner\":\"\"}"] {
        std::fs::write(&journal,malformed).unwrap();
        assert!(dsh::owns_integration(&own,&profile_text).is_err());
        assert_eq!(std::fs::read_to_string(&journal).unwrap(),malformed);
    }
    std::fs::remove_file(&journal).unwrap();std::fs::remove_dir(&directory).unwrap();
    std::fs::remove_dir(&profile).unwrap();std::fs::remove_dir(profile.parent().unwrap()).unwrap();std::fs::remove_dir(root).unwrap();
    println!("EXIT_OWNERSHIP_PASS missing=SKIP foreign=SKIP own=RESTORE invalid=ERROR read_only=YES");
}

#[tokio::test]
async fn exit_foreign_profile_does_not_block_or_restore_other_copy() {
    let root=root();let profile=root.join("profiles/web");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("package.json"),"{}").unwrap();
    let original="- id: llm-pi-ai\n  config:\n    providers: {}\n";
    std::fs::write(profile.join("cordis.patch.yml"),original).unwrap();
    fn state(directory:PathBuf,profile:&std::path::Path)->AppState {
        let mut store=Store::open(directory.clone()).unwrap();
        let mut settings=store.config.settings.clone();settings.dsh_profile=profile.display().to_string();
        store.settings(settings).unwrap();
        store.save(serde_json::from_value(serde_json::json!({
            "name":"Synthetic exit fixture","model":"exit-fixture","protocol":"openai",
            "baseUrl":"http://127.0.0.1:9/v1","apiKey":"SYNTHETIC_EXIT_KEY",
            "headers":{},"enabled":true,"reasoning":"default","prompt":""
        })).unwrap()).unwrap();
        let store=Arc::new(Mutex::new(store));let gateway=gateway::Gateway::new(store.clone()).unwrap();
        AppState{updater:update::Service::default(),update_startup:Mutex::new(None),update_ready:std::sync::atomic::AtomicBool::new(true),dsh:Mutex::new(dsh::Status::default()),integration_control:tokio::sync::Mutex::new(()),close_request:Mutex::new(None),store,gateway,directory,smoke:false,autostart_key:String::new(),smoke_finished:std::sync::atomic::AtomicBool::new(false)}
    }
    let old=state(root.join("old-data"),&profile);
    let new=state(root.join("new-data"),&profile);
    let old_port=old.gateway.start_at(0).await.unwrap();
    old.bridge("sync").await.unwrap();
    let files=[profile.join("cordis.patch.yml"),root.join(".credentials.yaml"),profile.join(".dsh-patrick/journal.json")];
    let bytes:Vec<_>=files.iter().map(|f|std::fs::read(f).unwrap()).collect();
    for own_gateway_running in [false,true] {
        if own_gateway_running {new.gateway.start_at(0).await.unwrap();}
        let result=new.disconnect().await;
        assert!(result.is_ok(),"NEW_COPY_EXIT_BLOCKED: {result:?}");
        assert!(!new.gateway.running());
        assert!(old.gateway.running());
        assert!(tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST,old_port)).await.is_ok());
        for (file,before) in files.iter().zip(&bytes) {assert_eq!(std::fs::read(file).unwrap(),*before);}
    }
    // Explicit bridge actions must still reject a foreign owner; do not weaken sync guards.
    assert!(new.bridge("disconnect").await.unwrap_err().contains("另一份"));
    for (file,before) in files.iter().zip(&bytes) {assert_eq!(std::fs::read(file).unwrap(),*before);}
    // The actual owner's exit still restores its own profile and stops its gateway.
    old.disconnect().await.unwrap();
    assert!(!old.gateway.running());
    assert_eq!(std::fs::read_to_string(&files[0]).unwrap(),original);
    assert!(!files[1].exists() && !files[2].exists());
    println!("EXIT_MULTI_COPY_PASS new_stopped=YES old_listener=UNCHANGED profile_credentials_journal=BYTE_IDENTICAL explicit_foreign_restore=REJECTED own_exit=RESTORED local_only=YES");
    // Keep the bounded, generated fixture for evidence. No real DSH files are read or written.
}

async fn recovery_fixture() -> (AppState, PathBuf) {
    let root=root();let profile=root.join("profiles/web");std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("package.json"),"{}").unwrap();
    std::fs::write(profile.join("cordis.patch.yml"),"- id: llm-pi-ai\n  config: {providers: {}}\n").unwrap();
    let directory=root.join("patrick-data");let mut store=Store::open(directory.clone()).unwrap();
    let mut settings=store.config.settings.clone();settings.dsh_profile=profile.display().to_string();store.settings(settings).unwrap();
    store.save(serde_json::from_value(serde_json::json!({"name":"Synthetic recovery fixture","model":"mock","protocol":"openai","baseUrl":"http://127.0.0.1:9/v1","apiKey":"SYNTHETIC_KEY","headers":{},"enabled":true,"reasoning":"default","prompt":""})).unwrap()).unwrap();
    let store=Arc::new(Mutex::new(store));let gateway=gateway::Gateway::new(store.clone()).unwrap();
    let state=AppState{updater:update::Service::default(),update_startup:Mutex::new(None),update_ready:std::sync::atomic::AtomicBool::new(true),dsh:Mutex::new(dsh::Status::default()),integration_control:tokio::sync::Mutex::new(()),close_request:Mutex::new(None),store,gateway,directory,smoke:false,autostart_key:String::new(),smoke_finished:std::sync::atomic::AtomicBool::new(false)};
    state.gateway.start_at(0).await.unwrap();state.bridge("sync").await.unwrap();
    let file=profile.join("cordis.patch.yml");let text=std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("DSH Patrick · openai"));std::fs::write(&file,text.replace("DSH Patrick · openai","externally-edited-provider")).unwrap();
    (state,profile)
}

#[tokio::test]
async fn exit_recovery_conflict() {
    for exit in [false,true] {
        let (state,profile)=recovery_fixture().await;
        let files=[profile.join("cordis.patch.yml"),profile.parent().unwrap().parent().unwrap().join(".credentials.yaml"),profile.join(".dsh-patrick/journal.json")];
        let before:Vec<_>=files.iter().map(|f|std::fs::read(f).unwrap()).collect();
        let port=state.gateway.runtime().unwrap().port;
        let error=state.request_close(exit).await.unwrap_err();assert!(error.contains("外部修改"));assert!(state.gateway.running());
        let first=state.close_request.lock().unwrap().clone().unwrap();assert_eq!(first.exit,exit);
        assert!(state.resolve_close_request("wrong",true).await.is_err());
        assert!(!state.resolve_close_request(&first.id,false).await.unwrap());assert!(state.gateway.running());
        assert!(!lifecycle::recovery_pending(&state.directory,&profile.display().to_string()));
        assert!(files.iter().zip(&before).all(|(p,b)|std::fs::read(p).unwrap()==*b));
        state.request_close(exit).await.unwrap_err();
        let second=state.close_request.lock().unwrap().clone().unwrap();assert_ne!(first.id,second.id);
        assert!(state.resolve_close_request(&first.id,true).await.is_err());
        assert_eq!(state.resolve_close_request(&second.id,true).await.unwrap(),exit);
        assert!(!state.gateway.running());assert!(tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST,port)).await.is_err());
        assert!(files.iter().zip(&before).all(|(p,b)|std::fs::read(p).unwrap()==*b));
        assert!(state.close_request.lock().unwrap().is_none());
        let directory=state.directory.clone();drop(state);
        assert!(lifecycle::recovery_pending(&directory,&profile.display().to_string()));
        assert!(!lifecycle::recovery_pending(&directory,"another-profile"));
    }
    println!("EXIT_RECOVERY_PASS stop_and_exit=CONFIRMED cancel=NO_CHANGE stale=REJECTED listener=CLOSED profile_credentials_journal=EXACT restart_reminder=PERSISTED");
}

#[tokio::test]
async fn exit_recovery_busy_and_successful_retry() {
    use tokio::io::AsyncWriteExt;
    let (state,profile)=recovery_fixture().await;state.request_close(true).await.unwrap_err();
    let prompt=state.close_request.lock().unwrap().clone().unwrap();
    let port=state.gateway.runtime().unwrap().port;
    let mut upload=tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST,port)).await.unwrap();
    let token=state.store.lock().unwrap().config.gateway_token.clone();
    upload.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: 10000\r\n\r\n{{").as_bytes()).await.unwrap();
    let mut busy=false;
    for _ in 0..100 {if state.gateway.pause_if_idle().is_err(){busy=true;break;}tokio::time::sleep(std::time::Duration::from_millis(10)).await;}
    assert!(busy);
    assert!(state.resolve_close_request(&prompt.id,true).await.unwrap_err().contains("进行中"));assert!(state.gateway.running());
    assert!(!lifecycle::recovery_pending(&state.directory,&profile.display().to_string()));
    drop(upload);
    for _ in 0..100 {if state.gateway.pause_if_idle().is_ok(){busy=false;break;}tokio::time::sleep(std::time::Duration::from_millis(10)).await;}
    assert!(!busy);assert!(state.resolve_close_request(&prompt.id,true).await.unwrap());
    let journal:serde_json::Value=serde_json::from_slice(&std::fs::read(profile.join(".dsh-patrick/journal.json")).unwrap()).unwrap();
    std::fs::write(profile.join("cordis.patch.yml"),journal["after"]["profile"].as_str().unwrap()).unwrap();
    state.disconnect().await.unwrap();
    assert!(!profile.join(".dsh-patrick/journal.json").exists());
    assert!(!lifecycle::recovery_pending(&state.directory,&profile.display().to_string()));
    println!("EXIT_BUSY_PASS arrival_after_prompt=BLOCKED request_not_killed=YES retry_after_idle=STOPPED successful_restore=CLEARS_REMINDER");
}

#[tokio::test]
async fn exit_recovery_malformed_record_preserved() {
    let (state,profile)=recovery_fixture().await;let journal=profile.join(".dsh-patrick/journal.json");
    std::fs::write(&journal,"{SYNTHETIC_BROKEN_JOURNAL").unwrap();
    let before=std::fs::read(&journal).unwrap();
    assert!(state.request_close(true).await.is_err());
    {let status=state.dsh.lock().unwrap();assert!(!status.configured && !status.profile_valid && status.checked_at>0);assert_eq!(status.profile,profile.display().to_string());}
    let prompt=state.close_request.lock().unwrap().clone().unwrap();
    assert!(!prompt.message.contains("SYNTHETIC_BROKEN_JOURNAL"));
    assert!(state.resolve_close_request(&prompt.id,true).await.unwrap());
    assert_eq!(std::fs::read(&journal).unwrap(),before);assert!(!state.gateway.running());
    println!("EXIT_MALFORMED_PASS explicit_choice=EXITS raw_journal=NOT_ECHOED original_record=EXACT");
}
