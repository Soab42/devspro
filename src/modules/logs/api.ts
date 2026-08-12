import { invoke } from "@tauri-apps/api/core";

export interface LogSource {
  id: string;
  site_name: string;
  hostname: string;
  kind: "access" | "error" | "laravel";
  path: string;
}

export const listLogSources = () => invoke<LogSource[]>("logs_list_sources");
export const tailLog = (sourceId: string, lines: number) =>
  invoke<string>("logs_tail", { sourceId, lines });
