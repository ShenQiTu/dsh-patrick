import { useState } from "react";
import { Empty, Icon, Modal } from "./ui";
import { Select } from "./Select";
import { protocols, type CallRecord } from "./types";

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
export function CallDetails({
  record,
  onClose,
}: {
  record: CallRecord;
  onClose: () => void;
}) {
  return (
    <Modal
      title={record.source === "test" ? "模型测试详情" : "请求详情"}
      subtitle={record.name + " · " + protocols[record.protocol]}
      wide
      onClose={onClose}
    >
      <div className="detail-body">
        <div className={"result-banner " + record.state}>
          <Icon name={record.state === "success" ? "check" : "info"} />
          <div>
            <b>{stateNames[record.state]}</b>
            <p>{record.message}</p>
          </div>
          <span>HTTP {record.status || "—"}</span>
        </div>
        <div className="detail-metrics">
          {[
            ["总耗时", <Duration value={record.durationMs} />],
            ["首个文本响应", <Duration value={record.firstTokenMs} />],
            [
              "平均输出速率",
              record.tokensPerSecond === null
                ? "—"
                : `${record.tokensPerSecond} t/s`,
            ],
            ["输入 Token", record.inputTokens ?? "—"],
            ["输出 Token", record.outputTokens ?? "—"],
            ["响应传输量", `${(record.bytes / 1024).toFixed(1)} KiB`],
          ].map(([label, value]) => (
            <div key={String(label)}>
              <small>{label}</small>
              <b>{value}</b>
            </div>
          ))}
        </div>
        <div className="detail-info">
          <div data-zen-status>
            <span>Zen 请求头</span>
            <span>
              {record.source === "test" ? "本机测试" : `客户端：${record.zenHeaderReceived === true ? "已收到" : record.zenHeaderReceived === false ? "未收到" : "未确认"}`}
              {" · "}上游：{record.zenHeaderSent === true ? "已携带" : record.zenHeaderSent === false ? "未携带" : "未确认"}
            </span>
          </div>
          <div>
            <span>服务商</span>
            <code>{record.host}</code>
          </div>
          <div>
            <span>开始时间</span>
            <span>{new Date(record.startedAt).toLocaleString()}</span>
          </div>
          <div>
            <span>请求形式</span>
            <span>
              {record.streamed ? "流式 SSE" : "非流式 JSON"} ·{" "}
              {record.source === "test" ? "连通性测试" : "网关请求"}
            </span>
          </div>
        </div>
        {record.preview && (
          <section className="response-preview">
            <h3>测试响应预览</h3>
            <pre>{record.preview}</pre>
            <small>
              最多 500 字符；已移除配置中的凭据值。正式请求不记录正文。
            </small>
          </section>
        )}
        <p className="detail-note">
          Zen 仅记录请求头有无，不保存其内容；上游状态在收到响应后确认。
          “—” 表示上游未上报或该指标不适用。平均输出速率 = 上游输出 Token /
          总耗时（含等待与推理）；不是纯文本生成速度，不用字符数估算 Token。
        </p>
      </div>
      <footer className="modal-foot">
        <span>记录仅保留在本次应用会话中</span>
        <button onClick={onClose}>关闭</button>
      </footer>
    </Modal>
  );
}
export function Calls({
  records,
  onDetail,
  onClear,
  disabled,
}: {
  records: CallRecord[];
  onDetail: (r: CallRecord) => void;
  onClear: () => void;
  disabled: boolean;
}) {
  const [query, setQuery] = useState(""),
    [status, setStatus] = useState("all"),
    [source, setSource] = useState("all");
  const visible = records.filter(
    (r) =>
      (status === "all" || r.state === status) &&
      (source === "all" || r.source === source) &&
      (r.name + " " + r.host).toLowerCase().includes(query.toLowerCase()),
  );
  return (
    <section className="panel">
      <div className="panel-title">
        <h3>
          请求记录 <span className="record-count">{records.length}</span>
        </h3>
        <button disabled={disabled || records.length === 0} onClick={onClear}>
          清空记录
        </button>
      </div>
      <div className="calls-filters">
        <label className="search">
          <Icon name="search" size={16} />
          <input
            aria-label="搜索调用记录"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索模型或服务商"
          />
        </label>
        <Select
          label="筛选请求状态"
          value={status}
          onChange={setStatus}
          options={[
            { value: "all", label: "全部状态" },
            ...Object.entries(stateNames).map(([value, label]) => ({
              value,
              label,
            })),
          ]}
        />
        <Select
          label="筛选请求来源"
          value={source}
          onChange={setSource}
          options={[
            { value: "all", label: "全部来源" },
            { value: "gateway", label: "网关请求" },
            { value: "test", label: "模型测试" },
          ]}
        />
      </div>
      <div className="table-head">
        <span>模型 / 时间</span>
        <span>来源</span>
        <span>状态</span>
        <span>耗时</span>
      </div>
      {visible.length === 0 ? (
        <Empty
          icon="chart"
          title={records.length ? "没有匹配的记录" : "暂无请求"}
          description="手动测试模型或通过网关发送请求后，这里会显示实际记录。"
        />
      ) : (
        <div className="calls-list">
          {visible.map((r) => (
            <button key={r.id} className="call-row" onClick={() => onDetail(r)}>
              <span>
                <b>{r.name}</b>
                <small>
                  {new Date(r.startedAt).toLocaleTimeString()} · {r.host}
                </small>
              </span>
              <span>{r.source === "test" ? "模型测试" : "网关请求"}</span>
              <span>
                <i className={"state-tag " + r.state}>{stateNames[r.state]}</i>
              </span>
              <span>
                {r.state === "running" ? (
                  "—"
                ) : (
                  <Duration value={r.durationMs} />
                )}{" "}
                <Icon name="arrow" size={13} />
              </span>
            </button>
          ))}
        </div>
      )}
      <div className="calls-foot">
        仅保留本次会话最近记录，不保存 Key、请求头、对话正文。
      </div>
    </section>
  );
}
