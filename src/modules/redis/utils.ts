/**
 * Pure helpers for the Redis page: connection persistence, TTL/uptime
 * formatting and rendering a typed value as text for the CodeBlock.
 */
import type { BadgeTone } from "../../ui";
import type { RedisConn, ValueView } from "./api";

export const STORAGE_KEY = "devspro.redis.conn";

export const DEFAULT_CONN: RedisConn = { host: "127.0.0.1", port: 6379, db: 0 };

/**
 * Restores host/port/db from a stored JSON blob. The password is deliberately
 * never persisted — it lives in memory for the session only — so it is
 * dropped even if an older blob happens to contain one.
 */
export function parseStoredConn(json: string | null): RedisConn {
  if (!json) return DEFAULT_CONN;
  try {
    const raw = JSON.parse(json) as Partial<Record<keyof RedisConn, unknown>>;
    const host = typeof raw.host === "string" && raw.host.trim() ? raw.host.trim() : DEFAULT_CONN.host;
    const port = parsePort(raw.port) ?? DEFAULT_CONN.port;
    const db = parseDb(raw.db) ?? DEFAULT_CONN.db;
    return { host, port, db };
  } catch {
    return DEFAULT_CONN;
  }
}

/** What goes into localStorage: everything except the password. */
export function serialiseConn(conn: RedisConn): string {
  return JSON.stringify({ host: conn.host, port: conn.port, db: conn.db });
}

export function parsePort(value: unknown): number | null {
  const n = typeof value === "string" ? Number(value.trim()) : value;
  if (typeof n !== "number" || !Number.isInteger(n) || n < 1 || n > 65535) return null;
  return n;
}

export function parseDb(value: unknown): number | null {
  const n = typeof value === "string" ? Number(value.trim()) : value;
  if (typeof n !== "number" || !Number.isInteger(n) || n < 0 || n > 15) return null;
  return n;
}

/** "127.0.0.1:6379 / db 0" for the header. */
export function describeConn(conn: RedisConn): string {
  return `${conn.host}:${conn.port} / db ${conn.db}`;
}

/** Redis TTL semantics: -1 has no expiry, -2 no longer exists. */
export function formatTtl(ttl: number): string {
  if (ttl === -1) return "no expiry";
  if (ttl < 0) return "gone";
  return formatDuration(ttl);
}

export function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (d > 0) return h > 0 ? `${d}d ${h}h` : `${d}d`;
  if (h > 0) return m > 0 ? `${h}h ${m}m` : `${h}h`;
  return `${m}m ${seconds % 60}s`;
}

export function typeTone(type: string): BadgeTone {
  switch (type) {
    case "string":
      return "info";
    case "list":
      return "accent";
    case "hash":
      return "success";
    case "set":
    case "zset":
      return "warning";
    default:
      return "neutral";
  }
}

/** A one-line summary of a value's size for the modal description. */
export function describeValue(value: ValueView): string {
  switch (value.type) {
    case "string":
      return `string, ${value.value.length} chars`;
    case "list":
      return `list, ${value.length} items${value.length > value.items.length ? ` (showing first ${value.items.length})` : ""}`;
    case "hash":
      return `hash, ${value.length} fields${value.length > value.fields.length ? ` (showing first ${value.fields.length})` : ""}`;
    case "set":
      return `set, ${value.length} members${value.length > value.members.length ? ` (showing ${value.members.length})` : ""}`;
    case "zset":
      return `sorted set, ${value.length} members${value.length > value.members.length ? ` (showing first ${value.members.length})` : ""}`;
    case "none":
      return "key no longer exists";
    default:
      return `${value.kind} (not shown)`;
  }
}

/** Strings that are JSON get pretty-printed; everything else is verbatim. */
export function prettyIfJson(text: string): string {
  const t = text.trim();
  if (!(t.startsWith("{") || t.startsWith("["))) return text;
  try {
    return JSON.stringify(JSON.parse(t), null, 2);
  } catch {
    return text;
  }
}

/** Renders a typed value as the text shown in the CodeBlock. */
export function formatValue(value: ValueView): string {
  switch (value.type) {
    case "string":
      return prettyIfJson(value.value);
    case "list":
      return value.items.map((item, i) => `${i}) ${item}`).join("\n");
    case "hash":
      return value.fields.map(([k, v]) => `${k} = ${v}`).join("\n");
    case "set":
      return value.members.join("\n");
    case "zset":
      return value.members.map((m) => `${m.score}\t${m.member}`).join("\n");
    case "none":
      return "";
    default:
      return `Values of type "${value.kind}" are not rendered here.`;
  }
}
