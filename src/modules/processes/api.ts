import { invoke } from "@tauri-apps/api/core";

export interface PortEntry {
  port: number;
  address: string;
  pid: number | null;
  process_name: string | null;
  command: string | null;
}

export const listPorts = () => invoke<PortEntry[]>("processes_list_ports");
export const killProcess = (pid: number, force: boolean) => invoke<string>("processes_kill", { pid, force });
