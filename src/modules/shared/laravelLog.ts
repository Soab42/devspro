/**
 * Reads Laravel's console output back into runs, for the log viewer's Table
 * tab. These shapes are understood:
 *
 *   queue:work (Laravel 9+)     2026-10-22 13:17:09 App\Jobs\LabJob ..... RUNNING
 *                               2026-10-22 13:17:13 App\Jobs\LabJob ..... 3s DONE
 *   queue:work -v               2026-10-22 13:17:09 App\Jobs\LabJob 226 database default .. RUNNING
 *                               2026-10-22 13:17:13 App\Jobs\LabJob 226 database default .. 62.71ms 4MB FAIL
 *   queue:work (Laravel <= 8)   [2026-10-22 13:17:09][42] Processing: App\Jobs\LabJob
 *                               [2026-10-22 13:17:13][42] Processed:  App\Jobs\LabJob
 *   schedule:run                2026-10-22 13:17:00 Running ["artisan" inspire] .. 1s DONE
 *                                 ⇂ description of the event
 *
 * A job is two lines, start and end, paired by job id when the worker prints
 * one (-v, Laravel 8) and by class name in order otherwise. A scheduled event
 * is one line written when it finishes. Any other line belongs to the run
 * printed just before it — that is where a crashing job's error lands — and
 * is kept as that run's output.
 */

export type RunStatus = "running" | "done" | "failed" | "unknown";

export interface LogRun {
  /** Stable across polls, while the 300-line tail slides underneath. */
  key: string;
  /** Job class, or the scheduled command without `Running [...]`. */
  name: string;
  kind: "job" | "command";
  status: RunStatus;
  /** As printed, `YYYY-MM-DD HH:MM:SS`. */
  started?: string;
  finished?: string;
  /** As printed, e.g. `3s`, `15.20ms`, `1m 5s`. */
  duration?: string;
  durationMs?: number;
  /** -v only. */
  jobId?: string;
  connection?: string;
  queue?: string;
  memory?: string;
  /** A scheduled event's `⇂ ...` line. */
  description?: string;
  /** Lines the command printed between this run and the next one. */
  output: string[];
}

export interface ParsedLog {
  /** In log order, oldest first. */
  runs: LogRun[];
  /** Non-blank lines that are not part of a run. */
  otherLines: number;
}

const STAMP = String.raw`(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d)`;
const DURATION = String.raw`(\d+(?:\.\d+)?\s?(?:ms|s|m|h)(?:\s\d+(?:\.\d+)?\s?(?:ms|s|m|h))*)`;
const MEMORY = String.raw`(\d+(?:\.\d+)?\s?[KMGT]?B)`;

/** Name, optional dot leader, optional duration and memory, outcome. */
const MODERN = new RegExp(
  String.raw`^\s*${STAMP}\s+(.+?)\s*\.*\s+(?:${DURATION}\s+)?(?:${MEMORY}\s+)?(RUNNING|DONE|FAIL|FAILED)\s*$`,
);
const LEGACY = new RegExp(String.raw`^\s*\[${STAMP}\](?:\[([^\]]*)\])?\s*(Processing|Processed|Failed):\s+(.+?)\s*$`);
/** The arrow Laravel prints before an event's description. */
const DESCRIPTION = /^\s*\u21C2\s+(.+?)\s*$/;
/** DevsPro's own `--- manual run at ... ---` header. */
const SEPARATOR = /^\s*---.*---\s*$/;
const ANSI = /\u001b\[[0-9;]*[A-Za-z]/g;

const UNIT_MS: Record<string, number> = { ms: 1, s: 1000, m: 60_000, h: 3_600_000 };

export function durationToMs(text: string): number | undefined {
  let total = 0;
  let seen = false;
  for (const [, value, unit] of text.matchAll(/(\d+(?:\.\d+)?)\s?(ms|s|m|h)/g)) {
    total += Number(value) * UNIT_MS[unit];
    seen = true;
  }
  return seen ? total : undefined;
}

/** `Running ["artisan" inspire --x]` -> `artisan inspire --x`; `Running [Callback]` -> `Callback`. */
function commandName(raw: string): string | null {
  const m = /^Running \[(.*)\]$/.exec(raw);
  if (!m) return null;
  return m[1].replace(/"/g, "").replace(/\s+/g, " ").trim();
}

/** -v puts `<id> <connection> <queue>` after the class. */
function jobParts(raw: string): Pick<LogRun, "name" | "jobId" | "connection" | "queue"> {
  const parts = raw.split(/\s+/);
  if (parts.length === 4 && /^[\w-]+$/.test(parts[1])) {
    return { name: parts[0], jobId: parts[1], connection: parts[2], queue: parts[3] };
  }
  return { name: raw };
}

export function parseLaravelLog(text: string): ParsedLog {
  const runs: LogRun[] = [];
  /** Indexes into `runs` of jobs seen starting but not yet ending. */
  const open: number[] = [];
  /** The run whose line was printed last; output that follows is its. */
  let latest: LogRun | undefined;
  let otherLines = 0;
  const seenKeys = new Map<string, number>();

  const add = (run: Omit<LogRun, "key" | "output">) => {
    const base = `${run.jobId ?? ""}|${run.started ?? `end ${run.finished}`}|${run.name}`;
    const n = (seenKeys.get(base) ?? 0) + 1;
    seenKeys.set(base, n);
    const full: LogRun = { ...run, key: `${base}|${n}`, output: [] };
    runs.push(full);
    latest = full;
    return full;
  };

  const start = (raw: string, at: string, jobId?: string) => {
    add({ ...jobParts(raw), ...(jobId ? { jobId } : {}), kind: "job", status: "running", started: at });
    open.push(runs.length - 1);
  };

  const finish = (raw: string, at: string, ok: boolean, duration?: string, memory?: string, jobId?: string) => {
    const status: RunStatus = ok ? "done" : "failed";
    const timing = { duration, durationMs: duration ? durationToMs(duration) : undefined, memory };
    const command = commandName(raw);
    if (command !== null) {
      // Scheduled events print once, stamped with their start.
      add({ name: command, kind: "command", status, started: at, ...timing });
      return;
    }
    const parts = { ...jobParts(raw), ...(jobId ? { jobId } : {}) };
    const pos = open.findIndex((i) =>
      parts.jobId ? runs[i].jobId === parts.jobId : runs[i].name === parts.name,
    );
    if (pos === -1) {
      add({ ...parts, kind: "job", status, finished: at, ...timing });
      return;
    }
    const run = runs[open[pos]];
    open.splice(pos, 1);
    Object.assign(run, { status, finished: at, ...timing });
    latest = run;
  };

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.replace(ANSI, "");
    if (!line.trim()) continue;

    const modern = MODERN.exec(line);
    if (modern) {
      const [, at, name, duration, memory, outcome] = modern;
      if (outcome === "RUNNING") start(name, at);
      else finish(name, at, outcome === "DONE", duration, memory);
      continue;
    }

    const legacy = LEGACY.exec(line);
    if (legacy) {
      const [, at, jobId, verb, name] = legacy;
      if (verb === "Processing") start(name, at, jobId);
      else finish(name, at, verb === "Processed", undefined, undefined, jobId);
      continue;
    }

    const description = DESCRIPTION.exec(line);
    if (description && latest?.kind === "command" && !latest.description) {
      latest.description = description[1];
      continue;
    }

    otherLines += 1;
    if (latest && !SEPARATOR.test(line)) latest.output.push(line);
  }

  // Only the newest unfinished job can still be going: a worker runs one job
  // at a time, so an earlier one that never printed its end was cut short
  // (worker killed, log truncated mid-run).
  open.slice(0, -1).forEach((i) => {
    runs[i].status = "unknown";
  });

  return { runs, otherLines };
}

export interface RunSummary {
  total: number;
  done: number;
  failed: number;
  running: number;
  /** Mean of the runs that printed a duration. */
  averageMs?: number;
}

export function summarize(runs: LogRun[]): RunSummary {
  const timed = runs.map((r) => r.durationMs).filter((ms): ms is number => ms !== undefined);
  return {
    total: runs.length,
    done: runs.filter((r) => r.status === "done").length,
    failed: runs.filter((r) => r.status === "failed").length,
    running: runs.filter((r) => r.status === "running").length,
    averageMs: timed.length ? timed.reduce((a, b) => a + b, 0) / timed.length : undefined,
  };
}

export function formatMs(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)}s`;
  const minutes = Math.floor(ms / 60_000);
  return `${minutes}m ${Math.round((ms % 60_000) / 1000)}s`;
}

/** `YYYY-MM-DD HH:MM:SS` moved by `seconds`, as wall-clock time. */
export function shiftStamp(stamp: string, seconds: number): string {
  const [date, time] = stamp.split(" ");
  const [y, mo, d] = date.split("-").map(Number);
  const [h, mi, s] = time.split(":").map(Number);
  const t = new Date(Date.UTC(y, mo - 1, d, h, mi, s + seconds));
  return t.toISOString().slice(0, 19).replace("T", " ");
}

/**
 * The stretch of time a failed run's error could have been logged in, from
 * its start to just after its end. Laravel stamps its log and the worker's
 * console with the same app clock, in whole seconds.
 *
 * `likelyFrom` narrows that to around the end: a job's exception is reported
 * the moment it fails, which is the moment the FAIL line is printed. Earlier
 * entries in the window are often a neighbouring job's, started the same
 * second.
 */
export function failureWindow(run: LogRun): { from: string; to: string; likelyFrom: string } | null {
  const first = run.started ?? run.finished;
  const last = run.finished ?? run.started;
  if (!first || !last) return null;
  return { from: first, to: shiftStamp(last, 1), likelyFrom: run.finished ? shiftStamp(run.finished, -1) : first };
}
