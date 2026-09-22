import { invoke } from "@tauri-apps/api/core";

export interface ScheduledTask {
  id: string;
  name: string;
  /** 5-field cron expression, normalised to single spaces by the backend. */
  schedule: string;
  command: string;
  working_dir: string;
  site_id: string | null;
  enabled: boolean;
  created_at: string;
}

export interface ScheduledTaskView extends ScheduledTask {
  description: string;
  /** RFC 3339; null when disabled or when the expression never fires. */
  next_run: string | null;
  /** RFC 3339, from the log file's mtime; null before the first run. */
  last_run_at: string | null;
  last_output_tail: string;
}

export interface ScheduledTaskInput {
  id: string | null;
  name: string;
  schedule: string;
  command: string;
  workingDir: string | null;
  siteId: string | null;
  enabled: boolean;
}

export const listScheduledTasks = () => invoke<ScheduledTaskView[]>("scheduled_list");

export const saveScheduledTask = (input: ScheduledTaskInput) =>
  invoke<ScheduledTaskView>("scheduled_save", { ...input });

export const deleteScheduledTask = (id: string) => invoke<void>("scheduled_delete", { id });

export const setScheduledTaskEnabled = (id: string, enabled: boolean) =>
  invoke<ScheduledTaskView>("scheduled_set_enabled", { id, enabled });

export const runScheduledTaskNow = (id: string) => invoke<void>("scheduled_run_now", { id });

export const fetchScheduledTaskLogs = (id: string, tail: number) =>
  invoke<string>("scheduled_logs", { id, tail });
