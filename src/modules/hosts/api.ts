import { invoke } from "@tauri-apps/api/core";

export interface HostsEntry {
  /** 1-based line number in the hosts file; the handle for edit and delete. */
  line_no: number;
  ip: string;
  hostnames: string[];
  comment: string | null;
  /** Written by DevsPro (tagged `# devspro`, or inside the nginx sync block). */
  managed: boolean;
}

export interface HostsView {
  entries: HostsEntry[];
  raw: string;
  /** Where the hosts file lives on this OS (`/etc/hosts`, or under System32 on Windows). */
  path: string;
}

export const readHosts = () => invoke<HostsView>("hosts_file_read");

/** Every write returns the re-read file, so callers can replace their data in one step. */
export const addHostsEntry = (ip: string, hostname: string) =>
  invoke<HostsView>("hosts_file_add", { ip, hostname });

export const removeHostsEntry = (lineNo: number) =>
  invoke<HostsView>("hosts_file_remove", { lineNo });

export const updateHostsEntry = (lineNo: number, ip: string, hostnames: string[]) =>
  invoke<HostsView>("hosts_file_update", { lineNo, ip, hostnames });

/** Enabled nginx site domains with no loopback entry in the hosts file. */
export const missingSiteHosts = () => invoke<string[]>("hosts_file_missing_sites");
