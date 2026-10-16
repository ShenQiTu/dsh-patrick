import { useRef, useState, type FormEvent } from "react";
import { api, native } from "./api";
import {
  emptyModel,
  type Model,
  type ModelInput,
  type Protocol,
} from "./types";
import { Field, Icon, Modal, Switch } from "./ui";
import { Help, ModelCombobox, Select } from "./Select";

const presets = [
  {
    name: "智谱 GLM",
    icon: "zhipu.svg",
    chat: "https://open.bigmodel.cn/api/coding/paas/v4/chat/completions",
    anthropic: "https://open.bigmodel.cn/api/anthropic",
    full: true,
  },
  {
    name: "Kimi",
    icon: "kimi.svg",
    chat: "https://api.kimi.com/coding",
    anthropic: "https://api.kimi.com/coding",
    full: false,
  },
  {
    name: "DeepSeek",
    icon: "deepseek.svg",
    chat: "https://api.deepseek.com",
    anthropic: "https://api.deepseek.com/anthropic",
    full: false,
  },
  {
    name: "火山方舟",
    icon: "huoshan.png",
    chat: "https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions",
    anthropic: "https://ark.cn-beijing.volces.com/api/coding",
    full: true,
  },
  {
    name: "MiniMax",
    icon: "minimax.svg",
    chat: "https://api.minimaxi.com",
    anthropic: "https://api.minimaxi.com/anthropic",
    full: false,
  },
];
const MAX_PROMPT_BYTES = 128 * 1024;
const assetUrls = import.meta.glob<string>("./assets/*", {
  eager: true,
  query: "?url",
  import: "default",
});
function jsonObject(text: string, label: string): Record<string, unknown> {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error(label + "格式错误，请填写有效的 JSON 对象");
  }
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error(label + "必须是 JSON 对象");
  return value as Record<string, unknown>;
}
export function ModelEditor({
  model,
  inheritedVision,
  onClose,
  onSaved,
  onTest,
}: {
  model: Model | null;
  inheritedVision?: boolean;
  onClose: () => void;
  onSaved: () => Promise<void>;
  onTest: (id: string) => void;
}) {
  const [draft, setDraft] = useState<ModelInput>(
    model ? { ...model, compactionPercent: model.compactionPercent ?? 85, apiKey: null } : emptyModel(),
  );
  const [headers, setHeaders] = useState(
    JSON.stringify(model?.headers ?? {}, null, 2),
  );
  const [extra, setExtra] = useState(
    JSON.stringify(model?.extraParams ?? {}, null, 2),
  );
  const [keyVisible, setKeyVisible] = useState(false),
    [error, setError] = useState("");
  const [saving, setSaving] = useState(false),
    [discovering, setDiscovering] = useState(false);
  const [options, setOptions] = useState<string[]>([]),
    [openSignal, setOpenSignal] = useState(0);
  const [modelsUrl, setModelsUrl] = useState("");
  const [discoveryNote, setDiscoveryNote] = useState("");
  const modelInput = useRef<HTMLInputElement>(null);
  const busy = saving || discovering;
  const promptBytes = new TextEncoder().encode(draft.prompt).length;
  function patch(value: Partial<ModelInput>) {
    setDraft((d) => ({ ...d, ...value }));
    setError("");
  }
  function changeSource(value: Partial<ModelInput>) {
    patch(value);
    setOptions([]);
    setDiscoveryNote("");
  }
  function prepared(): ModelInput {
    if (draft.compactionPercent===null || !Number.isFinite(draft.compactionPercent) || draft.compactionPercent<=0 || draft.compactionPercent>100) throw new Error("自动压缩阈值需大于 0 且不超过 100%");
    if (promptBytes > MAX_PROMPT_BYTES)
      throw new Error(`注入提示词过长：当前 ${promptBytes} 字节，上限 131072 字节（128 KiB，按 UTF-8 计算）`);
    let parsed: Record<string, unknown> = draft.headers;
    try {
      const candidate = jsonObject(headers, "自定义 Headers");
      if (Object.values(candidate).some((v) => typeof v !== "string"))
        throw new Error("自定义 Headers 的值必须是字符串");
      parsed = candidate;
    } catch (e) {
      if (draft.headersEnabled) throw e;
    }
    let extraParams = draft.extraParams;
    try {
      extraParams = jsonObject(extra, "额外参数");
    } catch (e) {
      if (draft.extraEnabled) throw e;
    }
    if (
      model?.hasKey &&
      draft.apiKey === null &&
      new URL(draft.baseUrl).origin !== new URL(model.baseUrl).origin
    )
      throw new Error("更换上游服务商后，请重新填写或明确清除 Key");
    return {
      ...draft,
      headers: parsed as Record<string, string>,
      extraParams,
    };
  }
  function reportError(e: unknown) {
    const message = String(e instanceof Error ? e.message : e);
    setError(message);
    const selector = message.includes("Headers")
      ? '[aria-label="自定义 Headers JSON"]'
      : message.includes("额外参数")
        ? '[aria-label="额外参数 JSON"]'
        : message.includes("提示词")
          ? '[aria-label="注入提示词"]'
          : message.includes("Key")
            ? 'input[name="apiKey"]'
            : message.includes("地址") || message.includes("URL")
              ? 'input[name="baseUrl"]'
              : null;
    if (selector)
      requestAnimationFrame(() =>
        document.querySelector<HTMLElement>(selector)?.focus(),
      );
  }
  async function discover() {
    setError("");
    setDiscovering(true);
    setDiscoveryNote("");
    try {
      const ids = await api.discover(prepared(), modelsUrl);
      setOptions(ids);
      setOpenSignal((n) => n + 1);
      setDiscoveryNote(`已获取 ${ids.length} 个模型，可选择或手动输入`);
    } catch (e) {
      reportError(e);
    } finally {
      setDiscovering(false);
    }
  }
  async function save(test: boolean) {
    setError("");
    try {
      const input = prepared();
      if (!input.name.trim() || !input.model.trim() || !input.baseUrl.trim())
        throw new Error("请填写服务器地址、模型名称和显示名称");
      setSaving(true);
      const id = await api.save(input);
      await onSaved();
      onClose();
      if (test) onTest(id);
    } catch (e) {
      reportError(e);
    } finally {
      setSaving(false);
    }
  }
  function selectModel(id: string) {
    patch({
      model: id,
      name: !draft.name || draft.name === draft.model ? id : draft.name,
    });
  }
  function preset(p: (typeof presets)[number], type = draft.protocol) {
    const baseUrl = type === "anthropic" ? p.anthropic : p.chat;
    let sameOrigin = false;
    try {
      sameOrigin = new URL(draft.baseUrl).origin === new URL(baseUrl).origin;
    } catch {}
    changeSource({
      baseUrl,
      useFullUrl: type !== "anthropic" && p.full,
      protocol:
        type === "anthropic"
          ? "anthropic"
          : p.name === "MiniMax"
            ? "responses"
            : "openai",
      apiKey: sameOrigin ? draft.apiKey : "",
      headersEnabled: sameOrigin && draft.headersEnabled,
      notes: draft.notes || p.name,
      reasoning:
        type === "anthropic" && draft.reasoning === "xhigh"
          ? "default"
          : draft.reasoning,
      thinkingBudget: type === "anthropic" ? draft.thinkingBudget : null,
    });
    if (!sameOrigin) {
      setHeaders("{}");
      setExtra("{}");
      patch({ extraEnabled: false, headers: {}, extraParams: {} });
    }
    setModelsUrl("");
  }
  function setType(type: Protocol) {
    const current = draft.baseUrl.replace(/\/+$/, "");
    const matched = presets.find((p) =>
      [p.chat, p.anthropic].includes(current),
    );
    if (matched) preset(matched, type);
    else
      changeSource({
        protocol: type,
        reasoning: "default",
        thinkingBudget: null,
      });
  }
  const endpoint =
    draft.protocol === "anthropic"
      ? "messages"
      : draft.protocol === "responses"
        ? "responses"
        : "chat/completions";
  const base = draft.baseUrl.replace(/\/+$/, "");
  const actualUrl = draft.useFullUrl
    ? draft.baseUrl
    : `${base}${base.endsWith("/v1") ? "" : "/v1"}/${endpoint}`;
  return (
    <Modal
      title={model ? "编辑模型" : "添加模型"}
      onClose={() => {
        if (!busy) onClose();
      }}
      wide
    >
      <form
        className="compact-editor"
        autoComplete="off"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          void save(false);
        }}
      >
        {error && (
          <div className="form-error editor-error" role="alert">
            {error}
          </div>
        )}
        <div className="editor-scroll">
          <fieldset disabled={busy} className="editor-body">
            <div className="preset-row">
              <small>
                常用预设{" "}
                <Help text="参考源码中的服务商地址模板；选择只填表，更换服务商会清空 Key 和自定义头。实际支持以服务商为准。" />
              </small>
              <div>
                {presets.map((p) => (
                  <button
                    type="button"
                    key={p.name}
                    className={
                      base ===
                      (draft.protocol === "anthropic" ? p.anthropic : p.chat)
                        ? "active"
                        : undefined
                    }
                    aria-pressed={
                      base ===
                      (draft.protocol === "anthropic" ? p.anthropic : p.chat)
                    }
                    onClick={() => preset(p)}
                  >
                    <img
                      className={
                        p.icon === "kimi.svg" ? "preset-monochrome" : undefined
                      }
                      src={assetUrls[`./assets/${p.icon}`]}
                      alt=""
                    />
                    {p.name}
                  </button>
                ))}
              </div>
            </div>
            <div className="form-grid">
              <Field label="模型类型">
                <Select
                  label="模型类型"
                  value={
                    draft.protocol === "anthropic" ? "anthropic" : "openai"
                  }
                  disabled={busy}
                  onChange={(v) => setType(v as Protocol)}
                  options={[
                    { value: "openai", label: "OpenAI", icon: "openai" },
                    {
                      value: "anthropic",
                      label: "Anthropic",
                      icon: "anthropic",
                    },
                  ]}
                />
              </Field>
              <Field
                label="请求协议"
                hint="决定请求格式；完整 URL 模式不自动改写地址。"
              >
                <Select
                  label="请求协议"
                  value={draft.protocol}
                  disabled={busy}
                  onChange={(v) => changeSource({ protocol: v as Protocol })}
                  options={
                    draft.protocol === "anthropic"
                      ? [{ value: "anthropic", label: "Messages API" }]
                      : [
                          { value: "openai", label: "Chat Completions API" },
                          { value: "responses", label: "Responses API" },
                        ]
                  }
                />
              </Field>
              <Field
                label={draft.useFullUrl ? "完整请求 URL" : "服务器地址"}
                hint="HTTPS 服务商地址；本机 mock 可用 HTTP。更换服务商需重新填写 Key。"
              >
                <input
                  name="baseUrl"
                  type="url"
                  required
                  value={draft.baseUrl}
                  placeholder="https://api.openai.com"
                  onChange={(e) => changeSource({ baseUrl: e.target.value })}
                />
              </Field>
              <Field
                label="API Key"
                hint={
                  model?.hasKey
                    ? "已加密保存；留空保留原 Key，不回显原值。"
                    : "只在本机加密保存；获取模型和测试时发送给所填上游。"
                }
              >
                <span className="secret-input">
                  <input
                    name="apiKey"
                    type={keyVisible ? "text" : "password"}
                    value={draft.apiKey ?? ""}
                    placeholder={
                      model?.hasKey ? "已保存 · 留空不修改" : "输入 API Key"
                    }
                    onChange={(e) => patch({ apiKey: e.target.value || null })}
                    autoComplete="new-password"
                    spellCheck={false}
                  />
                  <button
                    type="button"
                    aria-label={keyVisible ? "隐藏新 Key" : "显示新 Key"}
                    onClick={() => setKeyVisible((v) => !v)}
                  >
                    <Icon name={keyVisible ? "eye-off" : "eye"} size={14} />
                  </button>
                </span>
              </Field>
              <div className="full inline-checks">
                <label>
                  <input
                    type="checkbox"
                    checked={draft.useFullUrl}
                    onChange={(e) =>
                      changeSource({ useFullUrl: e.target.checked })
                    }
                  />
                  使用完整请求地址
                </label>
                {model?.hasKey && (
                  <button
                    type="button"
                    className="text-button danger-text"
                    onClick={() => patch({ apiKey: "" })}
                  >
                    {draft.apiKey === ""
                      ? "保存后将清除 Key"
                      : "清除已保存 Key"}
                  </button>
                )}
              </div>
              <div className="full url-preview" title={actualUrl}>
                实际请求：{base ? actualUrl : "请先填写服务器地址"}
              </div>
              {draft.useFullUrl && (
                <Field
                  full
                  label="模型列表 URL（可选）"
                  hint="标准端点自动推导；自定义路径时填写同源模型列表地址。"
                >
                  <input
                    aria-label="模型列表 URL"
                    type="url"
                    placeholder="https://同一服务商/v1/models"
                    value={modelsUrl}
                    onChange={(e) => setModelsUrl(e.target.value)}
                  />
                </Field>
              )}
              <Field
                label="模型名称"
                hint="可以获取模型列表，也可以手动输入准确的模型 ID。"
              >
                <ModelCombobox
                  inputRef={modelInput}
                  value={draft.model}
                  options={options}
                  openSignal={openSignal}
                  busy={busy}
                  onChange={(value) =>
                    patch({
                      model: value,
                      name:
                        !draft.name || draft.name === draft.model
                          ? value
                          : draft.name,
                    })
                  }
                  onSelect={selectModel}
                >
                  <button
                    type="button"
                    data-discover-models
                    disabled={!native || !draft.baseUrl.trim()}
                    onClick={() => void discover()}
                  >
                    {discovering ? "获取中…" : "获取模型"}
                  </button>
                </ModelCombobox>
              </Field>
              <Field
                label="显示名称"
                hint="仅用于网关展示，不修改发送给上游的模型 ID。"
              >
                <input
                  name="name"
                  required
                  maxLength={120}
                  value={draft.name}
                  onChange={(e) => patch({ name: e.target.value })}
                />
              </Field>
              {discoveryNote && (
                <small className="full discovery-note" role="status">
                  {discoveryNote}
                </small>
              )}
              <Field full label="备注" hint="仅用于卡片说明，不发送给上游。">
                <input
                  name="notes"
                  maxLength={2048}
                  value={draft.notes}
                  placeholder="可选：用途、服务商或模型说明"
                  onChange={(e) => patch({ notes: e.target.value })}
                />
              </Field>
              <Field
                label="上下文窗口 Token"
                hint="可选，1024–4000000；作为 DSH 的模型容量声明，不改变模型真实容量。留空沿用匹配的已有声明或 DSH 默认值。"
              >
                <input
                  name="contextWindow"
                  type="number"
                  min={1024}
                  max={4000000}
                  placeholder="留空使用默认值"
                  value={draft.contextWindow ?? ""}
                  onChange={(e) =>
                    patch({
                      contextWindow:
                        e.target.value === "" ? null : Number(e.target.value),
                    })
                  }
                />
              </Field>
              <Field
                label="最大输出 Token"
                hint="可选；优先保留客户端值。留空不为 OpenAI 请求追加上限；Anthropic 请求也未提供时使用 8192。"
              >
                <input
                  name="maxOutput"
                  type="number"
                  min={1}
                  max={draft.contextWindow ?? 4000000}
                  placeholder="留空使用默认值"
                  value={draft.maxOutput ?? ""}
                  onChange={(e) =>
                    patch({
                      maxOutput:
                        e.target.value === "" ? null : Number(e.target.value),
                    })
                  }
                />
              </Field>
              <Field
                label="推理强度"
                hint="不设置时保留 DSH 发来的推理参数；选择其他值会覆盖它。额外参数 JSON 或手填思考预算也可能覆盖；实际生效取决于上游支持。"
              >
                <Select
                  label="推理强度"
                  value={draft.reasoning}
                  disabled={busy}
                  onChange={(v) => patch({ reasoning: v })}
                  options={[
                    "default",
                    "none",
                    "low",
                    "medium",
                    "high",
                    "xhigh",
                    "max",
                  ]
                    .filter(
                      (v) =>
                        draft.protocol !== "anthropic" ||
                        v !== "xhigh" ||
                        draft.reasoning === "xhigh",
                    )
                    .map((v) => ({
                      value: v,
                      label: v === "default" ? "不设置" : v,
                      disabled: draft.protocol === "anthropic" && v === "xhigh",
                    }))}
                />
              </Field>
              <Field label="自动压缩阈值（%）">
                <input name="compactionPercent" type="number" min={0} max={100} step="any" required
                  value={draft.compactionPercent ?? ""}
                  onChange={e => patch({compactionPercent: e.target.value === "" ? null : Number(e.target.value)})} />
              </Field>
              {draft.protocol === "anthropic" && (
                <Field
                  label="思考预算 Token"
                  hint="留空时由思考强度决定；不设置强度时保留客户端值。手动预算至少 1024 且小于最大输出。"
                >
                  <input
                    name="thinkingBudget"
                    type="number"
                    min={1024}
                    max={(draft.maxOutput ?? 8192) - 1}
                    placeholder="留空由思考强度决定"
                    value={draft.thinkingBudget ?? ""}
                    onChange={(e) =>
                      patch({
                        thinkingBudget: e.target.value
                          ? Number(e.target.value)
                          : null,
                      })
                    }
                  />
                </Field>
              )}
              <div className="full toggle-json">
                <div><span>多模态（识图） <Help text="声明模型支持图片输入，接入时同步到 DSH。" /></span>
                <Switch label="多模态（识图）" checked={draft.multimodal ?? inheritedVision ?? false}
                  disabled={busy} onChange={() => patch({multimodal: !(draft.multimodal ?? inheritedVision ?? false)})} />
                </div>
              </div>
              <div className="full toggle-json">
                <div>
                  <span>
                    Zen 请求头（仅测试）{" "}
                    <Help text="仅用于测试、保存并测试和批量测试；每次生成独立会话，并以 x-opencode-session 与 x-opencode-session-id 一致发送。正常 DSH 对话继续沿用 DSH 插件的会话头，获取模型不受此开关影响。" />
                  </span>
                  <Switch
                    label="Zen 请求头（仅测试）"
                    checked={draft.zenTestEnabled}
                    onChange={() =>
                      patch({ zenTestEnabled: !draft.zenTestEnabled })
                    }
                  />
                </div>
              </div>
              <div className="full toggle-json">
                <div>
                  <span>自定义 Headers</span>
                  <Switch
                    label="自定义 Headers"
                    checked={draft.headersEnabled}
                    onChange={() =>
                      patch({ headersEnabled: !draft.headersEnabled })
                    }
                  />
                </div>
                {draft.headersEnabled && (
                  <textarea
                    className="code-input"
                    aria-label="自定义 Headers JSON"
                    rows={5}
                    value={headers}
                    onChange={(e) => {
                      setHeaders(e.target.value);
                      setError("");
                    }}
                    spellCheck={false}
                  />
                )}
              </div>
              <div className="full toggle-json">
                <div>
                  <span>
                    {draft.protocol === "anthropic" ? "Anthropic" : "OpenAI"}{" "}
                    额外参数{" "}
                    <Help text="顶层 JSON 参数覆盖同名客户端参数；模型、消息、地址、鉴权及流式开关受保护。" />
                  </span>
                  <Switch
                    label="额外参数"
                    checked={draft.extraEnabled}
                    onChange={() =>
                      patch({ extraEnabled: !draft.extraEnabled })
                    }
                  />
                </div>
                {draft.extraEnabled && (
                  <textarea
                    className="code-input"
                    aria-label="额外参数 JSON"
                    rows={5}
                    value={extra}
                    onChange={(e) => {
                      setExtra(e.target.value);
                      setError("");
                    }}
                    spellCheck={false}
                  />
                )}
              </div>
              <div className="full toggle-json">
                <div>
                  <span>注入提示词</span>
                  <Switch
                    label="启用注入提示词"
                    checked={draft.promptEnabled}
                    onChange={() => patch({ promptEnabled: !draft.promptEnabled })}
                  />
                </div>
                <small className="prompt-size" data-prompt-size aria-live="polite">
                  {promptBytes} / {MAX_PROMPT_BYTES} 字节（UTF-8，上限 128 KiB）
                </small>
                {draft.promptEnabled && (
                  <textarea
                    aria-label="注入提示词"
                    rows={5}
                    aria-invalid={promptBytes > MAX_PROMPT_BYTES}
                    value={draft.prompt}
                    placeholder="仅对此模型生效；关闭开关并保存后停止注入，已填写内容保留。"
                    onChange={(e) => patch({ prompt: e.target.value })}
                  />
                )}
              </div>
              <div className="full toggle-json">
                <div>
                  <span>启用模型</span>
                  <Switch
                    label="启用模型"
                    checked={draft.enabled}
                    onChange={() => patch({ enabled: !draft.enabled })}
                  />
                </div>
              </div>
            </div>
            <p className="editor-network-note">
              “获取模型”和“保存并测试”会访问当前填写的上游；“保存”只写入本机。关闭的
              JSON 选项不发送，格式错误的未启用草稿不保存。
            </p>
          </fieldset>
        </div>
        <footer className="modal-foot">
          <span>
            {model?.hasKey && draft.apiKey === null
              ? "已保存的 Key 保持不变"
              : "凭据加密保存"}
          </span>
          <div>
            <button type="button" disabled={busy} onClick={onClose}>
              取消
            </button>
            <button
              type="button"
              data-save-test
              disabled={busy || !native}
              onClick={() => void save(true)}
            >
              保存并测试
            </button>
            <button className="primary" type="submit" disabled={busy}>
              {saving ? "保存中…" : "保存"}
            </button>
          </div>
        </footer>
      </form>
    </Modal>
  );
}
