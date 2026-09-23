import { invoke } from "@tauri-apps/api/core";
import { listContainers } from "../docker/api";
import { listSites } from "../nginx-ssl/api";
import { listPorts } from "../processes/api";
import { PortClaimsInput, SITE_LISTEN_PORTS } from "./portConflicts";

/**
 * One thermal sensor. `high_c` / `critical_c` are the vendor's own thresholds
 * when the driver publishes them; most do not, so the UI carries defaults.
 */
export interface TemperatureReading {
  label: string;
  celsius: number;
  high_c: number | null;
  critical_c: number | null;
}

export interface CpuInfo {
  model: string;
  core_count: number;
  usage_percent: number;
  per_core_percent: number[];
  load_avg_1: number;
  load_avg_5: number;
  load_avg_15: number;
  /** Package temperature, or the hottest core. Null on hardware with no sensor. */
  temperature: TemperatureReading | null;
  /** Every sensor the CPU's driver exposes — package plus per core. */
  temperatures: TemperatureReading[];
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
  temperature: TemperatureReading | null;
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

/**
 * Everything on this machine that claims a host port, gathered for
 * `findPortConflicts`. Fetched as one resource so the dashboard has a single
 * (slow) poll for it rather than three. Each source is optional: Docker may not
 * be installed and nginx may have no sites, and a missing source must not hide
 * the conflicts the others can still reveal — hence `allSettled`, not `all`.
 */
export const fetchPortClaims = async (): Promise<PortClaimsInput> => {
  const [ports, containers, sites] = await Promise.allSettled([
    listPorts(),
    listContainers(),
    listSites(),
  ]);
  const settled = <T,>(r: PromiseSettledResult<T[]>): T[] => (r.status === "fulfilled" ? r.value : []);
  return {
    listening: settled(ports).map((p) => ({ port: p.port, pid: p.pid, process: p.process_name })),
    containers: settled(containers).map((c) => ({ name: c.name, ports: c.ports, state: c.state })),
    sites: settled(sites).map((s) => ({
      name: s.name,
      ports: s.enabled ? [...SITE_LISTEN_PORTS] : [],
    })),
  };
};
