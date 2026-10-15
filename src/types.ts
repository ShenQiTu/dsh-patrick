export type Protocol = "openai" | "responses" | "anthropic";
export interface Model {
  id: string;
  name: string;
  model: string;
  protocol: Protocol;
  baseUrl: string;
  apiKey: string;
  hasKey: boolean;
  headers: Record<string, string>;
  enabled: boolean;
  contextWindow: number | null;
  maxOutput: number | null;
  reasoning: string;
  prompt: string;
  promptEnabled: boolean;
  useFullUrl: boolean;
  notes: string;
  headersEnabled: boolean;
  zenTestEnabled: boolean;
  extraEnabled: boolean;
  extraParams: Record<string, unknown>;
  thinkingBudget: number | null;
}
export type ModelInput = Omit<Model, "id" | "hasKey" | "apiKey"> & {
  id: string | null;
  apiKey: string | null;
};
export interface Settings {
  dshProfile: string;
  port: number;
  timeoutSeconds: number;
  streamIdleSeconds: number;
  streamGraceSeconds: number;
  maxBodyMb: number;
  bodyConcurrency: number;
  upstreamConcurrency: number;
  theme: "light" | "dark" | "system";
  logLimit: number;
  detailedLogging: boolean;
}
export interface TokenPricing { input: number | null; output: number | null; cacheRead: number | null; cacheWrite: number | null }
export interface AutostartStatus { enabled: boolean; currentExecutable: boolean }
export interface StorageInfo { calls: number; bytes: number; historyError: string | null }
export interface UpdateInfo { version: string; state: "available" | "current" | "ahead" }
export interface Snapshot {
  pricing: TokenPricing;
  models: Model[];
  settings: Settings;
  dataDirectory: string;
  appVersion: string;
  testMode: boolean;
  gatewayRunning: boolean;
  gatewayAvailable: boolean;
  dshConnected: boolean;
  dshRecoveryPending: boolean;
  closeRequest: {id:string; exit:boolean; message:string} | null;
  dsh: {profile:string; configured:boolean; modelCount:number; zenCount:number; message:string; backupDirectory:string;profileValid:boolean;checkedAt:number;syncState:string;checks:{id:string;label:string;state:"pass"|"fail"|"unchecked";detail:string}[]};
  trayAvailable: boolean;
  runtime: {
    lastDshRequest: number;
    port: number;
    activeRequests: number;
    calls: CallRecord[];
    testResults: Record<string, CallRecord>;
    historyVersion: number;
    historyError: string | null;
    callError?: string | null;
  };
  testUpstream?: string | null;
  testPauseSeconds?: number;
}
export interface CallInfo {
  model?: string | null; requestUrl?: string | null; reasoning?: string | null;
  finishReason?: string | null; ttfbMs?: number | null; ttfrMs?: number | null;
  reasoningTokens?: number | null; messageCount?: number | null; toolCount?: number | null;
  errorKind?: string | null; detailed?: boolean; legacy?: boolean;
}
export interface CallPayload { headers: Record<string,string>; body: unknown; response: string; responseChunks: number; requestTruncated: boolean; responseTruncated: boolean }
export interface CallDetail { record: CallRecord; payload: CallPayload | null }
export interface CallRecord {
  info?: CallInfo;
  id: string;
  modelId: string;
  name: string;
  protocol: Protocol;
  source: "test" | "gateway";
  host: string;
  startedAt: number;
  state: "running" | "success" | "error" | "cancelled";
  status: number;
  zenHeaderReceived: boolean | null;
  zenHeaderSent: boolean | null;
  zenSessionState?: string | null;
  durationMs: number;
  firstTokenMs: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
  tokensPerSecond: number | null;
  streamed: boolean;
  bytes: number;
  preview: string;
  message: string;
}
export type Page =
  | "overview"
  | "models"
  | "calls"
  | "connection"
  | "settings"
  | "help";
export const protocols: Record<Protocol, string> = {
  openai: "OpenAI Chat",
  responses: "OpenAI Responses",
  anthropic: "Anthropic",
};
export function emptyModel(): ModelInput {
  return {
    id: null,
    name: "",
    model: "",
    protocol: "openai",
    baseUrl: "",
    apiKey: null,
    headers: {},
    enabled: true,
    contextWindow: null,
    maxOutput: null,
    reasoning: "default",
    prompt: "",
    promptEnabled: false,
    useFullUrl: false,
    notes: "",
    headersEnabled: false,
    zenTestEnabled: false,
    extraEnabled: false,
    extraParams: {},
    thinkingBudget: null,
  };
}

// This is configuration verification, not whether the listener is running or a model answered.
export function dshConnectionLabel(s: Snapshot | null): string {
  if (!s) return "待核对";
  if (s.dshRecoveryPending) return "待恢复";
  if (!s.settings.dshProfile.trim()) return "未配置";
  if (s.dsh.profile !== s.settings.dshProfile || !s.dsh.checkedAt) return "待核对";
  if (!s.dsh.profileValid || s.dsh.syncState === "error" || s.dsh.checks.some(c=>c.state === "fail")) return "核验失败";
  if (s.dsh.configured) return "已同步";
  return s.dsh.syncState === "unchecked" ? "待核对" : "未同步";
}
