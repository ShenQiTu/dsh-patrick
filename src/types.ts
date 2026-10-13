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
  maxBodyMb: number;
  theme: "light" | "dark" | "system";
  logLimit: number;
}
export interface Snapshot {
  models: Model[];
  settings: Settings;
  dataDirectory: string;
  appVersion: string;
  testMode: boolean;
  gatewayRunning: boolean;
  gatewayAvailable: boolean;
  dshConnected: boolean;
  dsh: {profile:string; configured:boolean; modelCount:number; zenCount:number; message:string; backupDirectory:string};
  trayAvailable: boolean;
  runtime: {
    lastDshRequest: number;
    port: number;
    activeRequests: number;
    calls: CallRecord[];
    testResults: Record<string, CallRecord>;
    historyVersion: number;
    historyError: string | null;
  };
  testUpstream?: string | null;
  testPauseSeconds?: number;
}
export interface CallRecord {
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
