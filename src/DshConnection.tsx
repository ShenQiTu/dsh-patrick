import {useEffect, useState} from "react";
import {api,native} from "./api";
import {Field,Icon} from "./ui";
import type {Snapshot} from "./types";

export function DshConnection({snapshot:s,busy,action}:{snapshot:Snapshot;busy:boolean;action:(fn:()=>Promise<unknown>,message:string)=>Promise<void>}) {
  const [profile,setProfile]=useState(s.settings.dshProfile);
  const [checking,setChecking]=useState(false),[expanded,setExpanded]=useState(false);
  useEffect(()=>setProfile(s.settings.dshProfile),[s.settings.dshProfile]);
  const dirty=profile.trim()!==s.settings.dshProfile;
  const current=!dirty&&s.dsh.profile===s.settings.dshProfile&&s.dsh.checkedAt>0;
  const valid=current&&s.dsh.profileValid;
  const pending=!dirty&&s.dshRecoveryPending;
  const synced=current&&s.dsh.configured&&!pending;
  const directoryState=!current?"unchecked":valid?"pass":"fail";
  const syncState=pending?"fail":!current||!valid?"unchecked":s.dsh.syncState==="error"?"fail":synced?"pass":"unchecked";
  useEffect(()=>setExpanded(s.dsh.checks.some(c=>c.state==="fail")),[s.dsh.checkedAt]);
  const directoryTitle=!current?"目录结构：尚未检查":valid?"目录结构：检查通过":s.dsh.checks.some(c=>c.state==="fail")?"目录结构：检查失败":"目录结构：检查未完成";
  const syncTitle=pending?"配置恢复：待处理":syncState==="fail"?"配置同步：核对失败":synced?"配置同步：已同步":!current||!valid||s.dsh.syncState==="unchecked"?"配置同步：尚未核对":"配置同步：尚未同步";
  const checkStatus=async()=>{setChecking(true);try{await action(api.checkDsh,"已完成目录与同步状态检查");}finally{setChecking(false);}};
  const icon=(state:string)=>state==="pass"?"check":state==="fail"?"close":"info";
  return <section className="panel connection-panel">
    <div className="panel-title"><h3><Icon name="plug"/> DSH 自动接入</h3><span className="pill">{pending?"恢复待处理":synced?"配置已同步":directoryState==="fail"||syncState==="fail"?"检查未通过":"待确认"}</span></div>
    <div className="connection-content">
    <p>启用模型由 Patrick 管理；DSH 的原有模型、默认选择和其他插件保留。</p>
    <div className="connection-profile">
    <Field label="DSH profile 目录"><input aria-label="DSH profile 目录" value={profile} disabled={busy||s.gatewayRunning||s.dshConnected} onChange={e=>setProfile(e.target.value)} placeholder="DSH_HOME\profiles\web" spellCheck={false}/></Field>
    <small className="connection-path-hint" data-dsh-path-hint>{dirty?"路径已修改，尚未保存；请先保存，再检查新路径。":"填写 profile 配置目录，而非 EXE 安装目录；保存路径不代表检查通过。"}</small>
    <div className="connection-buttons">
      <button disabled={!native||busy||s.gatewayRunning||s.dshConnected||!dirty} onClick={()=>void action(()=>api.settings({...s.settings,dshProfile:profile.trim()}),"DSH profile 已保存，请检查状态")}>保存路径</button>
      <button data-dsh-check-button disabled={!native||busy||s.testMode||dirty} onClick={()=>void checkStatus()}>{checking?"检查中…":"检查状态"}</button>
      <button disabled={!native||busy||!s.gatewayRunning||s.testMode} onClick={()=>void action(api.syncDsh,"启用模型已重新同步")}>重新同步</button>
      <button disabled={!native||busy||s.testMode||s.runtime.activeRequests>0} onClick={()=>void action(api.stop,"已断开并恢复 DSH 配置")}>断开并恢复</button>
    </div>
    </div>
    <div className="address-row"><span>本机 API</span><code>http://127.0.0.1:{s.runtime.port||s.settings.port}/v1</code></div>
    <div className="connection-options connection-diagnostics" aria-live="polite">
      <div data-dsh-stage="directory" data-state={directoryState}><Icon name={icon(directoryState)}/><span><b>{directoryTitle}</b><small>{!current?dirty?"未保存的路径尚未检查，不沿用旧路径的检查结果。":"点击检查状态，仅检查指定目录，不自动扫描，也不修改 DSH 配置。":valid?"只确认配置结构符合接入要求；文件写入不代表客户端已加载。":s.dsh.message}</small></span></div>
      <div data-dsh-stage="sync" data-state={syncState}><Icon name={icon(syncState)}/><span><b>{syncTitle}</b><small>{pending?"上次停止保留了 DSH 配置、凭据和恢复记录；请核对冲突后重新同步或断开并恢复。网关是否运行请看左下角。":!current||!valid?"请先通过目录结构检查，再核对接入配置。":syncState==="fail"?s.dsh.message:synced?`已同步 ${s.dsh.modelCount} 个模型 · Zen 会话规则 ${s.dsh.zenCount} 个；仍需在 DSH 选择 Patrick 模型。`:s.dsh.message}</small></span></div>
      <div data-dsh-stage="client" data-state="unchecked"><Icon name="link"/><span><b>{s.runtime.lastDshRequest?"客户端请求：已收到入口请求":!s.gatewayRunning?"客户端请求：网关未运行":"客户端请求：尚未收到入口请求"}</b><small>{s.runtime.lastDshRequest?`最近记录：${new Date(s.runtime.lastDshRequest).toLocaleString()}。入口标记不区分具体 profile，请求是否成功请查看调用记录。`:"在正在使用的 DSH 中选择 Patrick 下的模型发起请求，再核对时间和调用记录。"}</small></span></div>
    </div>
    {current&&s.dsh.checks.length>0&&<details className="connection-check-details" open={expanded} onToggle={e=>setExpanded(e.currentTarget.open)}>
      <summary>查看检查明细 · {s.dsh.checks.filter(c=>c.state==="pass").length}/{s.dsh.checks.length} 项通过<span>{new Date(s.dsh.checkedAt).toLocaleTimeString()}</span></summary>
      <ul>{s.dsh.checks.map(c=><li key={c.id} data-dsh-item={c.id} data-state={c.state}><strong>{c.label}<em>{c.state==="pass"?"通过":c.state==="fail"?"失败":"未检查"}</em></strong><small>{c.detail}</small></li>)}</ul>
    </details>}
    <div className="connection-recovery">
    <p className="muted">真实模型测试由你发起。关闭网关或从托盘退出会先撤销本软件写入的模型与本机令牌；异常退出后可在此恢复。检测到外部修改冲突时可选择保留配置，仅停止网关或退出；下次启动继续提示恢复待处理。</p>
    {s.dsh.backupDirectory&&<div className="address-row"><span>接入备份</span><code style={{overflowWrap:"anywhere",whiteSpace:"normal"}}>{s.dsh.backupDirectory}</code></div>}
    </div>
    </div>
  </section>;
}
