import {useRef, useState, type ReactNode} from "react";
import {api} from "./api";
import {Select} from "./Select";
import {Switch} from "./ui";
import type {Settings} from "./types";

type NumberKey = { [K in keyof Settings]: Settings[K] extends number ? K : never }[keyof Settings];
type NumericField = {key:NumberKey; label:string; min:number; max:number; unit?:string};
type Save = (patch:Partial<Settings>)=>Promise<void>;
export function NumericSettingsCard({id,title,fields,settings,onSave,children,footer}: {
  id:string;title:string;fields:NumericField[];settings:Settings;onSave:Save;
  children?:ReactNode;footer?:ReactNode;
}) {
  const [editing,setEditing]=useState(false),[saving,setSaving]=useState(false),[error,setError]=useState("");
  const [draft,setDraft]=useState<Record<string,string>>({});
  return <section className="panel settings-panel settings-extra" data-settings-group={id}>
    <form id={`settings-${id}`} onSubmit={async e=>{
      e.preventDefault();setError("");const patch:Partial<Settings>={};
      for(const f of fields) {
        const n=Number(draft[f.key]);
        if(!draft[f.key]?.trim()||!Number.isInteger(n)||n<f.min||n>f.max) {setError(`${f.label}需为 ${f.min}–${f.max} 的整数`);return;}
        patch[f.key]=n;
      }
      setSaving(true);
      try {await onSave(patch);setEditing(false);}catch(e){setError(String(e));}finally{setSaving(false);}
    }}>
      <div className="panel-title"><h3>{title}</h3><div className="settings-card-actions">
        {editing?<><button type="button" data-cancel-settings={id} className="text-button" disabled={saving} onClick={()=>{setEditing(false);setError("");}}>取消</button>
          <button type="submit" data-save-settings={id} className="text-button" disabled={saving}>{saving?"保存中…":"保存"}</button></>:
          <button type="button" className="text-button" data-edit-settings={id} onClick={()=>{setDraft(Object.fromEntries(fields.map(f=>[f.key,String(settings[f.key])])));setError("");setEditing(true);}}>编辑</button>}
      </div></div>
      <div className="settings-values">{fields.map(f=><div className="settings-value-row" key={f.key}>
        <label htmlFor={`setting-${f.key}`}>{f.label}</label>
        {editing?<input id={`setting-${f.key}`} name={f.key} type="number" required min={f.min} max={f.max} step={1} disabled={saving}
          value={draft[f.key]??""} onChange={e=>setDraft({...draft,[f.key]:e.target.value})}/>:
          <span data-setting-value={f.key}>{settings[f.key]}{f.unit?` ${f.unit}`:""}</span>}
      </div>)}</div>
      {children}
      {error&&<p className="settings-message" role="alert">{error}</p>}
    </form>
    {footer}
  </section>;
}
// Per instance callback, not global mutable settings. A direct component keeps failures local.
function QuickSetting({onSave,children}:{onSave:Save;children:(save:Save,busy:boolean)=>ReactNode}) {
  const [busy,setBusy]=useState(false),[error,setError]=useState("");
  return <>{children(async patch=>{setBusy(true);setError("");try{await onSave(patch);}catch(e){setError(String(e));}finally{setBusy(false);}},busy)}
    {error&&<p role="alert" className="settings-message">{error}</p>}</>;
}
export function SettingsCards({settings,onSave}:{settings:Settings;onSave:(s:Settings)=>Promise<void>}) {
  const pending=useRef<Promise<void>>(Promise.resolve());
  // Serialize independent writes and merge into the latest persisted values, never another card's draft.
  const save:Save=patch=>{
    const next=pending.current.then(async()=>onSave({...((await api.snapshot()).settings),...patch}));
    pending.current=next.catch(()=>{});return next;
  };
  return <div className="settings-groups">
    <NumericSettingsCard id="gateway" title="网关设置" settings={settings} onSave={save} fields={[
      {key:"port",label:"监听端口",min:1024,max:65535},
      {key:"timeoutSeconds",label:"排队 / 非流式超时",min:5,max:700,unit:"秒"},
      {key:"maxBodyMb",label:"请求体上限",min:1,max:128,unit:"MiB"},
    ]}><p className="settings-message">仅监听本机回环地址；修改端口前请先关闭网关。排队 / 非流式超时不限制持续输出的累计时长。</p></NumericSettingsCard>
    <NumericSettingsCard id="stream-timeout" title="流式超时 · 中转站优先" settings={settings} onSave={save} fields={[
      {key:"streamIdleSeconds",label:"无数据兜底基准",min:5,max:7200,unit:"秒"},
      {key:"streamGraceSeconds",label:"额外宽限",min:1,max:60,unit:"秒"},
    ]}><p className="settings-message" data-stream-wait>当前兜底：{settings.streamIdleSeconds+settings.streamGraceSeconds} 秒（已保存基准 + 宽限）。等待响应头、连续无响应数据分别计时；收到数据或心跳后重新计时，不限制持续输出的累计时长。</p>
      <p className="settings-message">中转站负责换号重试；本网关不检测换号、不自动重发。新设置用于新请求，进行中的请求沿用开始时的设置。</p></NumericSettingsCard>
    <NumericSettingsCard id="dsh-timeout" title="DSH 模型输出等待" settings={settings} onSave={save} fields={[
      {key:"dshStreamIdleSeconds",label:"模型输出等待时间",min:1,max:2147483,unit:"秒"},
    ]}><p className="settings-message">默认 300 秒。同步到 Patrick 管理的 DSH 模型渠道，不改变原有直连渠道；已接入时保存会重新同步，未接入时下次接入生效。DSH 需加载更新后的配置。</p></NumericSettingsCard>
    <NumericSettingsCard id="concurrency" title="并发与排队" settings={settings} onSave={save} fields={[
      {key:"bodyConcurrency",label:"请求体处理并发数",min:1,max:16},
      {key:"upstreamConcurrency",label:"模型转发并发数",min:1,max:32},
    ]}><p className="settings-message">满额后排队；每阶段最多等待 128 个请求，排队总预算沿用“排队 / 非流式超时”。并发数在网关空闲时保存生效。</p></NumericSettingsCard>
    <section className="panel settings-panel settings-extra" data-settings-group="appearance"><div className="panel-title"><h3>外观设置</h3></div>
      <QuickSetting onSave={save}>{(save,busy)=><div className="settings-row"><span>界面主题</span><Select label="界面主题" value={settings.theme} disabled={busy}
        onChange={v=>void save({theme:v as Settings["theme"]})} options={[{value:"light",label:"浅色"},{value:"dark",label:"深色"},{value:"system",label:"跟随系统"}]}/></div>}</QuickSetting>
    </section>
    <NumericSettingsCard id="calls" title="调用记录" settings={settings} onSave={save} fields={[
      {key:"logLimit",label:"调用记录上限",min:10,max:1000,unit:"条"},
    ]} footer={<QuickSetting onSave={save}>{(save,busy)=><div className="settings-row settings-detail-toggle"><div><b>详细记录</b><small>开启后保存之后调用的脱敏请求与响应，可能包含对话正文；关闭仅保留调用信息。请求与响应分别最多 128 KiB，超出部分明确标注。</small></div>
      <Switch label="详细记录" checked={settings.detailedLogging} disabled={busy} onChange={()=>void save({detailedLogging:!settings.detailedLogging})}/></div>}</QuickSetting>}>
      <p className="settings-message">保存最近 10–1000 条调用，超出上限移除旧调用及其详情；首页统计独立保留。调用信息本机加密保存，重启后保留。</p>
    </NumericSettingsCard>
  </div>;
}
