import type { TokenPricing } from "./types";
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
export function estimateCost(records: UsageRecord[], pricing: TokenPricing) {
  const configured = Object.values(pricing).some(p => p !== null);
  let covered = 0, amount = 0;
  for (const r of records) {
    if (r.inputTokens === null || r.outputTokens === null) continue;
    const read = Math.min(r.cacheReadTokens ?? 0, r.inputTokens);
    const write = Math.min(r.cacheWriteTokens ?? 0, Math.max(0, r.inputTokens - read));
    const categories: [number, number | null][] = [[Math.max(0, r.inputTokens - read - write), pricing.input], [r.outputTokens, pricing.output], [read, pricing.cacheRead], [write, pricing.cacheWrite]];
    if (categories.some(([tokens, price]) => tokens > 0 && (price === null || !Number.isFinite(price) || price < 0))) continue;
    covered++;
    amount += categories.reduce((n, [tokens, price]) => n + tokens * (price ?? 0) / 1000000, 0);
  }
  return { amount: configured && covered > 0 ? amount : null, covered: configured ? covered : 0, configured };
}
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
export function customRange(preset: Preset, custom: Range | null, now = Date.now()): Range {
  return preset === "custom" ? custom ?? presetRange("month", now) : presetRange(preset, now);
}
export function localInput(ms: number) {
  const date = new Date(ms);
  return new Date(ms - date.getTimezoneOffset() * 60000).toISOString().slice(0, 23);
}
export function parseLocalInput(value: string, original?: number) {
  const parsed = new Date(value).getTime();
  return original !== undefined && Number.isFinite(parsed) && localInput(parsed) === localInput(original) ? original : parsed;
}
export type Grain = "minute" | "hour" | "day" | "month" | "year";
export function timeBuckets(range: Range, preset: Preset = "custom") {
  const duration = range.end - range.start;
  const grain: Grain = preset === "week" || preset === "month" ? "day" : preset === "today" ? "hour"
    : duration <= 3600000 ? "minute" : duration <= day ? "hour" : duration <= 62 * day ? "day"
    : duration <= 5 * 366 * day ? "month" : "year";
  const buckets: { start: number; end: number; labelAt: number; input: number; output: number; read: number; write: number; calls: number }[] = [];
  if (!Number.isFinite(range.start) || !Number.isFinite(range.end) || duration <= 0) return { grain, buckets };
  const step = grain === "minute" ? (duration <= 600000 ? 1 : 5) : grain === "year"
    ? Math.max(1, Math.ceil((new Date(range.end).getFullYear() - new Date(range.start).getFullYear() + 1) / 60)) : 1;
  const date = new Date(range.start);
  if (grain === "minute") date.setTime(date.getTime() - (date.getMinutes() % step) * 60000 - date.getSeconds() * 1000 - date.getMilliseconds());
  else if (grain === "hour") {
    // Subtract the elapsed portion instead of setHours/setMinutes: a repeated DST
    // hour must keep its own offset rather than jump back to its first occurrence.
    date.setTime(date.getTime() - date.getMinutes() * 60000 - date.getSeconds() * 1000 - date.getMilliseconds());
  } else {
    date.setHours(0, 0, 0, 0);
    if (grain === "month" || grain === "year") date.setDate(1);
    if (grain === "year") date.setMonth(0);
  }
  while (date.getTime() < range.end) {
    const labelAt = date.getTime();
    if (grain === "minute" || grain === "hour") date.setTime(labelAt + step * (grain === "minute" ? 60000 : 3600000));
    else if (grain === "day") date.setDate(date.getDate() + 1);
    else if (grain === "month") date.setMonth(date.getMonth() + 1);
    else date.setFullYear(date.getFullYear() + step);
    const start = Math.max(labelAt, range.start), end = Math.min(date.getTime(), range.end);
    if (end > start) buckets.push({ start, end, labelAt, input: 0, output: 0, read: 0, write: 0, calls: 0 });
  }
  return { grain, buckets };
}
export function axisTicks(count: number) {
  return count ? [...new Set([0, 0.25, 0.5, 0.75, 1].map(v => Math.round((count - 1) * v)))] : [];
}
export function summarize(records: UsageRecord[], range: Range, source = "gateway", preset: Preset = "custom") {
  const selected = records.filter(r => r.startedAt >= range.start && r.startedAt < range.end && (source === "all" || r.source === source));
  const input = selected.reduce((n, r) => n + (r.inputTokens ?? 0), 0);
  const output = selected.reduce((n, r) => n + (r.outputTokens ?? 0), 0);
  const cacheSamples = selected.filter(r => r.inputTokens !== null && r.cacheReadTokens !== null);
  const cacheInput = cacheSamples.reduce((n, r) => n + r.inputTokens!, 0);
  const cacheRead = cacheSamples.reduce((n, r) => n + Math.min(r.inputTokens!, r.cacheReadTokens!), 0);
  const { grain, buckets } = timeBuckets(range, preset);
  for (const r of selected) {
    const b = buckets.find(b => r.startedAt >= b.start && r.startedAt < b.end);
    if (!b) continue;
    const read = Math.min(r.cacheReadTokens ?? 0, r.inputTokens ?? 0);
    const write = Math.min(r.cacheWriteTokens ?? 0, Math.max(0, (r.inputTokens ?? 0) - read));
    b.input += Math.max(0, (r.inputTokens ?? 0) - read - write);
    b.output += r.outputTokens ?? 0; b.read += read; b.write += write; b.calls++;
  }
  return { selected, input, output, buckets, grain, cacheSamples: cacheSamples.length,
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
export function calendarLayout(days: { date: string; count: number }[]) {
  if (!days.length) return { padding: 0, columns: 0, months: [] as { column: number; label: string }[] };
  const [year, month, date] = days[0].date.split("-").map(Number);
  const padding = new Date(year, month - 1, date).getDay(); // Sunday-first weeks.
  const columns = Math.ceil((padding + days.length) / 7);
  const months = days.flatMap((d, i) => i === 0 || d.date.slice(0, 7) !== days[i - 1].date.slice(0, 7)
    ? [{ column: Math.floor((padding + i) / 7), label: `${Number(d.date.slice(5, 7))}月` }] : []);
  // A short first partial month can share a column with the next month.
  if (months.length > 1 && months[1].column - months[0].column < 3) months.shift();
  return { padding, columns, months };
}
