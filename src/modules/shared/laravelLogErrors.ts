import { invoke } from "@tauri-apps/api/core";

export interface LaravelLogEntry {
  /** `YYYY-MM-DD HH:MM:SS`. */
  at: string;
  level: string;
  message: string;
  /** The whole entry, stack trace included. */
  text: string;
  /** Relative to the project, e.g. `storage/logs/laravel.log`. */
  file: string;
}

/** Error-level entries the project's Laravel log has between `from` and `to`. */
export const fetchLaravelLogErrors = (workingDir: string, from: string, to: string) =>
  invoke<LaravelLogEntry[]>("laravel_log_errors", { workingDir, from, to });
