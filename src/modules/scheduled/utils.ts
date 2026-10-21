/**
 * Pure helpers for the Scheduled tasks page. The cron parser and describer
 * here mirror the Rust ones (which are authoritative for validation and
 * next-run) so the form can show a live "Every 5 minutes" as the user types,
 * without a round-trip per keystroke.
 */

/**
 * On Windows the backend registers tasks with Task Scheduler (under a
 * `\DevsPro\` folder) instead of writing the crontab; only the wording differs.
 */
export const ON_WINDOWS = typeof navigator !== "undefined" && /Windows/i.test(navigator.userAgent);

/** Where enabled tasks end up, for UI copy. */
export const SCHEDULER_NAME = ON_WINDOWS ? "Task Scheduler" : "crontab";

export interface CronPreset {
  value: string;
  label: string;
  /** Empty for "custom". */
  expr: string;
}

export const CRON_PRESETS: CronPreset[] = [
  { value: "minute", label: "Every minute", expr: "* * * * *" },
  { value: "5min", label: "Every 5 minutes", expr: "*/5 * * * *" },
  { value: "hourly", label: "Hourly", expr: "0 * * * *" },
  { value: "daily", label: "Daily at 03:00", expr: "0 3 * * *" },
  { value: "weekly", label: "Weekly (Sunday 03:00)", expr: "0 3 * * 0" },
  { value: "custom", label: "Custom…", expr: "" },
];

export function normaliseSchedule(expr: string): string {
  return expr.trim().split(/\s+/).filter(Boolean).join(" ");
}

/** Which preset an expression corresponds to, or "custom". */
export function presetForSchedule(expr: string): string {
  const norm = normaliseSchedule(expr);
  return CRON_PRESETS.find((p) => p.expr && p.expr === norm)?.value ?? "custom";
}

// ---------------------------------------------------------------------------
// Cron parsing
// ---------------------------------------------------------------------------

export interface CronField {
  values: number[];
  any: boolean;
}

export interface CronSpec {
  minute: CronField;
  hour: CronField;
  dom: CronField;
  month: CronField;
  dow: CronField;
}

const MONTH_NAMES = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
const DAY_NAMES = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
const MONTH_LONG = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];
const DAY_LONG = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

function parseValue(raw: string, names: string[] | null, label: string): number {
  if (/^\d+$/.test(raw)) return Number(raw);
  if (names) {
    const i = names.indexOf(raw.toLowerCase());
    // Months are 1-based, weekdays 0-based, matching cron's numeric ranges.
    if (i >= 0) return names.length === 12 ? i + 1 : i;
  }
  throw new Error(`${label}: '${raw}' is not a number`);
}

function parseField(raw: string, min: number, max: number, names: string[] | null, label: string): CronField {
  if (!raw) throw new Error(`${label}: field is empty`);
  const values = new Set<number>();
  let any = false;
  for (const part of raw.split(",")) {
    const [range, stepRaw] = part.includes("/") ? part.split("/", 2) : [part, undefined];
    let step = 1;
    if (stepRaw !== undefined) {
      step = /^\d+$/.test(stepRaw) ? Number(stepRaw) : 0;
      if (step <= 0) throw new Error(`${label}: step '/${stepRaw}' must be a positive number`);
    }
    let lo: number;
    let hi: number;
    if (range === "*") {
      [lo, hi] = [min, max];
    } else if (range.includes("-")) {
      const [a, b] = range.split("-", 2);
      [lo, hi] = [parseValue(a, names, label), parseValue(b, names, label)];
    } else {
      const v = parseValue(range, names, label);
      [lo, hi] = step > 1 ? [v, max] : [v, v];
    }
    if (lo > hi) throw new Error(`${label}: range ${lo}-${hi} is backwards`);
    if (lo < min || hi > max) throw new Error(`${label}: values must be between ${min} and ${max}`);
    if (range === "*" && step === 1) any = true;
    for (let v = lo; v <= hi; v += step) values.add(v);
  }
  return { values: [...values].sort((a, b) => a - b), any };
}

/** Throws with a human-readable message on an invalid expression. */
export function parseCron(expr: string): CronSpec {
  const fields = expr.trim().split(/\s+/).filter(Boolean);
  if (fields.length !== 5) {
    throw new Error(`a schedule needs 5 fields (minute hour day month weekday), got ${fields.length}`);
  }
  const dow = parseField(fields[4], 0, 7, DAY_NAMES, "weekday");
  if (dow.values.includes(7)) {
    dow.values = dow.values.filter((d) => d !== 7);
    if (!dow.values.includes(0)) dow.values.unshift(0);
  }
  return {
    minute: parseField(fields[0], 0, 59, null, "minute"),
    hour: parseField(fields[1], 0, 23, null, "hour"),
    dom: parseField(fields[2], 1, 31, null, "day of month"),
    month: parseField(fields[3], 1, 12, MONTH_NAMES, "month"),
    dow,
  };
}

/** The error message for an invalid expression, or null when it's fine. */
export function cronError(expr: string): string | null {
  try {
    parseCron(expr);
    return null;
  } catch (e) {
    return e instanceof Error ? e.message : String(e);
  }
}

// ---------------------------------------------------------------------------
// Description
// ---------------------------------------------------------------------------

function joinNames(items: string[]): string {
  if (items.length <= 1) return items[0] ?? "";
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

function evenStep(field: CronField, min: number, max: number): number | null {
  if (field.any || field.values.length < 2 || field.values[0] !== min) return null;
  const step = field.values[1] - field.values[0];
  const expected: number[] = [];
  for (let v = min; v <= max; v += step) expected.push(v);
  return expected.length === field.values.length && expected.every((v, i) => v === field.values[i])
    ? step
    : null;
}

const pad = (n: number) => String(n).padStart(2, "0");
const hhmm = (h: number, m: number) => `${pad(h)}:${pad(m)}`;
const sameList = (a: number[], b: number[]) => a.length === b.length && a.every((v, i) => v === b[i]);

function daySuffix(spec: CronSpec): string {
  let dowPart: string | null = null;
  if (!spec.dow.any) {
    if (sameList(spec.dow.values, [1, 2, 3, 4, 5])) dowPart = "on weekdays";
    else if (sameList(spec.dow.values, [0, 6])) dowPart = "on weekends";
    else dowPart = `on ${joinNames(spec.dow.values.map((d) => DAY_LONG[d]))}`;
  }
  const domPart = spec.dom.any
    ? null
    : `on day ${joinNames(spec.dom.values.map(String))} of the month`;
  const monthPart = spec.month.any
    ? null
    : `in ${joinNames(spec.month.values.map((m) => MONTH_LONG[m - 1]))}`;

  const parts: string[] = [];
  if (!domPart && !dowPart) parts.push("every day");
  else if (domPart && dowPart) parts.push(`${domPart} or ${dowPart}`);
  else parts.push((domPart ?? dowPart) as string);
  if (monthPart) parts.push(monthPart);
  return parts.join(" ");
}

export function describeSpec(spec: CronSpec): string {
  const daysAny = spec.dom.any && spec.dow.any && spec.month.any;
  const withDays = (base: string) => (daysAny ? base : `${base} ${daySuffix(spec)}`);
  const singleMinute = !spec.minute.any && spec.minute.values.length === 1 ? spec.minute.values[0] : null;
  const singleHour = !spec.hour.any && spec.hour.values.length === 1 ? spec.hour.values[0] : null;

  if (spec.hour.any) {
    if (spec.minute.any) return withDays("Every minute");
    const step = evenStep(spec.minute, 0, 59);
    if (step !== null) return withDays(`Every ${step} minutes`);
    if (singleMinute !== null) {
      return withDays(singleMinute === 0 ? "Every hour" : `Every hour at :${pad(singleMinute)}`);
    }
  }

  const hourStep = evenStep(spec.hour, 0, 23);
  if (singleMinute !== null && hourStep !== null) {
    return withDays(
      singleMinute === 0 ? `Every ${hourStep} hours` : `Every ${hourStep} hours at :${pad(singleMinute)}`,
    );
  }

  if (singleMinute !== null && singleHour !== null) {
    return `At ${hhmm(singleHour, singleMinute)} ${daySuffix(spec)}`;
  }

  if (singleMinute !== null && !spec.hour.any) {
    return `At ${joinNames(spec.hour.values.map((h) => hhmm(h, singleMinute)))} ${daySuffix(spec)}`;
  }

  const minutePart = spec.minute.any ? "Every minute" : `At minute ${joinNames(spec.minute.values.map(String))}`;
  const hourPart = spec.hour.any ? "" : ` past hour ${joinNames(spec.hour.values.map(String))}`;
  return `${minutePart}${hourPart} ${daySuffix(spec)}`;
}

/** "Every 5 minutes", or the validation error for a bad expression. */
export function describeCron(expr: string): { text: string; valid: boolean } {
  try {
    return { text: describeSpec(parseCron(expr)), valid: true };
  } catch (e) {
    return { text: e instanceof Error ? e.message : String(e), valid: false };
  }
}

// ---------------------------------------------------------------------------
// Time formatting
// ---------------------------------------------------------------------------

/**
 * "in 3 min", "2 h ago", "just now". Table cells, so terse on purpose; the
 * absolute time goes in the title attribute.
 */
export function formatRelative(iso: string | null, nowMs: number): string {
  if (!iso) return "—";
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "—";
  const diff = t - nowMs;
  const abs = Math.abs(diff);
  const future = diff > 0;
  let text: string;
  if (abs < 45_000) return future ? "in under a minute" : "just now";
  else if (abs < 3_600_000) text = `${Math.round(abs / 60_000)} min`;
  else if (abs < 86_400_000) text = `${Math.round(abs / 3_600_000)} h`;
  else text = `${Math.round(abs / 86_400_000)} d`;
  return future ? `in ${text}` : `${text} ago`;
}

export function formatAbsolute(iso: string | null): string {
  if (!iso) return "";
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const d = new Date(t);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${hhmm(d.getHours(), d.getMinutes())}`;
}

/** Last non-blank line of a log tail, for the one-line preview in the table. */
export function lastOutputLine(tail: string): string {
  const lines = tail.split("\n").map((l) => l.trimEnd()).filter((l) => l.trim() !== "");
  return lines[lines.length - 1] ?? "";
}
