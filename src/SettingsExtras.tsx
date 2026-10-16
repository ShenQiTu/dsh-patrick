import { useEffect, useState } from "react";
import { api, native } from "./api";
import { Modal, Switch } from "./ui";
import type { AutostartStatus, Snapshot, StorageInfo, TokenPricing } from "./types";

const prices: [keyof TokenPricing, string][] = [["input", "输入价格"], ["output", "输出价格"], ["cacheRead", "缓存读取价格"], ["cacheWrite", "缓存写入价格"]];
const blank: TokenPricing = { input: null, output: null, cacheRead: null, cacheWrite: null };
const toDraft = (p: TokenPricing) => Object.fromEntries(prices.map(([k]) => [k, p[k] === null ? "" : String(p[k])])) as Record<keyof TokenPricing, string>;

export function PricingSettings({ pricing, onRefresh }: { pricing: TokenPricing; onRefresh: () => Promise<void> }) {
  const [editing, setEditing] = useState(false), [saving, setSaving] = useState(false);
  const [draft, setDraft] = useState(toDraft(pricing)), [error, setError] = useState("");
  return <section className="panel settings-extra pricing-settings">
    <div className="panel-title"><h3>Token 定价</h3><div className="settings-card-actions">{editing ? <>
      <button type="button" className="text-button" data-cancel-pricing disabled={saving} onClick={() => { setEditing(false); setError(""); }}>取消</button>
      <button type="submit" form="pricing-settings-form" className="text-button" data-save-pricing disabled={saving}>{saving ? "保存中…" : "保存"}</button>
    </> : <button className="text-button" data-edit-pricing onClick={() => { setDraft(toDraft(pricing)); setError(""); setEditing(true); }}>编辑</button>}</div></div>
    <p className="settings-description">用于首页价值估算的统一单价，单位：美元 / 百万 Token。留空表示未配置，0 表示免费；估算并非服务商账单。</p>
    <form id="pricing-settings-form" onSubmit={async e => {
      e.preventDefault(); setError("");
      const next = { ...blank };
      for (const [k] of prices) {
        const raw = draft[k].trim(); next[k] = raw === "" ? null : Number(raw);
        if (next[k] !== null && (!Number.isFinite(next[k]) || next[k]! < 0 || next[k]! > 1000000)) { setError("单价需为 0–1000000 的数值，或留空"); return; }
      }
      setSaving(true);
      try { await api.savePricing(next); await onRefresh(); setEditing(false); }
      catch (e) { setError(String(e)); }
      finally { setSaving(false); }
    }}>
      {prices.map(([key, label]) => <div className="settings-row price-row" key={key}>
        <label htmlFor={`price-${key}`}>{label}（$/1M）</label>
        {editing ? <input id={`price-${key}`} aria-label={label} name={`price-${key}`} type="number" min="0" max="1000000" step="any" placeholder="未配置" value={draft[key]} disabled={saving} onChange={e => setDraft({ ...draft, [key]: e.target.value })} /> : <span className="muted" data-price={key}>{pricing[key] === null ? "未配置" : `$${pricing[key]}`}</span>}
      </div>)}
      {error && <p role="alert" className="settings-message">{error}</p>}
      {editing && <footer className="settings-actions"><button type="button" disabled={saving} onClick={() => setDraft(toDraft(blank))}>清空单价</button></footer>}
    </form>
  </section>;
}

export function ApplicationSettings({ version }: { version: string }) {
  const [startup, setStartup] = useState<AutostartStatus | null>(null), [busy, setBusy] = useState(false), [checking, setChecking] = useState(false);
  const [startupError, setStartupError] = useState(""), [update, setUpdate] = useState("");
  useEffect(() => {
    let active = true;
    if (native) api.autostartStatus().then(s => { if (active) setStartup(s); }).catch(e => { if (active) setStartupError(String(e)); });
    return () => { active = false; };
  }, []);
  return <section className="panel settings-extra application-settings">
    <div className="panel-title"><h3>应用设置</h3></div>
    <div className="settings-row"><div><b>开机启动</b><small>{!native ? "浏览器预览不操作系统启动项。" : startup?.enabled && !startup.currentExecutable ? "启动项指向另一便携目录；关闭后重新开启可改为当前程序。" : "登录 Windows 后启动 DSH Patrick；不会自动接管 DSH。"}</small></div>
      <Switch label="开机启动" checked={startup?.enabled ?? false} disabled={!native || busy || !startup} onChange={() => {
        setBusy(true); setStartupError("");
        api.setAutostart(!startup?.enabled).then(setStartup).catch(e => setStartupError(String(e))).finally(() => setBusy(false));
      }} /></div>
    {startupError && <p role="alert" className="settings-message">{startupError}</p>}
    <div className="settings-row"><div><b>软件更新</b><small>当前版本 {version} · 仅手动检查，不自动下载或安装。</small></div>
      <button data-check-update disabled={!native || checking} onClick={async () => {
        setChecking(true); setUpdate("");
        try { const info = await api.checkUpdate(); setUpdate(info.state === "available" ? `发现新版本 ${info.version}，可打开发布页下载。` : info.state === "current" ? `当前已是最新正式版 ${info.version}。` : `当前版本高于已发布正式版 ${info.version}。`); }
        catch (e) { setUpdate(String(e)); }
        finally { setChecking(false); }
      }}>{checking ? "检查中…" : "检查更新"}</button></div>
    {update && <div className="settings-update-result"><p role="status" data-update-result>{update}</p><button disabled={!native} onClick={() => void api.openReleasePage().catch(e => setUpdate(String(e)))}>打开发布页</button></div>}
  </section>;
}

export function StorageSettings({ snapshot, onRefresh }: { snapshot: Snapshot; onRefresh: () => Promise<void> }) {
  const [info, setInfo] = useState<StorageInfo | null>(null), [error, setError] = useState("");
  const [confirm, setConfirm] = useState(false), [busy, setBusy] = useState(false);
  const [scope,setScope] = useState("all");
  const active = snapshot.runtime.activeRequests > 0;
  useEffect(() => {
    let live = true;
    api.statisticsStorage().then(v => { if (live) { setInfo(v); setError(""); } }).catch(e => { if (live) setError(String(e)); });
    return () => { live = false; };
  }, [snapshot.runtime.historyVersion]);
  return <section className="panel settings-extra storage-settings">
    <div className="panel-title"><h3>存储管理</h3></div>
    <div className="settings-row"><div><b>统计数据</b><small data-storage-count>{info ? `统计记录 ${info.calls} 条 · 加密文件 ${(info.bytes / 1024).toFixed(1)} KiB` : "正在读取…"}</small><small>仅保存用量元数据，不保存对话正文。</small></div>
      <button type="button" className="text-button" data-clear-details disabled={!native || active || busy} onClick={() => {setScope("details");setError("");setConfirm(true);}}>仅清理详细内容</button>
      <button className="text-button" data-clear-storage disabled={!native || active || busy || !info} onClick={() => { setScope("all"); setError(""); setConfirm(true); }}>清理存储空间</button></div>
    {active && <p className="settings-message">仍有请求进行中或排队中，结束后可清理。</p>}
    {(error || info?.historyError) && <p role="alert" className="settings-message">{error || info?.historyError}</p>}
    {confirm && <Modal title={scope === "all" ? "清理全部统计数据？" : "仅清理详细内容？"} onClose={() => { if (!busy) setConfirm(false); }}>
      <div className="confirm-body"><p>{scope === "all" ? "将清空历史 Token 统计、持久调用记录、请求响应详情和测试结果。" : "只清理请求与响应正文，保留调用信息和首页统计。"}模型、API Key、软件设置及 DSH 配置保持不变。</p><p>此操作不可撤销。</p>
        {error && <p role="alert">{error}</p>}
      </div>
      <footer className="modal-foot"><button data-cancel-clear disabled={busy} onClick={() => setConfirm(false)}>取消</button>
        <button className="danger" data-confirm-clear disabled={busy || active} onClick={async () => {
          setBusy(true); setError("");
          try { if(scope === "all") await api.clearStatistics(); else await api.clearCallDetails(); setInfo(await api.statisticsStorage()); await onRefresh(); setConfirm(false); }
          catch (e) { setError(String(e)); }
          finally { setBusy(false); }
        }}>{busy ? "清理中…" : "确认清理"}</button></footer>
    </Modal>}
  </section>;
}
