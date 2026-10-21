import { invoke } from "@tauri-apps/api/core";

export interface CpuInfo {
  model: string;
  core_count: number;
  usage_percent: number;
  per_core_percent: number[];
  /** `null` where the OS has no load average to report — Windows tracks nothing
   *  equivalent, and a zero there would read as a completely idle machine. */
  load_avg_1: number | null;
  load_avg_5: number | null;
  load_avg_15: number | null;
}

export interface MemoryInfo {
  total_kb: number;
  used_kb: number;
  available_kb: number;
  swap_total_kb: number;
  swap_used_kb: number;
}

export interface DiskInfo {
  device: string;
  mount_point: string;
  fs_type: string;
  total_bytes: number;
  used_bytes: number;
  available_bytes: number;
}

export interface NetworkInterfaceInfo {
  name: string;
  rx_bytes_total: number;
  tx_bytes_total: number;
  rx_bytes_per_sec: number;
  tx_bytes_per_sec: number;
}

export interface GpuInfo {
  name: string;
  driver: string | null;
  usage_percent: number | null;
  memory_used_mb: number | null;
  memory_total_mb: number | null;
  temperature_c: number | null;
}

export interface SystemSnapshot {
  hostname: string;
  os_name: string;
  os_version: string;
  kernel_version: string;
  uptime_seconds: number;
  cpu: CpuInfo;
  memory: MemoryInfo;
  disks: DiskInfo[];
  network: NetworkInterfaceInfo[];
  gpus: GpuInfo[];
}

export const fetchSystemSnapshot = () => invoke<SystemSnapshot>("system_info_snapshot");
