import { useEffect, useState } from "react";
import { api } from "./api";
import { Help, Select } from "./Select";
import { Modal } from "./ui";
import { calendar, presetRange, summarize, type Preset, type Range, type UsageRecord } from "./stats";

export const ranges: [Preset, string][] = [["hour", "近1小时"], ["today", "近1自然日"], ["ten", "近10分钟"], ["week", "近一周"], ["month", "近一个月"]];
export function RangeActions({ value, onSelect, custom, onCustom }: { value: Preset; onSelect: (v: Preset) => void; custom: Range | null; onCustom: (v: Range) => void }) {
  const [open, setOpen] = useState(false), [error, setError] = useState("");
  const [start, setStart] = useState(""), [end, setEnd] = useState("");
  function showCustom() {
    const range = custom ?? presetRange("today");
    setStart(localInput(range.start)); setEnd(localInput(range.end)); setError(""); setOpen(true);
  }
  return <>
    <div className="overview-range segmented" aria-label="统计时间范围">
      {ranges.map(([key, text]) => <button key={key} aria-pressed={value === key} onClick={() => onSelect(key)}>{text}</button>)}
      <button aria-pressed={value === "custom"} onClick={showCustom}>自定义</button>
    </div>
    {open && <Modal title="自定义统计范围" onClose={() => setOpen(false)}>
      <form className="custom-range" onSubmit={e => {
        e.preventDefault(); const from = new Date(start).getTime(), to = new Date(end).getTime();
        if (!Number.isFinite(from) || !Number.isFinite(to) || from >= to) { setError("开始时间须早于结束时间"); return; }
        onCustom({start:from,end:to}); setOpen(false);
      }}>
        <label>开始时间<input type="datetime-local" aria-label="统计开始时间" required value={start} onChange={e => setStart(e.target.value)} /></label>
        <label>结束时间<input type="datetime-local" aria-label="统计结束时间" required value={end} onChange={e => setEnd(e.target.value)} /></label>
        {error && <p role="alert">{error}</p>}
        <footer><button type="button" onClick={() => setOpen(false)}>取消</button><button type="submit" className="primary">应用</button></footer>
      </form>
    </Modal>}
  </>;
}
const number = (n: number) => Intl.NumberFormat("zh-CN", { notation: "compact", maximumFractionDigits: 1 }).format(n);
const dateTime = (n: number) => new Date(n).toLocaleString("zh-CN", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });
function localInput(ms: number) { const d = new Date(ms); return new Date(ms - d.getTimezoneOffset() * 60000).toISOString().slice(0,16); }

export function Overview({ version, historyError, preset, custom }: { version: number; historyError: string | null; preset: Preset; custom: Range | null }) {
  const [records, setRecords] = useState<UsageRecord[]>([]), [error, setError] = useState("");
  const [source, setSource] = useState("gateway");
  useEffect(() => {
    let active = true;
    api.statistics().then(r => { if (active) { setRecords(r); setError(""); } }).catch(e => { if (active) setError(String(e)); });
    return () => { active = false; };
  }, [version]);
  const range = preset === "custom" ? custom ?? presetRange("month") : presetRange(preset);
  const data = summarize(records, range, source);
  const days = calendar(records, range.end, source);
  const max = Math.max(1, ...data.buckets.map(b => b.input + b.output + b.read + b.write));
  const dayMax = Math.max(1, ...days.map(d => d.count));
  const rate = data.cacheRate;
  const rangeText = `${dateTime(range.start)} — ${dateTime(range.end)}`;
  const axisLabel = (ms: number) => new Date(ms).toLocaleString("zh-CN", range.end - range.start <= 86400000
    ? {hour:"2-digit",minute:"2-digit"} : {month:"numeric",day:"numeric"});
  const months = days.flatMap((d, i) => i === 0 || d.date.slice(5,7) !== days[i-1].date.slice(5,7)
    ? [{index:i,label:`${Number(d.date.slice(5,7))}月`}] : []);
  return <section className="usage-overview" data-overview>
    {(error || historyError) && <div role="alert" className="error-banner">{error || historyError}；本次会话数据仍可查看，统计历史可能不完整。</div>}
    <div className="usage-toolbar">
      <div className="usage-source"><Select label="统计请求来源" value={source} onChange={setSource} options={[{value:"gateway",label:"网关请求"},{value:"test",label:"模型测试"},{value:"all",label:"全部来源"}]} />
        <Help text={`统计范围：${rangeText}。统计保留最近 366 天、最多 10,000 条请求元数据，Windows 当前用户加密存储，不保存对话正文。网关请求不包含模型测试；DSH 尚未接入时，不会出现主 DSH 的调用。`} />
      </div>
      <div className="token-legend"><span><i className="token-input" />输入（非缓存）</span><span><i className="token-output" />输出</span><span><i className="token-read" />缓存读取</span><span><i className="token-write" />缓存写入</span></div>
    </div>
    <div className="token-chart" role="img" aria-label={`Token 用量趋势，${data.selected.length} 次调用，已上报 ${data.input + data.output} Token`}>
      <div className="token-bars">
        {data.buckets.map((b, i) => <div className="token-column" key={i} title={`${dateTime(b.start)} 起\n${b.calls} 次调用\n输入（非缓存）${b.input} · 输出 ${b.output}\n缓存读取 ${b.read} · 缓存写入 ${b.write}`}>
          <div className="token-stack">{(["output", "input", "write", "read"] as const).map(k => <i key={k} className={`token-${k}`} style={{height:`${100*b[k]/max}%`}} />)}</div>
        </div>)}
        {!data.reported && <p className="chart-empty">{data.selected.length ? "上游未返回 Token 用量" : "当前范围暂无调用"}</p>}
      </div>
      <div className="token-axis" aria-label={rangeText}>{[0, 0.25, 0.5, 0.75, 1].map(v => <span key={v} title={dateTime(range.start + (range.end-range.start)*v)}>{axisLabel(range.start + (range.end-range.start)*v)}</span>)}</div>
    </div>
    <div className="usage-metrics">
      <section><div>缓存命中率 <Help text="仅使用同时返回输入 Token 与缓存读取 Token 的请求计算；未上报的数据不按零命中处理。" /></div>
        <div className="cache-gauge"><svg viewBox="0 0 120 70" aria-hidden="true"><path d="M 10 60 A 50 50 0 0 1 110 60" pathLength="100" className="gauge-track"/><path d="M 10 60 A 50 50 0 0 1 110 60" pathLength="100" className="gauge-value" strokeDasharray={`${Math.min(100,Math.max(0,(rate??0)*100))} 100`} /></svg><b>{rate === null ? "—" : `${(rate*100).toFixed(2)}%`}</b></div>
        <small>{data.cacheSamples ? `${data.cacheSamples} 次上报缓存用量` : "上游未提供缓存数据"}</small></section>
      <section><div>LLM 调用 <Help text="按当前时间与来源筛选；默认仅统计经过网关的请求，模型测试单独查看。" /></div><b className="usage-value" data-usage-calls>{number(data.selected.length)}</b><small title={`成功 ${data.success} / 异常 ${data.failed} / 取消 ${data.cancelled}`}>成功 {number(data.success)} / 异常 {number(data.failed)}{data.cancelled > 0 ? <><br/>取消 {number(data.cancelled)}</> : ""}</small></section>
      <section><div>Token 消耗 <Help text="使用上游返回的用量，输入总量包含缓存，不重复计数；缺失用量不推算。" /></div><b className="usage-value" data-usage-tokens>{data.reported ? number(data.input+data.output) : "—"}</b><small title={`输入 ${data.input} / 输出 ${data.output}`}>输入 {number(data.input)} / 输出 {number(data.output)}{data.reported < data.selected.length && <><br/>{number(data.selected.length-data.reported)} 次未上报用量</>}</small></section>
      <section><div>价值估算 <Help text="当前未配置模型价格；不会用其他服务商价格或虚构金额估算你的费用。" /></div><b className="usage-value">—</b><small>未配置计价</small></section>
    </div>
    <div className="usage-calendar" aria-label="近365天调用活动">
      <div className="calendar-squares">{days.map(d => <span key={d.date} title={`${d.date} · ${d.count} 次调用`} style={d.count ? {backgroundColor:`color-mix(in srgb, var(--activity-green) ${25+75*d.count/dayMax}%, var(--surface))`} : undefined} />)}</div>
      <div className="calendar-axis" aria-label={`${days[0].date} 至 ${days[364].date}，近 365 天调用活动`}>{months.map(m => <span key={m.index}>{m.label}</span>)}</div>
    </div>
  </section>;
}
