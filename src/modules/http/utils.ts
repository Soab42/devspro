import type { StatusVariant } from "../../ui";
import type { HeaderPair } from "./api";

export const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] as const;
export type Method = (typeof METHODS)[number];

/** Methods where a body is meaningful; the editor is disabled for the rest. */
export const BODY_METHODS: ReadonlySet<string> = new Set(["POST", "PUT", "PATCH", "DELETE"]);

/**
 * A header row in the editor. Rows carry an id so React keys survive edits —
 * keying by index re-mounts every input below a removed row and drops focus.
 */
export interface HeaderRow {
  id: string;
  key: string;
  value: string;
}

let nextRowId = 0;
export function newHeaderRow(key = "", value = ""): HeaderRow {
  nextRowId += 1;
  return { id: `h${nextRowId}`, key, value };
}

/** Editor rows -> wire pairs. Blank keys are the "add another" affordance, not data. */
export function rowsToPairs(rows: HeaderRow[]): HeaderPair[] {
  return rows
    .map((r): HeaderPair => [r.key.trim(), r.value.trim()])
    .filter(([k]) => k !== "");
}

/** Wire pairs -> editor rows, always with one trailing blank row to type into. */
export function pairsToRows(pairs: HeaderPair[]): HeaderRow[] {
  return [...pairs.map(([k, v]) => newHeaderRow(k, v)), newHeaderRow()];
}

/** 2xx ok, 3xx info, 4xx warn, 5xx bad; anything else (1xx, 0) neutral. */
export function statusVariant(status: number): StatusVariant {
  if (status >= 200 && status < 300) return "ok";
  if (status >= 300 && status < 400) return "info";
  if (status >= 400 && status < 500) return "warn";
  if (status >= 500 && status < 600) return "bad";
  return "neutral";
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} kB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

export function formatElapsed(ms: number): string {
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(2)} s`;
}

/** Case-insensitive header lookup; HTTP header names are. */
export function findHeader(headers: HeaderPair[], name: string): string | null {
  const wanted = name.toLowerCase();
  const hit = headers.find(([k]) => k.toLowerCase() === wanted);
  return hit ? hit[1] : null;
}

/** Mirrors the Rust `is_json_content_type` so both sides agree on what to pretty-print. */
export function isJsonContentType(contentType: string | null): boolean {
  if (!contentType) return false;
  const mime = (contentType.split(";")[0] ?? "").trim().toLowerCase();
  return mime === "application/json" || mime === "text/json" || mime.endsWith("+json");
}

/** Pretty-printed JSON, or null when the text does not parse. */
export function prettyJson(text: string): string | null {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return null;
  }
}

/**
 * The body to show: pretty JSON when the server says it is JSON, otherwise
 * the raw text. A JSON content-type with an unparsable body (a truncated
 * one, say) falls back to raw rather than showing nothing.
 */
export function displayBody(body: string, contentType: string | null): string {
  if (!isJsonContentType(contentType)) return body;
  return prettyJson(body) ?? body;
}

/** The quick-pick fills the URL with the site's HTTPS root. */
export function siteUrl(hostname: string): string {
  return `https://${hostname}/`;
}

/** Local wall-clock time for the history list; the date only when it isn't today. */
export function formatTime(iso: string, now: Date = new Date()): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return sameDay ? time : `${d.toLocaleDateString()} ${time}`;
}
