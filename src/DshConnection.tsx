import {useEffect, useState} from "react";
import {api,native} from "./api";
import {Field,Icon} from "./ui";
import type {Snapshot} from "./types";

export function DshConnection({snapshot:s,busy,action}:{snapshot:Snapshot;busy:boolean;action:(fn:()=>Promise<unknown>,message:string)=>Promise<void>}) {
  const [profile,setProfile]=useState(s.settings.dshProfile);
  useEffect(()=>setProfile(s.settings.dshProfile),[s.settings.dshProfile]);
  return <section className="panel connection-panel">
    <div className="panel-title"><h3><Icon name="plug"/> DSH 自动接入</h3><span className="pill">{s.dshConnected?"配置已同步":"未同步"}</span></div>
    <div className="connection-content">
    <p>启用模型由 Patrick 管理；DSH 的原有模型、默认选择和其他插件保留。</p>
    <div className="connection-profile">
    <Field label="DSH profile 目录"><input aria-label="DSH profile 目录" value={profile} disabled={busy||s.gatewayRunning||s.dshConnected} onChange={e=>setProfile(e.target.value)} placeholder="DSH_HOME\profiles\web" spellCheck={false}/></Field>
    <div className="connection-buttons">
      <button disabled={!native||busy||s.gatewayRunning||s.dshConnected||profile===s.settings.dshProfile} onClick={()=>void action(()=>api.settings({...s.settings,dshProfile:profile.trim()}),"DSH profile 已保存")}>保存路径</button>
      <button disabled={!native||busy||s.testMode} onClick={()=>void action(api.checkDsh,"已重新核对 DSH 配置")}>检查状态</button>
      <button disabled={!native||busy||!s.gatewayRunning||s.testMode} onClick={()=>void action(api.syncDsh,"启用模型已重新同步")}>重新同步</button>
      <button disabled={!native||busy||s.testMode||s.runtime.activeRequests>0} onClick={()=>void action(api.stop,"已断开并恢复 DSH 配置")}>断开并恢复</button>
    </div>
    </div>
    <div className="address-row"><span>本机 API</span><code>http://127.0.0.1:{s.runtime.port||s.settings.port}/v1</code></div>
    <div className="connection-options">
      <div><Icon name="check"/><span><b>{s.dsh.message||"尚未同步"}</b><small>已同步 {s.dsh.modelCount} 个模型 · Zen 会话规则 {s.dsh.zenCount} 个</small></span></div>
      <div><Icon name="link"/><span><b>{!s.gatewayRunning?"网关未运行":s.runtime.lastDshRequest?"已收到 DSH 入口请求":"等待 DSH 入口请求"}</b><small>{s.runtime.lastDshRequest?new Date(s.runtime.lastDshRequest).toLocaleString():"文件写入不代表客户端已加载；启动 DSH 后选择 Patrick 下的模型。"}</small></span></div>
    </div>
    <div className="connection-recovery">
    <p className="muted">真实模型测试由你发起。关闭网关或从托盘退出会先撤销本软件写入的模型与本机令牌；异常退出后可在此恢复。检测到外部修改冲突时保留现场。</p>
    {s.dsh.backupDirectory&&<div className="address-row"><span>接入备份</span><code style={{overflowWrap:"anywhere",whiteSpace:"normal"}}>{s.dsh.backupDirectory}</code></div>}
    </div>
    </div>
  </section>;
}
