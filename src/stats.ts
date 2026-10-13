export interface UsageRecord {
  id: string;
  startedAt: number;
  source: "gateway" | "test";
  state: string;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
}
export type Range = { start: number; end: number };
export type Preset = "hour" | "today" | "ten" | "week" | "month" | "custom";
export const day = 86400000;
export function presetRange(preset: Exclude<Preset, "custom">, now = Date.now()): Range {
  const date = new Date(now);
  if (preset === "today") { date.setHours(0, 0, 0, 0); return { start: date.getTime(), end: now }; }
  if (preset === "month") {
    const d = date.getDate(); date.setDate(1); date.setMonth(date.getMonth() - 1);
    date.setDate(Math.min(d, new Date(date.getFullYear(), date.getMonth() + 1, 0).getDate()));
    return { start: date.getTime(), end: now };
  }
  return { start: now - (preset === "hour" ? 3600000 : preset === "ten" ? 600000 : 7 * day), end: now };
}
export function summarize(records: UsageRecord[], range: Range, source = "gateway") {
  const selected = records.filter(r => r.startedAt >= range.start && r.startedAt < range.end && (source === "all" || r.source === source));
  const input = selected.reduce((n, r) => n + (r.inputTokens ?? 0), 0);
  const output = selected.reduce((n, r) => n + (r.outputTokens ?? 0), 0);
  const cacheSamples = selected.filter(r => r.inputTokens !== null && r.cacheReadTokens !== null);
  const cacheInput = cacheSamples.reduce((n, r) => n + r.inputTokens!, 0);
  const cacheRead = cacheSamples.reduce((n, r) => n + Math.min(r.inputTokens!, r.cacheReadTokens!), 0);
  const count = range.end - range.start <= day ? 24 : 30;
  const buckets = Array.from({ length: count }, (_, i) => ({ start: range.start + (range.end - range.start) * i / count, input: 0, output: 0, read: 0, write: 0, calls: 0 }));
  for (const r of selected) {
    const index = Math.min(count - 1, Math.max(0, Math.floor((r.startedAt - range.start) / Math.max(1, range.end - range.start) * count)));
    const b = buckets[index];
    const read = Math.min(r.cacheReadTokens ?? 0, r.inputTokens ?? 0);
    const write = Math.min(r.cacheWriteTokens ?? 0, Math.max(0, (r.inputTokens ?? 0) - read));
    b.input += Math.max(0, (r.inputTokens ?? 0) - read - write);
    b.output += r.outputTokens ?? 0; b.read += read; b.write += write; b.calls++;
  }
  return { selected, input, output, buckets, cacheSamples: cacheSamples.length,
    cacheRate: cacheInput > 0 ? cacheRead / cacheInput : null,
    reported: selected.filter(r => r.inputTokens !== null || r.outputTokens !== null).length,
    success: selected.filter(r => r.state === "success").length,
    failed: selected.filter(r => r.state === "error").length,
    cancelled: selected.filter(r => r.state === "cancelled").length };
}
export function localDate(ms: number) {
  const d = new Date(ms); return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,"0")}-${String(d.getDate()).padStart(2,"0")}`;
}
export function calendar(records: UsageRecord[], end: number, source: string) {
  const last = new Date(end - 1); last.setHours(0, 0, 0, 0);
  const first = new Date(last); first.setDate(first.getDate() - 364);
  const counts = new Map<string, number>();
  for (const r of records) if (r.startedAt >= first.getTime() && r.startedAt < end && (source === "all" || r.source === source)) {
    const key = localDate(r.startedAt); counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return Array.from({ length: 365 }, (_, i) => {
    const d = new Date(first); d.setDate(d.getDate() + i);
    return { date: localDate(d.getTime()), count: counts.get(localDate(d.getTime())) ?? 0 };
  });
}
