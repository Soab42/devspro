import { invoke } from "@tauri-apps/api/core";

export interface DockerDetection {
  found: boolean;
  daemon_reachable: boolean;
  version: string | null;
  error: string | null;
}

export interface MailcatcherStatus {
  configured: boolean;
  running: boolean;
  state: string | null;
  smtp_port: number | null;
  http_port: number | null;
  web_url: string | null;
}

export const DEFAULT_SMTP_PORT = 1025;
export const DEFAULT_HTTP_PORT = 8025;

export const detectDocker = () => invoke<DockerDetection>("mailcatcher_detect_docker");
export const getStatus = () => invoke<MailcatcherStatus>("mailcatcher_status");
export const start = (smtpPort: number, httpPort: number) =>
  invoke<MailcatcherStatus>("mailcatcher_start", { smtpPort, httpPort });
export const stop = () => invoke<MailcatcherStatus>("mailcatcher_stop");
export const remove = () => invoke<MailcatcherStatus>("mailcatcher_remove");

// Reused from the nginx/SSL module's generic free-port scanner — same command
// the Databases module's create form already relies on, no second implementation
// of "is this port free" needed here.
export const suggestPort = (start: number) => invoke<number | null>("nginx_ssl_suggest_port", { start });
