import { invoke } from "@tauri-apps/api/core";

export interface Worker {
  id: string;
  name: string;
  command: string;
  working_dir: string;
  site_id: string | null;
  created_at: string;
}

export interface WorkerView extends Worker {
  running: boolean;
}

export const listWorkers = () => invoke<WorkerView[]>("workers_list");

export const createWorker = (name: string, command: string, workingDir: string | null, siteId: string | null) =>
  invoke<Worker>("workers_create", { name, command, workingDir, siteId });

export const updateWorker = (
  id: string,
  name: string,
  command: string,
  workingDir: string | null,
  siteId: string | null
) => invoke<Worker>("workers_update", { id, name, command, workingDir, siteId });

export const deleteWorker = (id: string) => invoke<void>("workers_delete", { id });

export const startWorker = (id: string) => invoke<WorkerView>("workers_start", { id });
export const stopWorker = (id: string) => invoke<WorkerView>("workers_stop", { id });
export const restartWorker = (id: string) => invoke<WorkerView>("workers_restart", { id });

export const fetchWorkerLogs = (id: string, tail: number) => invoke<string>("workers_logs", { id, tail });
