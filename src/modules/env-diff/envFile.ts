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
  /** Written as `# KEY=value`: in the file, but switched off. */
  commented: boolean;
}

export interface EnvIssue {
  line: number;
  message: string;
}

export interface ParsedEnv {
  /**
   * One entry per key: the last definition, since that is the one that takes
   * effect. A commented-out one is kept only when the key has no live one.
   */
  entries: Map<string, EnvEntry>;
  /** Every definition in file order, repeats and commented-out ones included. */
  definitions: EnvEntry[];
  issues: EnvIssue[];
}

const LINE_RE = /^\s*(export\s+)?([A-Za-z_][A-Za-z0-9_.-]*)\s*(=)?(.*)$/;

// A variable switched off with `#`. Stricter than a live line — no space before
// the `=` — so that prose such as `# set x = 1 to enable` stays a comment.
const COMMENTED_RE = /^\s*#+\s*(export\s+)?([A-Za-z_][A-Za-z0-9_.-]*)=(.*)$/;

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
  const definitions: EnvEntry[] = [];
  const issues: EnvIssue[] = [];

  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const trimmed = raw.trim();
    if (trimmed === "") continue;
    if (trimmed.startsWith("#")) {
      const off = readCommented(raw, i);
      if (!off) continue;
      definitions.push(off);
      // A live definition always wins over a commented-out one, wherever it sits.
      if (entries.get(off.key)?.commented !== false) entries.set(off.key, off);
      continue;
    }

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
    if (previous && !previous.commented) {
      issues.push({
        line: i + 1,
        message: `${key} is already set on line ${previous.line} — this later value is the one that counts.`,
      });
    }
    const entry: EnvEntry = {
      key,
      value: read.value,
      line: i + 1,
      endLine: read.lastIndex + 1,
      quote: read.quote,
      exported: Boolean(exportPrefix),
      trailing: read.trailing,
      commented: false,
    };
    entries.set(key, entry);
    definitions.push(entry);
    i = read.lastIndex;
  }

  issues.push(...sharedValueIssues(entries));
  // Stable, so a line's own problems stay in the order they were found.
  issues.sort((a, b) => a.line - b.line);
  return { entries, definitions, issues };
}

function readCommented(raw: string, index: number): EnvEntry | null {
  const match = COMMENTED_RE.exec(raw);
  if (!match) return null;
  const [, exportPrefix, key, rest] = match;
  // One line only: the lines after a commented-out quote aren't commented, so
  // they can't be its value. An unclosed quote is read as plain text, quietly —
  // it is a comment, and nothing will ever read it.
  const read = readValue(rest, [rest], 0);
  const plain = "unterminated" in read;
  return {
    key,
    value: plain ? rest.trim() : read.value,
    line: index + 1,
    endLine: index + 1,
    quote: plain ? "none" : read.quote,
    exported: Boolean(exportPrefix),
    trailing: plain ? "" : read.trailing,
    commented: true,
  };
}

// Flags like DEBUG=false are expected to repeat, so neither a shared value nor
// a rename is read into them: reporting those would bury the pairs worth a look,
// such as two secrets or two services on one port.
const TRIVIAL_VALUE_RE = /^(|true|false|yes|no|on|off|0|1)$/i;

/**
 * One issue per variable whose value an earlier variable already holds — often
 * a copy-paste slip, like a test database URL left pointing at the real one.
 * Only effective values count, and the message names keys, never the value:
 * it may be a secret, and the page masks values by default.
 */
function sharedValueIssues(entries: Map<string, EnvEntry>): EnvIssue[] {
  const firstWith = new Map<string, EnvEntry>();
  const issues: EnvIssue[] = [];
  const byLine = [...entries.values()]
    .filter((entry) => !entry.commented)
    .sort((a, b) => a.line - b.line);
  for (const entry of byLine) {
    if (TRIVIAL_VALUE_RE.test(entry.value)) continue;
    const first = firstWith.get(entry.value);
    if (!first) {
      firstWith.set(entry.value, entry);
      continue;
    }
    issues.push({
      line: entry.line,
      message: `${entry.key} has the same value as ${first.key} on line ${first.line}.`,
    });
  }
  return issues;
}

/* ------------------------------------------------------------------ diff -- */

/** `commented`: on both sides, but switched off with `#` on exactly one. */
export type DiffStatus = "same" | "changed" | "commented" | "only-left" | "only-right";

/** Which rows the comparison shows: all of them, every difference, or one status. */
export type DiffFilter = "all" | "diff" | DiffStatus;

export interface DiffRow {
  key: string;
  status: DiffStatus;
  left?: EnvEntry;
  right?: EnvEntry;
  /**
   * For a key on one side only: the key on the other side, also on its own,
   * that holds the same value — likely the same variable under a new name.
   */
  renamed?: string;
}

const compareKeys = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

export function diffEnv(
  left: ParsedEnv,
  right: ParsedEnv,
  { keysOnly = false }: { keysOnly?: boolean } = {},
): DiffRow[] {
  const keys = new Set([...left.entries.keys(), ...right.entries.keys()]);
  const rows = [...keys].sort(compareKeys).map((key): DiffRow => {
    const l = left.entries.get(key);
    const r = right.entries.get(key);
    const status: DiffStatus = !r
      ? "only-left"
      : !l
        ? "only-right"
        : // Live on one side and off on the other is a difference in which keys
          // take effect, so it counts in keys-only mode too.
          l.commented !== r.commented
          ? "commented"
          : keysOnly || l.value === r.value
            ? "same"
            : "changed";
    return { key, status, left: l, right: r };
  });
  // Renames are spotted by value, and keys-only mode sets values aside.
  if (!keysOnly) markRenames(rows);
  return rows;
}

/**
 * Pairs a key found only on the left with one found only on the right when the
 * two hold the same value. Only a one-to-one match counts: with two candidates
 * on either side there is no telling which one was renamed.
 */
function markRenames(rows: DiffRow[]) {
  const byValue = new Map<string, { left: DiffRow[]; right: DiffRow[] }>();
  for (const row of rows) {
    const entry =
      row.status === "only-left" ? row.left : row.status === "only-right" ? row.right : undefined;
    if (!entry || entry.commented || TRIVIAL_VALUE_RE.test(entry.value)) continue;
    let group = byValue.get(entry.value);
    if (!group) byValue.set(entry.value, (group = { left: [], right: [] }));
    (row.status === "only-left" ? group.left : group.right).push(row);
  }
  for (const { left, right } of byValue.values()) {
    if (left.length !== 1 || right.length !== 1) continue;
    left[0].renamed = right[0].key;
    right[0].renamed = left[0].key;
  }
}

export function countStatuses(rows: DiffRow[]): Record<DiffStatus, number> {
  const counts: Record<DiffStatus, number> = {
    same: 0,
    changed: 0,
    commented: 0,
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
  /** Written as `# KEY=value`, so a switched-off variable arrives switched off. */
  commented?: boolean;
}

export const toChange = (entry: EnvEntry): EnvChange => ({
  key: entry.key,
  value: entry.value,
  quote: entry.quote,
  commented: entry.commented,
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
    const off = change.commented ? "# " : "";
    const formatted = `${change.key}=${formatValue(change.value, change.quote)}`;
    // A commented-out entry is rewritten in place too: copying a live value
    // over `# KEY=old` switches it on where the file already keeps it.
    const entry = parsed.entries.get(change.key);
    if (!entry) {
      additions.push(off + formatted);
      continue;
    }
    const tail = entry.line === entry.endLine ? entry.trailing : "";
    replacements.push({
      start: entry.line - 1,
      end: entry.endLine - 1,
      line: `${off}${entry.exported ? "export " : ""}${formatted}${tail}`,
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

/* ------------------------------------------------------------- arranging -- */

export interface Arranged {
  text: string;
  /** The reference's variables the target lacks — each left as a blank line. */
  gaps: number;
  /** The target's variables the reference lacks — moved to the end. */
  extras: number;
}

/**
 * `target` laid out like `reference`, so each variable sits on the same line in
 * both and the two can be read side by side.
 *
 * The reference supplies the skeleton: its order, comments and blank lines.
 * The target supplies every definition, as written: no value moves between the
 * files, only position. A variable the target lacks leaves a blank line in its
 * place so the lines below still pair up, and the target's own extras follow
 * at the end, each with the comment lines that sat directly above it.
 */
export function arrangeLike(target: string, reference: string): Arranged {
  const ref = parseEnv(reference);
  const own = parseEnv(target);
  if (ref.definitions.length === 0 || own.definitions.length === 0) {
    return { text: target, gaps: 0, extras: 0 };
  }

  const bom = target.startsWith("\uFEFF") ? "\uFEFF" : "";
  const eol = target.includes("\r\n") ? "\r\n" : "\n";
  const ownLines = splitLines(target);
  const refLines = splitLines(reference);
  const endedWithNewline = ownLines[ownLines.length - 1] === "";
  if (refLines[refLines.length - 1] === "") refLines.pop();
  const source = (d: EnvEntry) => ownLines.slice(d.line - 1, d.endLine);
  const blank = (n: number) => Array.from({ length: Math.max(0, n) }, () => "");

  const pool = new Map<string, EnvEntry[]>();
  for (const d of own.definitions) pool.set(d.key, [...(pool.get(d.key) ?? []), d]);
  const refAt = new Map(ref.definitions.map((d) => [d.line, d]));

  const out: string[] = [];
  let gaps = 0;
  for (let i = 0; i < refLines.length; i++) {
    const slot = refAt.get(i + 1);
    if (!slot) {
      // Comments and blank lines are the reference's layout. A line that is
      // neither isn't a variable, and is the reference's own business.
      out.push(/^\s*(#|$)/.test(refLines[i]) ? refLines[i] : "");
      continue;
    }
    const span = slot.endLine - slot.line + 1;
    i = slot.endLine - 1;
    // Like for like first, so `# PORT=…` and `PORT=…` each find their match.
    // Failing that, the reference's effective definition takes whatever the
    // target has, and any other slot for the key is left blank.
    const candidates = pool.get(slot.key) ?? [];
    let at = candidates.findIndex((d) => d.commented === slot.commented);
    if (at === -1 && ref.entries.get(slot.key) === slot && candidates.length > 0) at = 0;
    if (at === -1) {
      if (ref.entries.get(slot.key) === slot) gaps++;
      out.push(...blank(span));
      continue;
    }
    const [placed] = candidates.splice(at, 1);
    const lines = source(placed);
    out.push(...lines, ...blank(span - lines.length));
  }

  const leftovers = [...pool.values()].flat().sort((a, b) => a.line - b.line);
  const extras = new Set(leftovers.filter((d) => !ref.entries.has(d.key)).map((d) => d.key)).size;
  if (leftovers.length > 0) {
    const defined = new Set(own.definitions.flatMap((d) => [d.line, d.endLine]));
    if (out.length > 0 && out[out.length - 1].trim() !== "") out.push("");
    for (const d of leftovers) {
      let top = d.line - 1;
      while (top > 0 && ownLines[top - 1].trim().startsWith("#") && !defined.has(top)) top--;
      out.push(...ownLines.slice(top, d.line - 1), ...source(d));
    }
  }

  return { text: bom + out.join(eol) + (endedWithNewline ? eol : ""), gaps, extras };
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
