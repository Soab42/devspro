import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type AppKind = "process" | "flatpak" | "service";

/** Every category an app can be filed under. The page adds "All" in front. */
export const CATEGORIES = [
  "Browsers",
  "Communication",
  "Dev",
  "Creative",
  "Servers",
  "Background",
  "Other",
] as const;
export type Category = (typeof CATEGORIES)[number];

/** A catalog entry: an app the user told DevsPro how to find, stop and start. */
export interface AppDef {
  /** Stable slug, unique, [a-z0-9-]+. */
  id: string;
  name: string;
  category: string;
  kind: AppKind;
  /** Substring of a process's full command line. */
  matcher: string;
  /** Process kind: a shell command. */
  start: string | null;
  /** Flatpak kind: stopped with `flatpak kill`, started with `flatpak run`. */
  flatpak_id: string | null;
  /** Service kind: a systemd unit (a service name on Windows). Starting and stopping it
   *  asks for the admin password. */
  service: string | null;
}

export interface MemoryInfo {
  total_mb: number;
  available_mb: number;
  used_pct: number;
  swap_total_mb: number;
  swap_used_mb: number;
}

export type AppState = "on" | "idle" | "off";

export interface AppRow {
  /** Catalog id, or "auto:<comm>" for a detected app. */
  id: string;
  name: string;
  category: string;
  kind: AppKind;
  /** Found running, not in the catalog. */
  detected: boolean;
  state: AppState;
  ram_mb: number;
  /** Sum over its processes, % of one core. */
  cpu_pct: number;
  process_count: number;
  can_start: boolean;
  /** Starting or stopping it shows the admin password prompt. */
  needs_admin: boolean;
  /** DevsPro itself or part of the desktop session: never stopped from here. */
  protected: boolean;
}

export interface WatchdogConfig {
  enabled: boolean;
  threshold_pct: number;
  prefer_idle: boolean;
}

export interface WatchdogEvent {
  at_ms: number;
  app: string;
  ram_mb: number;
  ram_pct: number;
  reason: "idle" | "largest";
  ok: boolean;
  message: string | null;
}

/** What this OS can do, so the page leaves out what it can't. */
export interface Support {
  os: "linux" | "windows" | "macos" | string;
  /** Flatpak apps exist (Linux). */
  flatpak: boolean;
  /** Swap can be pulled back into RAM. Not on Windows, which has no way to. */
  reclaim_swap: boolean;
}

export interface MemorySnapshot {
  taken_at_ms: number;
  memory: MemoryInfo;
  apps: AppRow[];
  watchdog: WatchdogConfig;
  support: Support;
}

export interface ActionResult {
  id: string;
  ok: boolean;
  message: string;
}

/** One step of an elevated helper run. */
export interface OpResult {
  success: boolean;
  message: string;
}

export interface ResetTarget {
  pid: number;
  /** /proc/<pid>/stat field 22, so a reused pid is never mistaken for this one. */
  start_time: number;
  name: string;
  command: string;
  ram_mb: number;
  /** The app it belongs to, when it belongs to one. */
  app: string | null;
}

export interface ResetPreview {
  processes: ResetTarget[];
  /** Running catalog services. */
  services: string[];
}

export interface ResetRequest {
  /** [pid, start_time] pairs. */
  processes: [number, number][];
  services: string[];
  drop_caches: boolean;
}

export interface ResetSummary {
  killed: number;
  failed: string[];
  admin: OpResult[];
}

export const memorySnapshot = () => invoke<MemorySnapshot>("memory_snapshot");

export const stopApps = (ids: string[]) => invoke<ActionResult[]>("memory_stop", { ids });

export const startApp = (id: string) => invoke<ActionResult>("memory_start", { id });

export const getApps = () => invoke<AppDef[]>("memory_apps_get");

export const setApps = (apps: AppDef[]) => invoke<void>("memory_apps_set", { apps });

export const getWatchdog = () => invoke<WatchdogConfig>("memory_watchdog_get");

export const setWatchdog = (config: WatchdogConfig) =>
  invoke<WatchdogConfig>("memory_watchdog_set", { config });

export const watchdogLog = () => invoke<WatchdogEvent[]>("memory_watchdog_log");

export const dropCaches = (reclaimSwap: boolean) =>
  invoke<OpResult[]>("memory_drop_caches", { reclaimSwap });

export const resetPreview = () => invoke<ResetPreview>("memory_reset_preview");

export const resetMemory = (request: ResetRequest) =>
  invoke<ResetSummary>("memory_reset", { request });

export const onWatchdog = (handler: (event: WatchdogEvent) => void) =>
  listen<WatchdogEvent>("memory://watchdog", (e) => handler(e.payload));
