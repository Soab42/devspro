import type {
  AppDef,
  AppKind,
  AppRow,
  AppState,
  ResetTarget,
  Support,
  WatchdogConfig,
  WatchdogEvent,
} from "./api";

/** Megabytes as people read them: GB from 1 GB up, whole MB below that. */
export function formatMb(mb: number): string {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`;
  return `${Math.max(0, Math.round(mb))} MB`;
}

export function formatCpu(pct: number): string {
  return `${pct.toFixed(1)}%`;
}

export const STATE_LABEL: Record<AppState, string> = { on: "On", idle: "Idle", off: "Off" };

/** Running first, then idle, then stopped: killer's "Active" order. */
export const STATE_RANK: Record<AppState, number> = { on: 0, idle: 1, off: 2 };

export const KIND_LABEL: Record<AppKind, string> = {
  process: "Process",
  flatpak: "Flatpak",
  service: "Service",
};

export const isRunning = (row: AppRow) => row.state !== "off";

/** Running and not part of DevsPro or the desktop session. */
export const canStop = (row: AppRow) => isRunning(row) && !row.protected;

export const ADMIN_NOTE = "This asks for your admin password.";

/** Everything available: what the page assumes until a snapshot says otherwise. */
export const FULL_SUPPORT: Support = { os: "linux", flatpak: true, reclaim_swap: true };

export const isWindows = (support: Support) => support.os === "windows";

export function processCount(n: number): string {
  return n === 1 ? "1 process" : `${n} processes`;
}

export function watchdogStatus(config: WatchdogConfig): string {
  return config.enabled
    ? `Watchdog on — closes an app at ${config.threshold_pct}%`
    : "Watchdog off";
}

/** A time of day for today, with the date in front for anything older. */
export function formatWhen(ms: number, now: number = Date.now()): string {
  const at = new Date(ms);
  const time = at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (at.toDateString() === new Date(now).toDateString()) return time;
  return `${at.toLocaleDateString([], { month: "short", day: "numeric" })}, ${time}`;
}

export function watchdogMessage(event: WatchdogEvent): string {
  const what = `${event.app} (${formatMb(event.ram_mb)})`;
  if (event.ok) return `Watchdog closed ${what} — RAM was ${event.ram_pct}%`;
  const why = event.message ? `: ${event.message}` : "";
  return `Watchdog couldn't close ${what}${why} — RAM was ${event.ram_pct}%`;
}

/* ---------------------------------------------------------- full reset -- */

export const targetKey = (t: Pick<ResetTarget, "pid" | "start_time">) => `${t.pid}:${t.start_time}`;

export interface TargetGroup {
  key: string;
  label: string;
  targets: ResetTarget[];
  ram_mb: number;
}

/** Processes grouped by the app they belong to, largest first. A process with
    no app is its own group, by name, so twenty `node`s still read as one line. */
export function groupTargets(targets: ResetTarget[]): TargetGroup[] {
  const groups = new Map<string, TargetGroup>();
  for (const t of targets) {
    const label = t.app ?? t.name;
    const group = groups.get(label) ?? { key: label, label, targets: [], ram_mb: 0 };
    group.targets.push(t);
    group.ram_mb += t.ram_mb;
    groups.set(label, group);
  }
  return [...groups.values()].sort((a, b) => b.ram_mb - a.ram_mb || a.label.localeCompare(b.label));
}

/* ------------------------------------------------------------- catalog -- */

/** systemd unit (or Windows service) names, as the elevated helper accepts them. */
export const UNIT_PATTERN = /^[A-Za-z0-9@._:-]{1,128}$/;

export function slugify(name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug || "app";
}

/** A slug for `name` that no other entry uses. */
export function uniqueId(name: string, taken: Iterable<string>): string {
  const used = new Set(taken);
  const base = slugify(name);
  if (!used.has(base)) return base;
  for (let n = 2; ; n++) {
    const candidate = `${base}-${n}`;
    if (!used.has(candidate)) return candidate;
  }
}

export type AppErrors = Partial<Record<"name" | "matcher" | "flatpak_id" | "service", string>>;

export function validateApp(app: AppDef): AppErrors {
  const errors: AppErrors = {};
  if (!app.name.trim()) errors.name = "Give it a name.";
  if (!app.matcher.trim()) errors.matcher = "Needed to find its processes.";
  if (app.kind === "flatpak" && !app.flatpak_id?.trim()) {
    errors.flatpak_id = "Needed to stop and start it.";
  }
  if (app.kind === "service") {
    const unit = app.service?.trim() ?? "";
    if (!unit) errors.service = "Needed to stop and start it.";
    else if (!UNIT_PATTERN.test(unit)) {
      errors.service = "Letters, digits and @ . _ : - only.";
    }
  }
  return errors;
}

/** Trims every field and clears the ones its kind doesn't use. */
export function cleanApp(app: AppDef): AppDef {
  const value = (s: string | null) => (s?.trim() ? s.trim() : null);
  return {
    ...app,
    name: app.name.trim(),
    matcher: app.matcher.trim(),
    start: app.kind === "process" ? value(app.start) : null,
    flatpak_id: app.kind === "flatpak" ? value(app.flatpak_id) : null,
    service: app.kind === "service" ? value(app.service) : null,
  };
}
