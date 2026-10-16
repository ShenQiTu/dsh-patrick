import { api } from "./api";
import { invoke } from "@tauri-apps/api/core";
import type { UpdateInfo } from "./types";

export async function runUpdateRegression() {
  const checks:string[]=[];
  const check=(v:unknown,n:string)=>{if(!v)throw Error("UPDATE_UI_FAIL "+n);checks.push("UPDATE_UI_"+n);};
  const q=(s:string)=>document.querySelector<HTMLElement>(s)!;
  const sleep=()=>new Promise(r=>setTimeout(r,100));
  const click=async(s:string)=>{q(s).click();await sleep();};
  const original={snapshot:api.snapshot,check:api.checkUpdate,install:api.installUpdate,cancel:api.cancelUpdate};
  let update:UpdateInfo={state:"available",version:"0.1.21",available:true,busy:false,downloaded:0,total:0,message:"发现新版本 v0.1.21",lastCheckedAt:1};
  let installs=0,cancels=0;
  const refresh=async()=>{await click('[aria-label="刷新"]');await sleep();};
  try {
    api.snapshot=async()=>({...await original.snapshot(),update:{...update}});
    api.checkUpdate=async()=>{throw Error("SYNTHETIC_UPDATE_OFFLINE");};
    api.installUpdate=async(v:string)=>{check(v==="0.1.21","selected_version_sent");installs++;update={...update,busy:true,state:"downloading",downloaded:25,total:100,message:"正在下载更新…"};};
    api.cancelUpdate=async()=>{cancels++;update={...update,busy:false,state:"error",message:"更新已取消，原版本保持不变"};};
    await click('[data-nav="settings"]');await refresh();
    check(q('[data-update-privacy]').textContent?.includes('访问 GitHub；不上传模型配置或 Key'),"privacy_discloses_update_check");
    check(!!q('[data-update-dot]'),"sidebar_red_dot");
    check(getComputedStyle(q('[data-update-dot]')).width==="7px","reference_dot_size");
    check(q('[data-install-update]').textContent?.includes('v0.1.21'),"available_version_action");
    await click('[data-check-update]');check(q('[data-update-result]').textContent?.includes('SYNTHETIC_UPDATE_OFFLINE'),"manual_check_failure_visible");
    check(!(q('[data-check-update]') as HTMLButtonElement).disabled,"check_failure_ends_loading");
    await click('[data-install-update]');await refresh();
    check(installs===1&&!!q('[data-update-progress]'),"download_started_progress_visible");
    check((q('progress[aria-label="更新下载进度"]') as HTMLProgressElement).value===25,"download_progress_value");
    await click('[data-install-update]');check(installs===1,"duplicate_install_disabled");
    update={...update,state:"waiting",downloaded:100,message:"等待请求结束"};await refresh();
    check(!!q('[data-cancel-update]')&&q('[data-update-progress]').textContent?.includes('等待空闲'),"waiting_can_cancel");
    await click('[data-cancel-update]');await refresh();check(cancels===1&&!q('[data-update-progress]'),"cancel_ends_loading");
    update={...update,busy:true,state:"installing",message:"安装后自动重新打开"};await refresh();
    check(!q('[data-cancel-update]')&&(q('[data-check-update]') as HTMLButtonElement).disabled,"replacement_not_interruptible");
    update={...update,busy:false,state:"available"};await refresh();
    for(const theme of ['light','dark'] as const) {
      const s=await original.snapshot();await api.settings({...s.settings,theme});await refresh();
      for(const [width,height] of [[960,640],[800,520]]) {
        await invoke('smoke_resize',{width,height});await sleep();
        const panel=q('[data-update-settings]');panel.scrollIntoView({block:'center'});await sleep();
        check(panel.scrollWidth<=panel.clientWidth+1,`no_horizontal_overflow_${width}_${theme}`);
        const a=q('[data-install-update]').getBoundingClientRect(),b=q('[data-check-update]').getBoundingClientRect();
        check(a.right<=b.left+1||a.bottom<=b.top+1,`buttons_no_overlap_${width}_${theme}`);
      }
    }
    update={...update,available:false,version:'0.1.20',state:'current',message:'当前已是最新正式版'};await refresh();
    check(!q('[data-update-dot]')&&!q('[data-install-update]'),"no_update_no_badge_or_install");
  } finally {
    api.snapshot=original.snapshot;api.checkUpdate=original.check;api.installUpdate=original.install;api.cancelUpdate=original.cancel;
    const s=await original.snapshot();await api.settings({...s.settings,theme:'light'});
    await invoke('smoke_resize',{width:960,height:640});await refresh();
  }
  return checks;
}
