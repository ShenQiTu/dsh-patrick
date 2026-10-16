use super::*;
use ring::signature::{Ed25519KeyPair,KeyPair};
fn key()->Ed25519KeyPair{Ed25519KeyPair::from_seed_unchecked(&[19;32]).unwrap()}
fn fixture()->Manifest{Manifest{schema:1,version:"0.1.21".into(),platform:"windows-x86_64".into(),data_schema:1,files:FILES.iter().map(|name|{
    let asset=match *name {"DSH Patrick.exe"=>"DSH-Patrick-v0.1.21-windows-x64.exe","runtime/node.exe"=>"patrick-node-windows-x64.exe",_=>"THIRD_PARTY_NOTICES.md"};
    Artifact{path:(*name).into(),url:format!("{DOWNLOAD_PREFIX}v0.1.21/{asset}"),size:512,sha256:"a".repeat(64)}
}).collect()}}
fn signed(m:&Manifest)->Vec<u8>{let body=serde_json::to_vec(m).unwrap();serde_json::to_vec(&serde_json::json!({"payload":STANDARD.encode(&body),"signature":STANDARD.encode(key().sign(&body).as_ref())})).unwrap()}
fn dir()->PathBuf{let p=std::env::temp_dir().join(format!("patrick-update-test-{}",crate::config::new_id()));fs::create_dir(&p).unwrap();p}
#[test]
fn signed_manifest_and_component_boundary(){
    let m=fixture();assert_eq!(verify_with_key(&signed(&m),key().public_key().as_ref()).unwrap().version,"0.1.21");
    assert!(verify(&signed(&m)).is_err(),"test key must not match production key");
    let mut changed=signed(&m);let pos=changed.iter().position(|b|*b==b'e').unwrap();changed[pos]=b'A';assert!(verify_with_key(&changed,key().public_key().as_ref()).is_err());
    for case in 0..12 {
        let mut m=fixture();match case {
            0=>m.files[0].path="../app-data/config.dpapi".into(),1=>m.files[0].path="C:\\evil.exe".into(),
            2=>m.files[0].path="DSH Patrick.exe:stream".into(),3=>m.files[1]=m.files[0].clone(),
            4=>{m.files.pop();},5=>m.files[0].size=256*1024*1024+1,6=>m.files[0].url="https://evil.invalid/app.exe".into(),
            7=>m.files[0].url=m.files[0].url.replace("https:","http:"),8=>m.data_schema=2,9=>m.platform="linux-x64".into(),
            10=>m.version="0.1.21-beta.1".into(),11=>m.files[2].sha256="G".repeat(64),_=>unreachable!()
        };assert!(verify_with_key(&signed(&m),key().public_key().as_ref()).is_err(),"case {case}");
    }
    assert!(version("0.1.20").unwrap()>version("0.1.9").unwrap());assert!(version("0.1.20;start").is_err());
    println!("SIGNED_UPDATE_PASS signature=VERIFIED tamper=REJECTED foreign_key=REJECTED paths=WHITELIST versions=SEMVER credentials=NONE");
}
#[test]
fn file_integrity_and_x64_pe(){
    let d=dir();let p=d.join("file.exe");let mut bytes=vec![0u8;512];bytes[..2].copy_from_slice(b"MZ");bytes[60..64].copy_from_slice(&64u32.to_le_bytes());bytes[64..70].copy_from_slice(b"PE\0\0\x64\x86");bytes[88..90].copy_from_slice(&[0x0b,0x02]);fs::write(&p,&bytes).unwrap();
    let mut f=fixture().files[0].clone();f.sha256=digest_file(&p).unwrap();check_file(&p,&f).unwrap();
    bytes[3]=1;fs::write(&p,&bytes).unwrap();assert!(check_file(&p,&f).is_err());f.sha256=digest_file(&p).unwrap();check_file(&p,&f).unwrap();
    bytes[68]=0x4c;fs::write(&p,&bytes).unwrap();f.sha256=digest_file(&p).unwrap();assert!(check_file(&p,&f).is_err());
    println!("UPDATE_FILE_PASS sha256=REQUIRED size=REQUIRED windows_x64=REQUIRED");
}
#[test]
fn state_double_click_cancel_and_retry(){
    let s=Service::default();s.begin().unwrap();assert!(s.begin().is_err());assert!(s.view().busy);
    s.cancel().unwrap();assert!(s.cancelled().is_err());s.finish_error("cancelled");assert!(!s.view().busy);
    s.begin().unwrap();assert!(s.cancelled().is_ok());s.phase("waiting","fixture");s.cancel().unwrap();assert!(s.cancelled().is_err());
    s.finish_error("fixture");s.begin().unwrap();s.phase("installing","fixture");assert!(s.cancel().is_err());s.finish_error("fixture failure");assert!(!s.view().busy);
    println!("UPDATE_STATE_PASS duplicate=BLOCKED cancel=SUPPORTED failure_loading=ENDED");
}
#[test]
fn atomic_replacement_and_backup_restore(){
    let d=dir();let target=d.join("app.exe");let new=d.join("new.exe");let backup=d.join("backup.exe");
    fs::write(&target,b"OLD_APPLICATION").unwrap();fs::write(&new,b"NEW_APPLICATION").unwrap();fs::copy(&target,&backup).unwrap();
    replacement::atomic_copy(&new,&target,"test1").unwrap();assert_eq!(fs::read(&target).unwrap(),b"NEW_APPLICATION");
    replacement::atomic_copy(&backup,&target,"test2").unwrap();assert_eq!(fs::read(&target).unwrap(),b"OLD_APPLICATION");
    assert_eq!(fs::read(&new).unwrap(),b"NEW_APPLICATION");println!("UPDATE_ATOMIC_PASS replaced=YES backup_restored=YES staged_copy=PRESERVED");
}
#[test]
fn locked_destination_preserves_old_component(){
    use std::os::windows::fs::OpenOptionsExt;
    let d=dir();let target=d.join("app.exe");let new=d.join("new.exe");
    fs::write(&target,b"OLD_APPLICATION").unwrap();fs::write(&new,b"NEW_APPLICATION").unwrap();
    let lock=fs::OpenOptions::new().read(true).share_mode(1).open(&target).unwrap();
    assert!(replacement::atomic_copy(&new,&target,"locked").is_err());
    assert_eq!(fs::read(&target).unwrap(),b"OLD_APPLICATION");drop(lock);
    replacement::atomic_copy(&new,&target,"unlocked").unwrap();
    assert_eq!(fs::read(&target).unwrap(),b"NEW_APPLICATION");
    println!("UPDATE_LOCKED_PASS occupied=NOT_REPLACED original=EXACT retry_after_unlock=PASS");
}
#[tokio::test]
async fn update_admission_waits_and_nested_pause_cannot_resume(){
    let d=dir();let store=std::sync::Arc::new(Mutex::new(crate::config::Store::open(d).unwrap()));let g=crate::gateway::Gateway::new(store).unwrap();
    let pause=g.pause_if_idle().unwrap();assert!(g.pause_if_idle().is_err());drop(pause);assert!(g.pause_if_idle().is_ok());
    println!("UPDATE_ADMISSION_PASS nested_pause=REJECTED owner_release=REQUIRED");
}

#[tokio::test]
async fn waiting_for_active_and_queued_requests_does_not_cancel_them(){
    use axum::{Router,routing::get};
    let release=std::sync::Arc::new(tokio::sync::Semaphore::new(0));let signal=release.clone();
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let port=listener.local_addr().unwrap().port();
    let server=tokio::spawn(async move{axum::serve(listener,Router::new().route("/v1/models",get(move||{let signal=signal.clone();async move{
        signal.acquire().await.unwrap().forget();axum::Json(serde_json::json!({"data":[{"id":"synthetic"}]}))
    }}))).await.unwrap();});
    let store=std::sync::Arc::new(Mutex::new(crate::config::Store::open(dir()).unwrap()));store.lock().unwrap().config.settings.upstream_concurrency=1;
    let g=crate::gateway::Gateway::new(store).unwrap();
    let input:crate::config::ModelInput=serde_json::from_value(serde_json::json!({"name":"synthetic","model":"synthetic","protocol":"openai","baseUrl":format!("http://127.0.0.1:{port}/v1"),"apiKey":"SYNTHETIC","headers":{},"enabled":true,"reasoning":"default","prompt":""})).unwrap();
    let a={let g=g.clone();let i=input.clone();tokio::spawn(async move{g.discover(i,None).await})};
    let b={let g=g.clone();let i=input.clone();tokio::spawn(async move{g.discover(i,None).await})};
    tokio::time::timeout(Duration::from_secs(5),async {while g.runtime().unwrap().active_requests!=2{tokio::time::sleep(Duration::from_millis(10)).await;}}).await.unwrap();
    let service=Service::default();service.begin().unwrap();service.phase("waiting","fixture");
    assert!(g.pause_if_idle().is_err());service.cancel().unwrap();service.finish_error("cancelled");
    assert!(!a.is_finished()&&!b.is_finished());release.add_permits(2);
    assert_eq!(a.await.unwrap().unwrap(),vec!["synthetic"]);assert_eq!(b.await.unwrap().unwrap(),vec!["synthetic"]);
    let pause=g.pause_if_idle().unwrap();assert!(g.discover(input,None).await.is_err());drop(pause);server.abort();
    println!("UPDATE_WAIT_PASS active_and_queued=WAIT cancel_update=REQUESTS_CONTINUE both_completed=YES installing_new_request=BLOCKED");
}

#[tokio::test]
async fn local_signed_release_http_and_failure_paths(){
    use axum::{Router,routing::get,http::{HeaderMap,StatusCode},response::IntoResponse};
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let port=listener.local_addr().unwrap().port();
    let signed=signed(&fixture());let release=signed.clone();
    let app=Router::new().route("/manifest",get(move|h:HeaderMap|{let b=release.clone();async move {assert!(!h.contains_key("authorization")&&!h.contains_key("cookie"));b}}))
        .route("/unsigned",get(||async{axum::Json(serde_json::json!({"tag_name":"v0.1.21","draft":false,"prerelease":false}))}))
        .route("/missing",get(||async{(StatusCode::NOT_FOUND,"")}))
        .route("/rate",get(||async{(StatusCode::TOO_MANY_REQUESTS,"")}))
        .route("/redirect",get(||async{(StatusCode::FOUND,[("location","http://127.0.0.1:9/not-allowed")],"")}))
        .route("/large",get(||async{vec![b'x';MAX_MANIFEST+1]}))
        .route("/asset",get(|h:HeaderMap|async move{assert!(!h.contains_key("authorization")&&!h.contains_key("cookie"));"SYNTHETIC_NOTICE"}))
        .route("/slow",get(||async{
            let body=axum::body::Body::from_stream(futures_util::stream::unfold(0,|n|async move {tokio::time::sleep(Duration::from_millis(100)).await;Some((Ok::<_,std::io::Error>(vec![b'a']),n+1))}));
            (StatusCode::OK,body).into_response()
        }));
    let task=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});let base=format!("http://127.0.0.1:{port}");let c=client().unwrap();
    let (_,m)=fetch_manifest_key(&c,&format!("{base}/manifest"),key().public_key().as_ref()).await.unwrap();assert_eq!(m.version,"0.1.21");
    for route in ["unsigned","missing","rate","redirect","large"] {assert!(fetch_manifest_key(&c,&format!("{base}/{route}"),key().public_key().as_ref()).await.is_err(),"{route}");}
    let d=dir();let source=d.join("source");fs::write(&source,"SYNTHETIC_NOTICE").unwrap();
    let a=Artifact{path:FILES[2].into(),url:format!("{base}/asset"),size:16,sha256:digest_file(&source).unwrap()};
    let a=Artifact{size:fs::metadata(&source).unwrap().len(),..a};let service=Service::default();service.begin().unwrap();
    service.download(&c,&a,&d.join("success")).await.unwrap();assert_eq!(fs::read(d.join("success")).unwrap(),b"SYNTHETIC_NOTICE");
    let bad=Artifact{sha256:"b".repeat(64),..a.clone()};assert!(service.download(&c,&bad,&d.join("hash-fail")).await.unwrap_err().contains("SHA256"));
    let bad=Artifact{size:1,..a.clone()};assert!(service.download(&c,&bad,&d.join("size-fail")).await.is_err());
    let slow=Artifact{url:format!("{base}/slow"),size:10000,..a.clone()};
    let cancel=async{tokio::time::sleep(Duration::from_millis(180)).await;service.cancel().unwrap();};
    let cancel_path=d.join("cancel");
    let (download,())=tokio::join!(service.download(&c,&slow,&cancel_path),cancel);assert!(download.unwrap_err().contains("已取消"));
    service.finish_error("cancelled");service.begin().unwrap();
    let timeout=reqwest::Client::builder().no_proxy().timeout(Duration::from_millis(150)).build().unwrap();
    assert!(service.download(&timeout,&slow,&d.join("timeout")).await.unwrap_err().contains("超时"));
    task.abort();println!("UPDATE_HTTP_PASS signed=PASS unsigned=BLOCKED http404=ERROR rate_limit=ERROR redirects=RESTRICTED oversize=BLOCKED hash=VERIFIED cancel=PASS timeout=ERROR credentials=NONE loopback_only=YES");
}
