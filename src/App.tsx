import { useCallback, useEffect, useRef, useState } from "react";
import { api, native } from "./api";
import { ModelEditor } from "./ModelEditor";
import { Empty, Field, Icon, Modal, Switch } from "./ui";
import {
  protocols,
  type CallRecord,
  type Model,
  type Page,
  type Settings,
  type Snapshot,
} from "./types";
import { CallDetails, Calls, TestStatus } from "./Calls";
import { listen } from "@tauri-apps/api/event";
import patrickIcon from "./assets/patrick-icon.png";
import { DshConnection } from "./DshConnection";
import { AppHeader } from "./AppHeader";
import { Select } from "./Select";
import { Overview, RangeActions } from "./Overview";
import type { Preset, Range } from "./stats";

const pages: Record<Page, { title: string; subtitle: string; icon: string }> = {
  help: { title: "使用教程", subtitle: "本地网关快速上手", icon: "tutorial" },
  overview: {
    title: "数据概览",
    subtitle: "你的模型，你的本地网关。",
    icon: "grid",
  },
  models: {
    title: "DSH",
    subtitle: "在一个地方管理所有模型与上游连接。",
    icon: "dsh",
  },
  calls: {
    title: "调用记录",
    subtitle: "查看请求状态、耗时与用量。",
    icon: "chart",
  },
  connection: {
    title: "DSH 接入",
    subtitle: "本地网关与客户端的连接状态。",
    icon: "plug",
  },
  settings: {
    title: "系统设置",
    subtitle: "让网关按你的习惯运行。",
    icon: "settings",
  },
};

export function App() {
  const [rangePreset, setRangePreset] = useState<Preset>("month");
  const [customRange, setCustomRange] = useState<Range | null>(null);
  const [state, setState] = useState<Snapshot | null>(null),
    [page, setPage] = useState<Page>("models");
  const [editor, setEditor] = useState<Model | null | undefined>(),
    [deleting, setDeleting] = useState<Model | null>(null);
  const [search, setSearch] = useState(""),
    [grouping, setGrouping] = useState("flat"),
    [notice, setNotice] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState<Set<string>>(new Set()),
    [testTargets, setTestTargets] = useState<string[] | null>(null),
    [detail, setDetail] = useState<CallRecord | null>(null),
    [confirmClear, setConfirmClear] = useState(false),
    [batch, setBatch] = useState(false);
  const cancelled = useRef(false),
    activeTests = useRef(new Set<string>());
  useEffect(() => {
    if (!native) return;
    const listener = listen<string>("gateway-busy-close", (event) =>
      setNotice(event.payload),
    );
    return () => {
      void listener.then((unlisten) => unlisten());
    };
  }, []);
  const refresh = useCallback(async () => {
    const next = await api.snapshot();
    setState(next);
    document.title = `DSH Patrick v${next.appVersion}`;
    setError("");
  }, []);
  useEffect(() => {
    refresh().catch((e) => setError(String(e)));
  }, [refresh]);
  useEffect(() => {
    if (!native) return;
    let disposed = false,
      pending = false;
    const timer = setInterval(async () => {
      if (pending) return;
      pending = true;
      try {
        const next = await api.snapshot();
        if (!disposed) setState(next);
      } catch {
      } finally {
        pending = false;
      }
    }, 1000);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, []);
  useEffect(() => {
    if (!state) return;
    const media = matchMedia("(prefers-color-scheme: dark)");
    const apply = () =>
      (document.documentElement.dataset.theme =
        state.settings.theme === "system"
          ? media.matches
            ? "dark"
            : "light"
          : state.settings.theme);
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [state?.settings.theme]);
  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(""), 4000);
    return () => clearTimeout(timer);
  }, [notice]);
  async function action(fn: () => Promise<unknown>, message: string) {
    setBusy(true);
    try {
      await fn();
      await refresh();
      setNotice(message);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  }
  async function testModels(ids: string[]) {
    setTestTargets(null);
    setBatch(ids.length > 1);
    cancelled.current = false;
    const queue = [...ids];
    let succeeded = 0,
      failed = 0;
    const worker = async () => {
      while (queue.length && !cancelled.current) {
        const id = queue.shift()!;
        activeTests.current.add(id);
        setTesting(new Set(activeTests.current));
        try {
          const result = await api.test(id);
          if (result.state === "success") succeeded++;
          else failed++;
        } catch (e) {
          if (!cancelled.current) {
            failed++;
            setNotice(String(e));
          }
        } finally {
          activeTests.current.delete(id);
          setTesting(new Set(activeTests.current));
          await refresh();
        }
      }
    };
    try {
      await Promise.all(
        Array.from({ length: Math.min(3, ids.length) }, worker),
      );
      setNotice(
        cancelled.current
          ? "测试已取消"
          : `测试完成：${succeeded} 成功，${failed} 失败`,
      );
    } finally {
      setBatch(false);
    }
  }
  async function cancelTests() {
    cancelled.current = true;
    await Promise.all([...activeTests.current].map((id) => api.cancel(id)));
  }
  const current = pages[page];
  const models = state?.models ?? [];
  const calls = state?.runtime.calls ?? [];
  const running = state?.gatewayRunning ?? false;
  const visible = models.filter((m) =>
    (m.name + " " + m.model + " " + m.notes + " " + m.baseUrl)
      .toLowerCase()
      .includes(search.toLowerCase()),
  );
  const groups: Record<string, Model[]> = {};
  for (const m of visible) {
    const key =
      grouping === "provider"
        ? new URL(m.baseUrl).host
        : grouping === "protocol"
          ? protocols[m.protocol]
          : "";
    (groups[key] ??= []).push(m);
  }
  return (
    <div className="app-shell">
      <AppHeader version={state?.appVersion ?? "0.1.3"} onError={setError} />
      <aside className="sidebar">
        <nav aria-label="主导航">
          {(["overview", "calls"] as Page[]).map((id) => (
            <button
              key={id}
              data-nav={id}
              aria-current={page === id ? "page" : undefined}
              onClick={() => setPage(id)}
            >
              <Icon name={pages[id].icon} size={18} />
              <span>{pages[id].title}</span>
            </button>
          ))}
          <p className="nav-label">模型配置</p>
          <button
            data-nav="models"
            aria-current={page === "models" ? "page" : undefined}
            onClick={() => setPage("models")}
          >
            <Icon name="dsh" size={18} />
            <span>DSH</span>
            <span
              className={
                "menu-status " + (state?.dshConnected ? "connected" : "")
              }
              data-dsh-status
              title="客户端接入状态与网关监听状态分开显示"
            >
              {state?.dshConnected ? "已同步" : "未接管"}
            </span>
          </button>
          <p className="nav-label">设置</p>
          {(["connection", "settings", "help"] as Page[]).map((id) => (
            <button
              key={id}
              data-nav={id}
              aria-current={page === id ? "page" : undefined}
              onClick={() => setPage(id)}
            >
              <Icon name={pages[id].icon} size={18} />
              <span>{pages[id].title}</span>
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <span className={"status-dot " + (running ? "" : "neutral")} />
          {running ? "网关监听中" : "网关未启动"}
          <small>127.0.0.1:{state?.settings.port ?? 8787}</small>
        </div>
      </aside>
      <div className="main-shell">
        <header className="topbar">
          <h1>{current.title}</h1>
          <div className="topbar-actions">
            {page === "overview" && <RangeActions value={rangePreset} onSelect={setRangePreset} custom={customRange} onCustom={range => { setCustomRange(range); setRangePreset("custom"); }} />}
            {page === "models" && <>
            <span className="gateway-label">
              {running ? "网关运行" : "网关关闭"}
            </span>
            <Switch
              checked={running}
              label="启动或关闭网关"
              disabled={
                !native || busy || (!running && !models.some((m) => m.enabled))
              }
              onChange={() =>
                void action(
                  () => (running ? api.stop() : api.start()),
                  running ? "网关已关闭，DSH 接入已恢复" : "网关已启动，启用模型已同步到 DSH",
                )
              }
            />
            </>}
            {page === "models" && (
              <>
                <span className="topbar-divider" />
                <div className="segmented">
                  {Object.entries({
                    flat: "默认平铺",
                    provider: "按供应商",
                    protocol: "按协议",
                  }).map(([value, label]) => (
                    <button
                      key={value}
                      aria-pressed={grouping === value}
                      onClick={() => setGrouping(value)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
                <button
                  data-batch-test
                  disabled={
                    !native ||
                    (!batch && models.filter((m) => m.enabled).length === 0) ||
                    (!batch && testing.size > 0)
                  }
                  onClick={() =>
                    batch
                      ? void cancelTests()
                      : setTestTargets(
                          models.filter((m) => m.enabled).map((m) => m.id),
                        )
                  }
                >
                  {batch ? "取消批量测试" : "一键测试"}
                </button>
              </>
            )}
            <button
              className="icon-button"
              title="刷新"
              aria-label="刷新"
              disabled={busy}
              onClick={() => void action(refresh, "已刷新配置")}
            >
              <Icon name="refresh" size={15} />
            </button>
            {page === "models" && (
              <button
                className="icon-button"
                data-add-model
                title="添加模型"
                aria-label="添加模型"
                disabled={!state}
                onClick={() => setEditor(null)}
              >
                <Icon name="plus" size={16} />
              </button>
            )}
          </div>
        </header>
        {!native && (
          <div className="preview-banner">
            <Icon name="info" size={15} /> 浏览器界面预览 ·
            配置仅保存在页面内存，不发送请求、不接管 DSH。
          </div>
        )}
        <main className={page === "overview" ? "overview-main" : undefined}>
          {error && (
            <div className="error-banner" role="alert">
              <span>{error}</span>
              <button aria-label="关闭错误" onClick={() => setError("")}>
                <Icon name="close" size={16} />
              </button>
            </div>
          )}
          {!state && !error && (
            <div className="loading">正在连接本机配置服务…</div>
          )}
          {state && page === "models" && (
            <>
              <div className="model-toolbar">
                <label className="search">
                  <Icon name="search" size={14} />
                  <input
                    aria-label="搜索模型"
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                    placeholder="搜索模型、备注或服务商"
                  />
                </label>
                <small>
                  {visible.length} / {models.length} 个模型
                </small>
              </div>
              {models.length === 0 ? (
                <section className="empty-models">
                  <Empty
                    title="添加你的第一个模型"
                    description="连接你选择的模型服务，把地址、Key 和请求头统一交给 DSH Patrick 管理。"
                  >
                    <button
                      className="primary"
                      data-add-model
                      onClick={() => setEditor(null)}
                    >
                      <Icon name="plus" size={17} /> 添加模型
                    </button>
                  </Empty>
                  <div className="setup-steps">
                    <div>
                      <i>01</i>
                      <b>配置上游</b>
                      <span>填写地址与 API Key</span>
                    </div>
                    <div>
                      <i>02</i>
                      <b>验证连接</b>
                      <span>发送短请求验证上游</span>
                    </div>
                    <div>
                      <i>03</i>
                      <b>连接 DSH</b>
                      <span>等待主任务结束后联调</span>
                    </div>
                  </div>
                </section>
              ) : visible.length === 0 ? (
                <Empty
                  icon="search"
                  title="没有匹配的模型"
                  description="试试其他关键词，或清空搜索条件。"
                />
              ) : (
                Object.entries(groups).map(([name, items]) => (
                  <section className="model-group" key={name}>
                    {name && (
                      <h3 className="group-title">
                        <Icon name="link" size={16} />
                        {name}
                        <span>{items.length}</span>
                      </h3>
                    )}
                    <div className="model-grid">
                      {items.map((m) => (
                        <article
                          className={
                            "model-card " + (!m.enabled ? "muted-card" : "")
                          }
                          key={m.id}
                          data-model-id={m.id}
                          data-model-url={m.baseUrl}
                        >
                          <div className="model-card-head">
                            <h3 title={m.name}>{m.name}</h3>
                            <span
                              className="protocol-badge"
                              title={protocols[m.protocol]}
                            >
                              <Icon
                                name={
                                  m.protocol === "anthropic"
                                    ? "anthropic"
                                    : "openai"
                                }
                                size={13}
                              />
                              {m.protocol === "anthropic"
                                ? "Anthropic"
                                : m.protocol === "responses"
                                  ? "Responses"
                                  : "OpenAI"}
                            </span>
                          </div>
                          <p
                            className="model-id"
                            title={`${m.model} · ${m.baseUrl} · ${m.contextWindow === null ? "默认上下文" : `${m.contextWindow} Token`} · ${m.hasKey ? "Key 已保存" : "未配置 Key"}`}
                          >
                            {m.model}
                          </p>
                          {m.notes && (
                            <p className="model-notes" title={m.notes}>
                              {m.notes}
                            </p>
                          )}
                          {!m.enabled && (
                            <small className="model-disabled">已停用</small>
                          )}
                          <TestStatus
                            result={state.runtime.testResults[m.id]}
                            testing={testing.has(m.id)}
                            onDetail={setDetail}
                          />
                          <div className="model-card-actions">
                            <button
                              data-test-model={m.id}
                              disabled={
                                !native || (batch && !testing.has(m.id))
                              }
                              onClick={() =>
                                testing.has(m.id)
                                  ? void api.cancel(m.id)
                                  : setTestTargets([m.id])
                              }
                            >
                              {testing.has(m.id) ? "取消" : "测试"}
                            </button>
                            <button
                              disabled={testing.has(m.id)}
                              aria-label={"编辑 " + m.name}
                              onClick={() => setEditor(m)}
                            >
                              编辑
                            </button>
                            <button
                              disabled={busy}
                              aria-label={"复制 " + m.name}
                              onClick={() =>
                                void action(
                                  () => api.duplicate(m.id),
                                  "已复制模型",
                                )
                              }
                            >
                              复制
                            </button>
                            <button
                              disabled={testing.has(m.id)}
                              className="danger-text"
                              aria-label={"删除 " + m.name}
                              onClick={() => setDeleting(m)}
                            >
                              删除
                            </button>
                          </div>
                        </article>
                      ))}
                    </div>
                  </section>
                ))
              )}
              <div className="page-note">
                <Icon name="shield" size={15} />{" "}
                保存不请求上游；网关运行时，启用模型自动同步到 DSH，原有默认模型保持不变。
              </div>
            </>
          )}
          {state && page === "overview" && <Overview version={state.runtime.historyVersion} historyError={state.runtime.historyError} preset={rangePreset} custom={customRange} />}
          {state && page === "calls" && (
            <Calls
              records={calls}
              onDetail={setDetail}
              onClear={() => setConfirmClear(true)}
              disabled={!native || state.runtime.activeRequests > 0}
            />
          )}
          {state && page === "connection" && <DshConnection snapshot={state} busy={busy} action={action} />}
          {state && page === "help" && (
            <section className="panel help-panel">
              <h2>DSH Patrick 使用教程</h2>
              <ol>
                <li>
                  <b>添加模型</b>
                  <p>
                    填写服务商地址和
                    Key；点击“获取模型”读取列表，也可以手动填写模型 ID。
                  </p>
                </li>
                <li>
                  <b>配置与测试</b>
                  <p>
                    自定义 Headers、额外 JSON
                    参数按需启用；注入提示词按模型独立启停，关闭并保存后保留文字但停止注入。“保存”只写本机；“保存并测试”会向该上游发送短请求。
                  </p>
                </li>
                <li>
                  <b>启动网关</b>
                  <p>
                    DSH 模型页打开网关时自动同步启用的模型；关闭时撤销 Patrick 的配置。关闭窗口只收起到托盘，网关继续运行。
                  </p>
                </li>
                <li>
                  <b>接入 DSH</b>
                  <p>
                    在“DSH 接入”核对 profile。DSH 启动或热更新后，在客户端选择 DSH Patrick 下的模型即可，无需填写地址和 Key。原默认模型不自动替换。
                  </p>
                </li>
              </ol>
              <p>
                模型列表读取使用上游 Key，本机 API
                使用独立令牌。网关调用记录不保存对话正文。
              </p>
            </section>
          )}
          {state && page === "settings" && (
            <SettingsPage
              snapshot={state}
              onSave={(settings) =>
                action(() => api.settings(settings), "设置已保存")
              }
            />
          )}
        </main>
        <footer className="statusbar">
          <span>
            <span className="status-dot" />
            {native ? "Rust · Tauri 桌面端" : "浏览器界面预览"}
          </span>
          <span>
            {state?.runtime.activeRequests ?? 0} 个请求进行中 <i /> {state?.dshConnected ? "DSH 配置已同步" : "DSH 未接管"}
          </span>
        </footer>
      </div>
      {notice && (
        <div className="toast" role="status">
          <Icon name="check" size={17} />
          {notice}
        </div>
      )}
      {detail && (
        <CallDetails
          record={calls.find((r) => r.id === detail.id) ?? detail}
          onClose={() => setDetail(null)}
        />
      )}
      {testTargets && (
        <Modal
          title={testTargets.length > 1 ? "批量测试模型？" : "测试模型连接？"}
          subtitle="测试会向你配置的上游发送一个短请求。"
          onClose={() => setTestTargets(null)}
        >
          <div className="confirm-body">
            <p>
              共 {testTargets.length} 个模型，最多同时测试 3
              个。请求可能计入服务商用量；不会修改 DSH 配置。
            </p>
            <ul className="test-targets">
              {testTargets.map((id) => {
                const m = models.find((m) => m.id === id)!;
                return (
                  <li key={id}>
                    <b>{m.name}</b>
                    <span>{new URL(m.baseUrl).host}</span>
                  </li>
                );
              })}
            </ul>
          </div>
          <footer className="modal-foot">
            <span>使用已保存的配置</span>
            <div>
              <button onClick={() => setTestTargets(null)}>取消</button>
              <button
                className="primary"
                data-confirm-test
                onClick={() => void testModels(testTargets)}
              >
                开始测试
              </button>
            </div>
          </footer>
        </Modal>
      )}
      {confirmClear && (
        <Modal
          title="清空本次调用记录？"
          subtitle="模型配置和凭据保持不变。"
          onClose={() => setConfirmClear(false)}
        >
          <div className="confirm-body">
            只清空当前会话的调用列表；各模型最近一次测试结果仍保留。
          </div>
          <footer className="modal-foot">
            <span />
            <div>
              <button onClick={() => setConfirmClear(false)}>取消</button>
              <button
                className="danger"
                onClick={() =>
                  void action(async () => {
                    await api.clearRecords();
                    setConfirmClear(false);
                  }, "调用列表已清空")
                }
              >
                确认清空
              </button>
            </div>
          </footer>
        </Modal>
      )}
      {editor !== undefined && (
        <ModelEditor
          model={editor}
          onTest={(id) => void testModels([id])}
          onClose={() => setEditor(undefined)}
          onSaved={async () => {
            await refresh();
            setNotice("模型配置已保存");
          }}
        />
      )}
      {deleting && (
        <Modal
          title="删除模型？"
          subtitle="仅删除网关内的这份配置，不修改 DSH。"
          onClose={() => {
            if (!busy) setDeleting(null);
          }}
        >
          <div className="confirm-body">
            确定删除 <strong>{deleting.name}</strong>？保存的 Key
            和模型专属配置也会一并移除。
          </div>
          <footer className="modal-foot">
            <span />
            <div>
              <button disabled={busy} onClick={() => setDeleting(null)}>
                取消
              </button>
              <button
                className="danger"
                disabled={busy}
                onClick={() =>
                  void action(async () => {
                    await api.remove(deleting.id);
                    setDeleting(null);
                  }, "模型已删除")
                }
              >
                确认删除
              </button>
            </div>
          </footer>
        </Modal>
      )}
    </div>
  );
}

function SettingsPage({
  snapshot,
  onSave,
}: {
  snapshot: Snapshot;
  onSave: (s: Settings) => Promise<void>;
}) {
  const [draft, setDraft] = useState(snapshot.settings),
    [saving, setSaving] = useState(false);
  useEffect(
    () => setDraft(snapshot.settings),
    [
      snapshot.settings.port,
      snapshot.settings.timeoutSeconds,
      snapshot.settings.maxBodyMb,
      snapshot.settings.logLimit,
      snapshot.settings.theme,
    ],
  );
  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        setSaving(true);
        try {
          await onSave(draft);
        } finally {
          setSaving(false);
        }
      }}
    >
      <section className="panel settings-panel">
        <div className="panel-title">
          <h3>网关设置</h3>
          <span>仅监听本机回环地址</span>
        </div>
        <div className="form-grid">
          <Field label="监听端口" hint="默认 8787；修改端口前请先关闭网关">
            <input
              type="number"
              min={1024}
              max={65535}
              required
              value={draft.port}
              onChange={(e) =>
                setDraft({ ...draft, port: Number(e.target.value) })
              }
            />
          </Field>
          <Field label="请求超时（秒）" hint="5–600 秒">
            <input
              type="number"
              min={5}
              max={600}
              required
              value={draft.timeoutSeconds}
              onChange={(e) =>
                setDraft({ ...draft, timeoutSeconds: Number(e.target.value) })
              }
            />
          </Field>
          <Field
            label="请求体上限（MiB）"
            hint="默认 64；1–128 MiB，适用于长上下文和图片输入"
          >
            <input
              name="maxBodyMb"
              type="number"
              min={1}
              max={128}
              required
              value={draft.maxBodyMb}
              onChange={(e) =>
                setDraft({ ...draft, maxBodyMb: Number(e.target.value) })
              }
            />
          </Field>
          <Field label="调用记录上限" hint="仅保留本次会话最近请求的元数据">
            <input
              type="number"
              min={10}
              max={1000}
              required
              value={draft.logLimit}
              onChange={(e) =>
                setDraft({ ...draft, logLimit: Number(e.target.value) })
              }
            />
          </Field>
          <Field label="界面主题">
            <Select
              label="界面主题"
              value={draft.theme}
              disabled={saving}
              onChange={(v) =>
                setDraft({ ...draft, theme: v as Settings["theme"] })
              }
              options={[
                { value: "light", label: "浅色" },
                { value: "dark", label: "深色" },
                { value: "system", label: "跟随系统" },
              ]}
            />
          </Field>
        </div>
        <footer className="settings-foot">
          <span>保存不会重启 DSH</span>
          <button className="primary" type="submit" disabled={saving}>
            {saving ? "保存中…" : "保存设置"}
          </button>
        </footer>
      </section>
      <section className="panel settings-panel">
        <div className="panel-title">
          <h3>本地数据与隐私</h3>
          <Icon name="shield" />
        </div>
        <div className="setting-line">
          <span>存储方式</span>
          <b>Windows 当前用户 DPAPI 加密</b>
        </div>
        <div className="setting-line">
          <span>后台上报 / 自动更新请求</span>
          <b>未启用</b>
        </div>
        <div className="setting-line directory">
          <span>数据目录</span>
          <code>{snapshot.dataDirectory}</code>
        </div>
        <p className="muted">
          整个配置文件均加密，包括 Key、请求头和提示词。复制到其他 Windows
          账户后需要重新配置凭据。
        </p>
      </section>
      <LocalToken />
      <section className="panel about-panel">
        <img
          className="app-logo"
          src={patrickIcon}
          alt="DSH Patrick 图标"
          width={44}
          height={44}
        />
        <div>
          <h3>DSH Patrick</h3>
          <p>Rust + Tauri 2 · React + TypeScript</p>
        </div>
        <span className="pill">v{snapshot.appVersion}</span>
      </section>
    </form>
  );
}

function LocalToken() {
  const [token, setToken] = useState(""),
    [error, setError] = useState("");
  return (
    <section className="panel settings-panel">
      <div className="panel-title">
        <h3>本机网关鉴权</h3>
        <span>与上游 API Key 分离</span>
      </div>
      <div className="token-body">
        <p>
          本机 API 使用独立令牌，自动接入时由软件写入 DSH 的专用凭据引用；真实上游 Key 只保存在 Patrick。
        </p>
        <div className="token-field">
          <input
            aria-label="本机网关令牌"
            readOnly
            type="text"
            value={token || "••••••••••••••••••••••••"}
            spellCheck={false}
          />
          <button
            type="button"
            disabled={!native}
            onClick={async () => {
              if (token) {
                setToken("");
                return;
              }
              try {
                setToken(await api.token());
              } catch (e) {
                setError(String(e));
              }
            }}
          >
            {token ? "隐藏" : "显示令牌"}
          </button>
        </div>
        {error && (
          <div role="alert" className="form-error">
            {error}
          </div>
        )}
        <small>
          仅用于 http://127.0.0.1 的网关鉴权，不会作为上游凭据转发。
        </small>
      </div>
    </section>
  );
}
