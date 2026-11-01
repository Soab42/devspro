import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Backend = "native" | "docker";

/** Who is on the Mailpit ports — see `resolve` in the Rust module. */
export type RunState = "off" | "running" | "external" | "conflict";

export interface MailcatcherStatus {
  state: RunState;
  /** Which of ours is running when `state` is "running"; otherwise the last one chosen. */
  backend: Backend;
  smtp_port: number;
  http_port: number;
  web_url: string | null;
  conflict: string | null;
  native_available: boolean;
  native_downloaded: boolean;
  native_version: string;
  docker_configured: boolean;
  start_with_app: boolean;
  keep_running: boolean;
}

export interface InstallProgress {
  downloaded: number;
  total: number | null;
}

export const getStatus = () => invoke<MailcatcherStatus>("mailcatcher_status");
export const start = (backend: Backend, smtpPort: number, httpPort: number) =>
  invoke<MailcatcherStatus>("mailcatcher_start", { backend, smtpPort, httpPort });
export const stop = () => invoke<MailcatcherStatus>("mailcatcher_stop");
export const removeContainer = () => invoke<MailcatcherStatus>("mailcatcher_remove_container");
export const install = () => invoke<MailcatcherStatus>("mailcatcher_install");
export const uninstall = (deleteMail: boolean) =>
  invoke<MailcatcherStatus>("mailcatcher_uninstall", { deleteMail });
export const setOptions = (startWithApp: boolean, keepRunning: boolean) =>
  invoke<MailcatcherStatus>("mailcatcher_set_options", { startWithApp, keepRunning });

export const onInstallProgress = (handler: (p: InstallProgress) => void) =>
  listen<InstallProgress>("mailcatcher://install-progress", (e) => handler(e.payload));

// Reused from the nginx/SSL module's generic free-port scanner — same command
// the Databases module's create form already relies on, no second implementation
// of "is this port free" needed here.
export const suggestPort = (start: number) => invoke<number | null>("nginx_ssl_suggest_port", { start });
