import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  CallRecord,
  AutostartStatus, StorageInfo, TokenPricing, UpdateInfo,
  Model,
  ModelInput,
  Settings,
  Snapshot,
} from "./types";

export const native = isTauri();
// Browser preview is explicitly labelled and memory-only. It never calls an upstream or persists credentials.
let preview: Snapshot = {
  models: [],
  pricing: {input:null,output:null,cacheRead:null,cacheWrite:null},
  settings: {
    port: 8787,
    dshProfile: "",
    timeoutSeconds: 120,
    streamIdleSeconds: 600,
    streamGraceSeconds: 10,
    maxBodyMb: 64,
    bodyConcurrency: 16,
    upstreamConcurrency: 32,
    theme: "light",
    logLimit: 200,
    detailedLogging: false,
  },
  dataDirectory: "浏览器预览 · 数据只保留在当前页面内存",
  appVersion: "0.1.7",
  testMode: false,
  gatewayRunning: false,
  gatewayAvailable: false,
  dshConnected: false,
  dsh: {profile:"",configured:false,modelCount:0,zenCount:0,message:"浏览器预览不读写 DSH 文件",backupDirectory:""},
  trayAvailable: false,
  runtime: { lastDshRequest:0, port: 0, activeRequests: 0, calls: [], testResults: {}, historyVersion: 0, historyError: null },
};
const clone = <T>(x: T): T => structuredClone(x);
export const api = {
  savePricing: async (pricing: TokenPricing) => {
    if (native) return invoke<void>("save_pricing", { pricing });
    preview.pricing = clone(pricing);
  },
  autostartStatus: () => invoke<AutostartStatus>("autostart_status"),
  setAutostart: (enabled: boolean) => invoke<AutostartStatus>("set_autostart", { enabled }),
  checkUpdate: () => invoke<UpdateInfo>("check_update"),
  openReleasePage: () => invoke<void>("open_release_page"),
  statisticsStorage: () => native ? invoke<StorageInfo>("statistics_storage") : Promise.resolve({calls:0,bytes:0,historyError:null}),
  clearStatistics: () => invoke<void>("clear_statistics"),
  statistics: () => native ? invoke<import("./stats").UsageRecord[]>("statistics") : Promise.resolve([]),
  snapshot: () =>
    native ? invoke<Snapshot>("snapshot") : Promise.resolve(clone(preview)),
  save: async (input: ModelInput) => {
    // Views contain hasKey; never serialize read-only view fields into the strict Rust input.
    const {
      id,
      name,
      model,
      protocol,
      baseUrl,
      apiKey,
      headers,
      enabled,
      contextWindow,
      maxOutput,
      reasoning,
      prompt,
      promptEnabled,
      useFullUrl,
      notes,
      headersEnabled,
      zenTestEnabled,
      extraEnabled,
      extraParams,
      thinkingBudget,
    } = input;
    if (native)
      return invoke<string>("save_model", {
        input: {
          id,
          name,
          model,
          protocol,
          baseUrl,
          apiKey,
          headers,
          enabled,
          contextWindow,
          maxOutput,
          reasoning,
          prompt,
      promptEnabled,
          useFullUrl,
          notes,
          headersEnabled,
          zenTestEnabled,
          extraEnabled,
          extraParams,
          thinkingBudget,
        },
      });
    if (!input.name.trim() || !input.model.trim())
      throw new Error("请填写显示名称和模型 ID");
    const url = new URL(input.baseUrl);
    if (
      url.protocol !== "https:" &&
      !(
        url.protocol === "http:" &&
        ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname)
      )
    )
      throw new Error("上游使用 HTTPS，本机测试可用 HTTP");
    if (url.username || url.password || url.search || url.hash)
      throw new Error("地址中不要包含凭据、查询或片段");
    if (preview.models.some((m) => m.id !== input.id && m.name === input.name))
      throw new Error("显示名称已存在");
    const old = preview.models.find((m) => m.id === input.id);
    const savedId = input.id ?? crypto.randomUUID();
    const saved: Model = {
      ...input,
      id: savedId,
      apiKey: "",
      hasKey:
        input.apiKey === null
          ? (old?.hasKey ?? false)
          : input.apiKey.length > 0,
    };
    preview.models = old
      ? preview.models.map((m) => (m.id === savedId ? saved : m))
      : [...preview.models, saved];
    return savedId;
  },
  remove: async (id: string) => {
    if (native) return invoke<void>("delete_model", { id });
    preview.models = preview.models.filter((m) => m.id !== id);
  },
  duplicate: async (id: string) => {
    if (native) return invoke<string>("duplicate_model", { id });
    const model = preview.models.find((m) => m.id === id);
    if (!model) throw new Error("模型已不存在");
    const newId = crypto.randomUUID();
    let name = model.name + " 副本",
      n = 1;
    while (preview.models.some((m) => m.name === name))
      name = model.name + " 副本 " + ++n;
    preview.models.push({ ...clone(model), id: newId, name });
    return newId;
  },
  settings: async (settings: Settings) => {
    if (native) return invoke<void>("save_settings", { settings });
    preview.settings = clone(settings);
  },
  syncDsh: () => invoke<void>("sync_dsh"),
  checkDsh: () => invoke<void>("check_dsh"),
  start: () => invoke<number>("start_gateway"),
  discover: (input: ModelInput, modelsUrl: string) => {
    const { hasKey: _hasKey, ...clean } = input as ModelInput & {
      hasKey?: boolean;
    };
    return invoke<string[]>("discover_models", {
      input: clean,
      modelsUrl: modelsUrl || null,
    });
  },
  stop: () => invoke<void>("stop_gateway"),
  test: (id: string) => invoke<CallRecord>("test_model", { id }),
  cancel: (id: string) => invoke<void>("cancel_test", { id }),
  clearRecords: () => invoke<void>("clear_records"),
  callDetail: (id: string) => invoke<import("./types").CallDetail>("call_detail", { id }),
  clearCallDetails: () => invoke<void>("clear_call_details"),
  token: () => invoke<string>("gateway_token"),
};
