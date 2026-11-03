import type { BadgeTone } from "../../ui";
import type { Observation, Source } from "./api";

export const SOURCE_LABEL: Record<Source, string> = {
  langfuse: "Langfuse",
  langsmith: "LangSmith",
  otel: "OpenTelemetry",
};

/** "820 µs", "312 ms", "4.21 s", "2m 05s". Input in microseconds. */
export function formatDuration(us: number | null | undefined): string {
  if (us === null || us === undefined || !Number.isFinite(us) || us < 0) return "-";
  if (us < 1000) return `${Math.round(us)} µs`;
  const ms = us / 1000;
  if (ms < 1000) return `${Math.round(ms)} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(s < 10 ? 2 : 1)} s`;
  const m = Math.floor(s / 60);
  return `${m}m ${String(Math.round(s - m * 60)).padStart(2, "0")}s`;
}

export function spanUs(start: number | null | undefined, end: number | null | undefined): number | null {
  if (start === null || start === undefined || end === null || end === undefined) return null;
  return Math.max(0, end - start);
}

/** 980, 12.3k, 4.1M. */
export function formatTokens(n: number | null | undefined): string {
  if (n === null || n === undefined) return "-";
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

/** Enough decimals to show a fraction of a cent: $0.000123, $0.42, $12.50. */
export function formatCost(cost: number | null | undefined): string {
  if (cost === null || cost === undefined) return "-";
  if (cost === 0) return "$0";
  if (cost >= 1) return `$${cost.toFixed(2)}`;
  // Three significant digits, at least two decimals, no trailing zeros past those.
  const rounded = Number(cost.toPrecision(3));
  const digits = Math.min(8, Math.max(2, -Math.floor(Math.log10(rounded)) + 2));
  return `$${rounded.toFixed(digits).replace(/(\.\d{2,}?)0+$/, "$1")}`;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** "14:03:22" today, "3 Nov 14:03" before that. */
export function formatClock(us: number, now: Date = new Date()): string {
  const date = new Date(us / 1000);
  const time = date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  if (date.toDateString() === now.toDateString()) return time;
  const day = date.toLocaleDateString(undefined, { day: "numeric", month: "short" });
  return `${day} ${date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}`;
}

export function kindTone(kind: string): BadgeTone {
  switch (kind) {
    case "generation":
      return "accent";
    case "embedding":
    case "retriever":
    case "agent":
      return "info";
    case "tool":
      return "warning";
    default:
      return "neutral";
  }
}

/* ------------------------------------------------------------------ tree -- */

export interface TreeRow {
  obs: Observation;
  depth: number;
  /** Where its bar starts and how long it is, as fractions of the trace. */
  offset: number;
  width: number;
}

/**
 * Observations in display order — each parent followed by its children,
 * siblings by start time — with their place on the trace's timeline.
 *
 * An observation whose parent never arrived (a remote parent in a
 * distributed trace, or one still in flight) is shown as a root rather than
 * dropped. Cycles, which no SDK should send, are cut rather than looped.
 */
export function buildTree(observations: Observation[], traceStart: number, traceEnd: number | null): TreeRow[] {
  const ids = new Set(observations.map((o) => o.id));
  const children = new Map<string | null, Observation[]>();
  for (const o of observations) {
    const parent = o.parent_id && ids.has(o.parent_id) && o.parent_id !== o.id ? o.parent_id : null;
    const list = children.get(parent) ?? [];
    list.push(o);
    children.set(parent, list);
  }
  const byStart = (a: Observation, b: Observation) => (a.start_us ?? Infinity) - (b.start_us ?? Infinity);
  for (const list of children.values()) list.sort(byStart);

  let start = traceStart;
  let end = traceEnd ?? traceStart;
  for (const o of observations) {
    if (o.start_us !== null) start = Math.min(start, o.start_us);
    end = Math.max(end, o.end_us ?? o.start_us ?? end);
  }
  const total = Math.max(end - start, 1);

  const rows: TreeRow[] = [];
  const seen = new Set<string>();
  const visit = (o: Observation, depth: number) => {
    if (seen.has(o.id)) return;
    seen.add(o.id);
    const s = o.start_us ?? start;
    const e = o.end_us ?? s;
    rows.push({
      obs: o,
      depth,
      offset: Math.min(1, Math.max(0, (s - start) / total)),
      width: Math.min(1, Math.max(0, (e - s) / total)),
    });
    for (const child of children.get(o.id) ?? []) visit(child, depth + 1);
  };
  for (const root of children.get(null) ?? []) visit(root, 0);
  // Anything left is part of a cycle: show it flat at the end.
  for (const o of observations) if (!seen.has(o.id)) visit(o, 0);
  return rows;
}

/* -------------------------------------------------------------- messages -- */

export interface ChatMessage {
  role: string;
  text: string;
  /** Tool calls the message asked for, shown as JSON below its text. */
  toolCalls?: unknown;
}

const LANGCHAIN_ROLES: Record<string, string> = {
  HumanMessage: "user",
  HumanMessageChunk: "user",
  AIMessage: "assistant",
  AIMessageChunk: "assistant",
  SystemMessage: "system",
  ToolMessage: "tool",
  FunctionMessage: "function",
  human: "user",
  ai: "assistant",
  system: "system",
  tool: "tool",
};

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

/** The words in a message's content, whichever way the SDK laid it out. */
export function contentText(content: unknown): string {
  if (content === null || content === undefined) return "";
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((part) => {
        if (typeof part === "string") return part;
        if (!isRecord(part)) return JSON.stringify(part);
        if (typeof part.text === "string") return part.text;
        if (typeof part.content === "string") return part.content;
        const type = typeof part.type === "string" ? part.type : "part";
        if (type.includes("image")) return "[image]";
        if (type.includes("audio")) return "[audio]";
        if (type === "tool_use" || type === "tool_call") return `[tool call: ${String(part.name ?? "")}]`;
        return JSON.stringify(part);
      })
      .join("\n");
  }
  if (isRecord(content) && typeof content.text === "string") return content.text;
  return JSON.stringify(content, null, 2);
}

function toMessage(item: unknown): ChatMessage | null {
  if (!isRecord(item)) return null;
  // LangChain's serialised form: {lc: 1, id: [..., "HumanMessage"], kwargs: {content}}.
  if (Array.isArray(item.id) && isRecord(item.kwargs)) {
    const cls = String(item.id[item.id.length - 1] ?? "");
    const role = LANGCHAIN_ROLES[cls] ?? (typeof item.kwargs.role === "string" ? item.kwargs.role : null);
    if (!role) return null;
    return { role, text: contentText(item.kwargs.content), toolCalls: nonEmpty(item.kwargs.tool_calls) };
  }
  let role: string | null = null;
  if (typeof item.role === "string") role = item.role;
  else if (typeof item.type === "string" && LANGCHAIN_ROLES[item.type]) role = LANGCHAIN_ROLES[item.type];
  if (!role) return null;
  const data = isRecord(item.data) ? item.data : item;
  // The GenAI conventions' newer shape: {role, parts: [{type: "text", content}]}.
  const content = data.content ?? data.parts ?? data.text;
  return { role, text: contentText(content), toolCalls: nonEmpty(data.tool_calls ?? data.toolCalls) };
}

function nonEmpty(v: unknown): unknown {
  if (v === null || v === undefined) return undefined;
  if (Array.isArray(v) && v.length === 0) return undefined;
  return v;
}

/**
 * The value as a chat, when it is one: a list of messages, `{messages}`, an
 * OpenAI response's `choices`, LangChain's `generations`, or one message on
 * its own. `null` when it's anything else, so the caller shows JSON instead.
 */
export function asMessages(value: unknown): ChatMessage[] | null {
  let items: unknown[] | null = null;
  if (Array.isArray(value)) items = value;
  else if (isRecord(value)) {
    if (Array.isArray(value.messages)) items = value.messages;
    else if (Array.isArray(value.choices))
      items = value.choices.map((c) => (isRecord(c) ? (c.message ?? c.delta ?? c) : c));
    else if (Array.isArray(value.generations))
      items = value.generations.flat().map((g) => (isRecord(g) ? (g.message ?? { role: "assistant", content: g.text }) : g));
    else if (typeof value.role === "string" || Array.isArray(value.id)) items = [value];
  }
  if (!items || items.length === 0) return null;
  // LangSmith wraps a batch of chats as [[...]]: one chat is the common case.
  if (items.every(Array.isArray)) items = (items as unknown[][]).flat();
  const messages = items.map(toMessage);
  if (messages.some((m) => m === null)) return null;
  return messages as ChatMessage[];
}

/** JSON for display; strings as they are, so prose isn't wrapped in quotes. */
export function prettyValue(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2) ?? String(value);
  } catch {
    return String(value);
  }
}

export function isEmptyValue(value: unknown): boolean {
  if (value === null || value === undefined || value === "") return true;
  if (Array.isArray(value)) return value.length === 0;
  if (isRecord(value)) return Object.keys(value).length === 0;
  return false;
}
