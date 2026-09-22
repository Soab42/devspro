/**
 * Env-file parsing, diffing, and in-place editing.
 *
 * Files are compared as *sets of variables*, never as lines. Two teammates who
 * add the same variable in different places, with different quoting or a
 * different comment above it, have not diverged — so position, blank lines,
 * comments and quote style all drop out here, and what is left is: which keys
 * exist, and what each is set to.
 *
 * The grammar follows docker compose's `.env` rules, since this app lives beside
 * compose: `#` starts a comment only after whitespace (so `COLOR=#fff` is a
 * value), double quotes process escapes, single quotes are literal, quoted
 * values may span lines, and when a key repeats the last one wins.
 */

export type QuoteStyle = "none" | "single" | "double";

export interface EnvEntry {
  key: string;
  /** The value as a program would read it: quotes stripped, escapes resolved. */
  value: string;
  /** 1-based first line of the definition. */
  line: number;
  /** Last line — differs from `line` only for a quoted value that spans lines. */
  endLine: number;
  quote: QuoteStyle;
  exported: boolean;
  /** Raw text after the value on its line, usually an inline comment. */
  trailing: string;
}

export interface EnvIssue {
  line: number;
  message: string;
}

export interface ParsedEnv {
  /** One entry per key — the last definition, since that is the one that takes effect. */
  entries: Map<string, EnvEntry>;
  issues: EnvIssue[];
}

const LINE_RE = /^\s*(export\s+)?([A-Za-z_][A-Za-z0-9_.-]*)\s*(=)?(.*)$/;

const ESCAPES: Record<string, string> = { n: "\n", r: "\r", t: "\t", '"': '"', "\\": "\\" };

function splitLines(text: string): string[] {
  return text.replace(/^\uFEFF/, "").split(/\r\n|\n|\r/);
}

interface ReadValue {
  value: string;
  quote: QuoteStyle;
  lastIndex: number;
  trailing: string;
}

/** Reads a quoted value starting just after its opening quote, across lines. */
function readQuoted(
  quote: '"' | "'",
  first: string,
  lines: string[],
  index: number,
): ReadValue | null {
  let out = "";
  let li = index;
  let text = first;
  for (;;) {
    for (let c = 0; c < text.length; c++) {
      const ch = text[c];
      if (quote === '"' && ch === "\\" && c + 1 < text.length) {
        const next = text[c + 1];
        out += ESCAPES[next] ?? ch + next;
        c++;
        continue;
      }
      if (ch === quote) {
        return {
          value: out,
          quote: quote === '"' ? "double" : "single",
          lastIndex: li,
          trailing: text.slice(c + 1),
        };
      }
      out += ch;
    }
    if (li + 1 >= lines.length) return null;
    out += "\n";
    li++;
    text = lines[li];
  }
}

function readValue(rest: string, lines: string[], index: number): ReadValue | { unterminated: true } {
  const body = rest.trimStart();
  const q = body[0];
  if (q === '"' || q === "'") {
    const quoted = readQuoted(q, body.slice(1), lines, index);
    if (quoted) return quoted;
    return { unterminated: true };
  }
  // Whitespace-then-# only: `COLOR=#fff` and `URL=http://x/#top` are values.
  const commentAt = rest.search(/\s#/);
  return {
    value: (commentAt === -1 ? rest : rest.slice(0, commentAt)).trim(),
    quote: "none",
    lastIndex: index,
    trailing: commentAt === -1 ? "" : rest.slice(commentAt),
  };
}

export function parseEnv(text: string): ParsedEnv {
  const lines = splitLines(text);
  const entries = new Map<string, EnvEntry>();
  const issues: EnvIssue[] = [];

  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const trimmed = raw.trim();
    if (trimmed === "" || trimmed.startsWith("#")) continue;

    const match = LINE_RE.exec(raw);
    if (!match) {
      issues.push({ line: i + 1, message: "Not a KEY=value line — ignored." });
      continue;
    }
    const [, exportPrefix, key, equals, rest] = match;
    if (!equals) {
      issues.push({ line: i + 1, message: `${key} has no "=" — ignored.` });
      continue;
    }

    let read = readValue(rest, lines, i);
    if ("unterminated" in read) {
      issues.push({
        line: i + 1,
        message: `${key} opens a quote that is never closed — read as plain text.`,
      });
      const commentless = rest.trim();
      read = { value: commentless, quote: "none", lastIndex: i, trailing: "" };
    }

    const previous = entries.get(key);
    if (previous) {
      issues.push({
        line: i + 1,
        message: `${key} is already set on line ${previous.line} — this later value is the one that counts.`,
      });
    }
    entries.set(key, {
      key,
      value: read.value,
      line: i + 1,
      endLine: read.lastIndex + 1,
      quote: read.quote,
      exported: Boolean(exportPrefix),
      trailing: read.trailing,
    });
    i = read.lastIndex;
  }

  return { entries, issues };
}

/* ------------------------------------------------------------------ diff -- */

export type DiffStatus = "same" | "changed" | "only-left" | "only-right";

export interface DiffRow {
  key: string;
  status: DiffStatus;
  left?: EnvEntry;
  right?: EnvEntry;
}

const compareKeys = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

export function diffEnv(
  left: ParsedEnv,
  right: ParsedEnv,
  { keysOnly = false }: { keysOnly?: boolean } = {},
): DiffRow[] {
  const keys = new Set([...left.entries.keys(), ...right.entries.keys()]);
  return [...keys].sort(compareKeys).map((key) => {
    const l = left.entries.get(key);
    const r = right.entries.get(key);
    const status: DiffStatus = !r
      ? "only-left"
      : !l
        ? "only-right"
        : keysOnly || l.value === r.value
          ? "same"
          : "changed";
    return { key, status, left: l, right: r };
  });
}

export function countStatuses(rows: DiffRow[]): Record<DiffStatus, number> {
  const counts: Record<DiffStatus, number> = {
    same: 0,
    changed: 0,
    "only-left": 0,
    "only-right": 0,
  };
  for (const row of rows) counts[row.status]++;
  return counts;
}

/* --------------------------------------------------------------- writing -- */

export interface EnvChange {
  key: string;
  value: string;
  /** The source's quote style, carried over so `"${A}/x"` doesn't silently become literal. */
  quote: QuoteStyle;
}

export const toChange = (entry: EnvEntry): EnvChange => ({
  key: entry.key,
  value: entry.value,
  quote: entry.quote,
});

function isBareSafe(value: string): boolean {
  return !/[\r\n]/.test(value) && value === value.trim() && !/^["']/.test(value) && !/\s#/.test(value);
}

function escapeDouble(value: string): string {
  return (
    value
      // A backslash only needs doubling where it would otherwise be read as
      // part of an escape (or would swallow the closing quote). Doubling every
      // one would turn `C:\dir` into `C:\\dir` for parsers that don't unescape.
      .replace(/\\(?=[nrt"\\\r\n]|$)/g, "\\\\")
      .replace(/"/g, '\\"')
      .replace(/\r/g, "\\r")
      .replace(/\n/g, "\\n")
  );
}

export function formatValue(value: string, quote: QuoteStyle): string {
  if (quote === "single" && !/[\r\n']/.test(value)) return `'${value}'`;
  if (quote === "none" && isBareSafe(value)) return value;
  return `"${escapeDouble(value)}"`;
}

/**
 * Returns `text` with each change applied *in the file's own layout*: an
 * existing variable is rewritten on its own line (keeping `export` and any
 * inline comment), a missing one is appended at the bottom. Everything else —
 * order, comments, blank lines, line endings — is left exactly as it was.
 */
export function applyChanges(text: string, changes: EnvChange[]): string {
  if (changes.length === 0) return text;

  const bom = text.startsWith("\uFEFF") ? "\uFEFF" : "";
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  const lines = splitLines(text);
  const endedWithNewline = lines[lines.length - 1] === "";
  if (endedWithNewline) lines.pop();

  const parsed = parseEnv(text);
  const replacements: { start: number; end: number; line: string }[] = [];
  const additions: string[] = [];

  for (const change of changes) {
    const formatted = `${change.key}=${formatValue(change.value, change.quote)}`;
    const entry = parsed.entries.get(change.key);
    if (!entry) {
      additions.push(formatted);
      continue;
    }
    const tail = entry.line === entry.endLine ? entry.trailing : "";
    replacements.push({
      start: entry.line - 1,
      end: entry.endLine - 1,
      line: `${entry.exported ? "export " : ""}${formatted}${tail}`,
    });
  }

  // Bottom-up, so a multi-line value collapsing to one line can't shift the
  // line numbers of the replacements still to come.
  replacements.sort((a, b) => b.start - a.start);
  for (const r of replacements) lines.splice(r.start, r.end - r.start + 1, r.line);
  lines.push(...additions);

  const finalNewline = endedWithNewline || additions.length > 0;
  return bom + lines.join(eol) + (finalNewline && lines.length > 0 ? eol : "");
}

/* ------------------------------------------------------------------ side -- */

/** One of the two files being compared. */
export interface EnvSide {
  text: string;
  /** Set only when the text came from a file on disk — and it is the one Save writes to. */
  path: string | null;
  /** What was on disk when last read or saved; `text` differing from it means unsaved edits. */
  disk: string | null;
}

export const emptySide = (): EnvSide => ({ text: "", path: null, disk: null });

export const isDirty = (side: EnvSide): boolean => side.path !== null && side.text !== side.disk;

/**
 * A label for a column header. Two files are often both called `.env` (one per
 * checkout), so when the names collide the parent folder is added.
 */
export function shortName(path: string | null, otherPath: string | null): string | null {
  if (!path) return null;
  const parts = path.split(/[\\/]/).filter(Boolean);
  const name = parts[parts.length - 1] ?? path;
  const otherName = otherPath?.split(/[\\/]/).filter(Boolean).pop();
  return name === otherName && parts.length > 1 ? `${parts[parts.length - 2]}/${name}` : name;
}
