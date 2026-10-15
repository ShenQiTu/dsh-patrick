import { useEffect, useState, type ReactNode } from "react";
import { Empty, Icon, Modal } from "./ui";
import { Select } from "./Select";
import { api, native } from "./api";
import { protocols, type CallRecord, type CallDetail } from "./types";

export const stateNames: Record<CallRecord["state"], string> = {
  running: "进行中",
  success: "成功",
  error: "失败",
  cancelled: "已取消",
};
export function Duration({ value }: { value: number | null }) {
  return (
    <>
      {value === null
        ? "—"
        : value < 1000
          ? `${value} ms`
          : `${(value / 1000).toFixed(2)} s`}
    </>
  );
}
export function TestStatus({
  result,
  testing,
  onDetail,
}: {
  result?: CallRecord;
  testing: boolean;
  onDetail: (r: CallRecord) => void;
}) {
  if (testing)
    return (
      <div className="test-status running">
        <span className="spinner" />
        正在测试…<small>可随时取消</small>
      </div>
    );
  if (!result)
    return (
      <div className="test-status">
        <span className="status-dot neutral" />
        未测试<small>点击测试检查连接</small>
      </div>
    );
  return (
    <button
      className={"test-status " + result.state}
      onClick={() => onDetail(result)}
    >
      <span className="status-dot" />
      {stateNames[result.state]}
      {result.state === "success" && (
        <span
          className="test-speed"
          title="端到端平均输出速率：上游输出 Token / 总耗时"
        >
          {result.tokensPerSecond === null ? (
            <Duration value={result.durationMs} />
          ) : (
            `${result.tokensPerSecond} tokens/s`
          )}
        </span>
      )}
      <small>查看详情 ›</small>
    </button>
  );
}
export function CallDetails({ record, onClose }: { record: CallRecord; onClose: () => void }) {
  const [tab, setTab] = useState("info");
  const [detail, setDetail] = useState<CallDetail | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  useEffect(() => {
    let disposed = false;
    setDetail(null); setError("");
    if (!native) return;
    setLoading(true);
    api.callDetail(record.id).then(value => { if (!disposed) setDetail(value); })
      .catch(e => { if (!disposed) setError(String(e)); })
      .finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [record.id, record.state]);
  const r = detail?.record ?? record, info = r.info ?? {}, payload = detail?.payload;
  const unknown = (v: string | number | null | undefined) => v ?? "—";
  const timing = (v: number | null | undefined) => v == null ? "—" : `${v} ms`;
  const rows: [string, string | number][] = [
    ["Call ID", r.id], ["调用类型", r.source === "test" ? "模型测试" : "网关请求"],
    ["显示名称", r.name], ["模型名称", unknown(info.model)],
    ["配置 ID", r.modelId || "—"], ["请求协议", protocols[r.protocol] ?? "—"],
    ["最终请求地址", unknown(info.requestUrl)], ["思考强度", unknown(info.reasoning)],
    ["状态", stateNames[r.state]], ["结果说明", r.message], ["Finish Reason", unknown(info.finishReason)],
    ["HTTP Status", r.status || "—"], ["开始时间", new Date(r.startedAt).toLocaleString()],
    ["耗时", info.legacy ? "—" : timing(r.state === "running" ? null : r.durationMs)],
    ["TTFB · 响应头", timing(info.ttfbMs)], ["TTFR · 首个有效内容", timing(info.ttfrMs)],
    ["TTFT · 首个文本", timing(r.firstTokenMs)],
    ["Input Token", unknown(r.inputTokens)], ["Output Token", unknown(r.outputTokens)],
    ["Total Token", r.inputTokens == null || r.outputTokens == null ? "—" : r.inputTokens + r.outputTokens],
    ["Cache Read Token", unknown(r.cacheReadTokens)], ["Cache Write Token", unknown(r.cacheWriteTokens)],
    ["Reasoning Token", unknown(info.reasoningTokens)], ["消息数", unknown(info.messageCount)],
    ["工具数", unknown(info.toolCount)], ["详细记录", info.legacy ? "旧版未记录" : info.detailed ? "已启用" : "未启用"],
    ["响应传输量", info.legacy ? "—" : `${r.bytes} bytes`],
    ["Error Kind", unknown(info.errorKind)], ["Error Message", r.state === "error" || r.state === "cancelled" ? r.message : "—"],
  ];
  const missing = r.state === "running" ? "请求进行中，结束后可查看已采集内容。" : info.legacy ? "旧版本未保存请求和响应内容。" : info.detailed ? "详细内容已清理或未采集到。" : "本次未开启详细记录。可在系统设置 → 调用记录中开启，仅对之后的请求生效。";
  return <Modal title={r.name} subtitle="调用详情" wide onClose={onClose}>
    <div className="call-details" data-state={r.state}>
      <div role="tablist" aria-label="调用详情页签" className="call-tabs">
        {[["info","调用信息"],["request","请求"],["response","响应流"]].map(([value,label]) =>
          <button key={value} type="button" role="tab" aria-selected={tab===value} aria-controls={`call-tab-${value}`} id={`call-tab-button-${value}`} onClick={() => setTab(value)}>{label}</button>)}
      </div>
      {loading && <p role="status">正在读取调用详情…</p>}
      {error && <p className="form-error" role="alert">{error}</p>}
      <section role="tabpanel" id={`call-tab-${tab}`} aria-labelledby={`call-tab-button-${tab}`} className="call-tab-content">
        {tab === "info" && <>
          {info.legacy && <p className="call-note">旧版历史摘要：模型、耗时、请求和响应正文未记录；已有统计保持原样。</p>}
          <dl className="call-info-grid">{rows.map(([label,value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>
          <div className="call-note" data-zen-status>Zen 请求头：{r.source === "test" ? "本机测试" : `客户端：${r.zenHeaderReceived == null ? "—" : r.zenHeaderReceived ? "已收到" : "未收到"}`} · 上游 {r.zenHeaderSent == null ? "—" : r.zenHeaderSent ? "已携带" : "未携带"}（仅表示头存在，不代表上游认可）{r.zenSessionState && <span data-zen-session-state> · {({absent:"未提供有效动态会话头",primary:"旧会话头有效，双头已统一",alias:"别名会话头有效，双头已统一",both:"双会话头一致",conflict_primary:"双头冲突，已按 x-opencode-session 统一"} as Record<string,string>)[r.zenSessionState] || "—"}</span>}</div>
          {r.preview && <section className="response-preview"><h3>测试响应预览</h3><pre>{r.preview}</pre><small>最多 500 字符；预览仅在本次运行保留。</small></section>}
          <p className="call-note">“—” 表示未上报或不适用。思考强度来自实际发出的请求，不猜测服务商默认值；Token 不用字符数估算。</p>
        </>}
        {tab === "request" && (payload ? <>
          <h3>请求头（凭据已脱敏）</h3><pre data-call-headers>{JSON.stringify(payload.headers,null,2)}</pre>
          <h3>请求体</h3>{payload.requestTruncated ? <p role="status">请求体超过 128 KiB，未保存正文；转发请求保持完整。</p> : <pre data-call-request>{JSON.stringify(payload.body,null,2)}</pre>}
        </> : <p className="call-empty">{missing}</p>)}
        {tab === "response" && (payload ? <>
          <p className="call-note">传输块数 {payload.responseChunks} · 实际接收 {r.bytes} bytes{payload.responseTruncated ? " · 记录达到 128 KiB 上限，以下仅为前段内容" : ""}</p>
          {payload.response ? <pre data-call-response>{payload.response}</pre> : <p className="call-empty">未记录到响应正文；具体状态见调用信息。</p>}
        </> : <p className="call-empty">{missing}</p>)}
      </section>
    </div>
    <footer className="modal-foot"><span>调用记录本机加密保存；详细模式可能包含对话正文。</span><button onClick={onClose}>关闭</button></footer>
  </Modal>;
}

export function Calls({ records, onDetail, onClear, disabled, error }: {
  records: CallRecord[]; onDetail: (r: CallRecord) => void; onClear: () => void; disabled: boolean; error?: string | null;
}) {
  const [query,setQuery] = useState(""), [status,setStatus] = useState("all"), [source,setSource] = useState("all");
  const [page,setPage] = useState(1), [pageSize,setPageSize] = useState(20);
  const visible = records.filter(r => (status === "all" || r.state === status) && (source === "all" || r.source === source)
    && `${r.name} ${r.host} ${r.info?.model ?? ""}`.toLowerCase().includes(query.toLowerCase()));
  const pageCount = Math.max(1,Math.ceil(visible.length/pageSize)), currentPage=Math.min(page,pageCount);
  useEffect(() => setPage(1), [query,status,source,pageSize]);
  useEffect(() => { if (page>pageCount) setPage(pageCount); },[page,pageCount]);
  const value = (v: string | number | null | undefined) => v ?? "—";
  const columns: {title: string; width: number; render:(r:CallRecord)=>ReactNode}[] = [
    {title:"状态",width:90,render:r=><i className={`state-tag ${r.state}`}>{stateNames[r.state]}</i>},
    {title:"显示名称",width:180,render:r=>r.name},
    {title:"时间",width:185,render:r=>new Date(r.startedAt).toLocaleString()},
    {title:"模型名称",width:200,render:r=>value(r.info?.model)},
    {title:"来源",width:100,render:r=>r.source==="test"?"模型测试":"网关请求"},
    {title:"思考强度",width:100,render:r=>value(r.info?.reasoning)},
    {title:"HTTP",width:75,render:r=>r.status||"—"},
    {title:"耗时",width:100,render:r=>r.info?.legacy||r.state==="running"?"—":<Duration value={r.durationMs}/>},
    {title:"TTFB",width:100,render:r=><Duration value={r.info?.ttfbMs??null}/>},
    {title:"TTFR",width:100,render:r=><Duration value={r.info?.ttfrMs??null}/>},
    {title:"TTFT",width:100,render:r=><Duration value={r.firstTokenMs}/>},
    {title:"Input Token",width:110,render:r=>value(r.inputTokens)},
    {title:"Output Token",width:110,render:r=>value(r.outputTokens)},
    {title:"Cache Read",width:110,render:r=>value(r.cacheReadTokens)},
    {title:"Cache Write",width:110,render:r=>value(r.cacheWriteTokens)},
    {title:"Finish Reason",width:120,render:r=>value(r.info?.finishReason)},
    {title:"Error Kind",width:120,render:r=>value(r.info?.errorKind)},
    {title:"服务商",width:200,render:r=>r.host||"—"},
  ];
  return <section className="calls-panel">
    <div className="calls-toolbar">
      <label className="search"><Icon name="search" size={16}/><input aria-label="搜索调用记录" value={query} onChange={e=>setQuery(e.target.value)} placeholder="搜索模型或服务商"/></label>
      <Select label="筛选请求状态" value={status} onChange={setStatus} options={[{value:"all",label:"全部状态"},...Object.entries(stateNames).map(([value,label])=>({value,label}))]}/>
      <Select label="筛选请求来源" value={source} onChange={setSource} options={[{value:"all",label:"全部来源"},{value:"gateway",label:"网关请求"},{value:"test",label:"模型测试"}]}/>
      <button disabled={disabled||records.length===0} onClick={onClear}>清空记录</button>
    </div>
    {error && <p className="form-error" role="alert">调用记录保存异常：{error}</p>}
    <div className="calls-table-region">
      <div className="calls-scroll" tabIndex={0} aria-label="调用记录表格，可横向滚动查看指标">
        <table className="calls-table" style={{width:columns.reduce((n,c)=>n+c.width,64)}}>
          <colgroup>{columns.map(c=><col key={c.title} style={{width:c.width}}/>)}<col style={{width:64}}/></colgroup>
          <thead><tr>{columns.map(c=><th key={c.title}>{c.title}</th>)}<th className="call-action">操作</th></tr></thead>
          <tbody>{visible.slice((currentPage-1)*pageSize,currentPage*pageSize).map(r=><tr className="call-row" key={r.id}>
            {columns.map(c=><td key={c.title} title={typeof c.render(r)==="string"?String(c.render(r)):undefined}>{c.render(r)}</td>)}
            <td className="call-action"><button className="call-detail-button" aria-label={`查看详情 ${r.name}`} title="查看详情" onClick={()=>onDetail(r)}><Icon name="eye" size={16}/></button></td>
          </tr>)}</tbody>
        </table>
      </div>
      {visible.length===0 && <div className="calls-empty"><Empty icon="chart" title={records.length?"没有匹配的记录":"暂无请求"} description="模型测试和网关请求会在此记录，重启后保留。"/></div>}
    </div>
    <div className="calls-pagination">
      <span data-calls-total>共 {visible.length} 条</span>
      <Select label="每页条数" value={String(pageSize)} onChange={v=>setPageSize(Number(v))} options={[20,50,100].map(n=>({value:String(n),label:`${n} 条/页`}))}/>
      <span data-calls-page>第 {currentPage} / {pageCount} 页</span>
      {[["第一页","«",1,currentPage===1],["上一页","‹",currentPage-1,currentPage===1],["下一页","›",currentPage+1,currentPage===pageCount],["最后一页","»",pageCount,currentPage===pageCount]].map(([label,icon,next,off])=>
        <button key={String(label)} aria-label={String(label)} title={String(label)} disabled={Boolean(off)} onClick={()=>setPage(Number(next))}>{icon}</button>)}
    </div>
  </section>;
}
