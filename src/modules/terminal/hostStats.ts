import type { HostSample } from "../ssh/api";

/** What the status strip shows, derived from two consecutive samples. */
export interface HostStats {
  hostname: string;
  /** null until a second sample gives a delta. */
  cpuPercent: number | null;
  /** null for this machine on Windows, which keeps no load average. */
  load1: number | null;
  memUsedKb: number;
  memTotalKb: number;
  memPercent: number;
  swapUsedKb: number;
  swapTotalKb: number;
  rxPerSec: number | null;
  txPerSec: number | null;
  uptimeSeconds: number;
  /** null for this machine on Windows. */
  users: number | null;
  disks: HostSample["disks"];
}

/**
 * Rates and percentages from raw counters. `dtSeconds` is the wall-clock gap
 * between the samples; a counter that went backwards (reboot, interface
 * reset) yields null for that tick rather than a huge negative rate.
 */
export function deriveStats(prev: HostSample | null, cur: HostSample, dtSeconds: number): HostStats {
  const usable = prev !== null && dtSeconds > 0;
  const delta = (a: number, b: number) => (usable && b >= a ? b - a : null);

  const cpuDeltaTotal = usable ? delta(prev.cpu_total, cur.cpu_total) : null;
  const cpuDeltaIdle = usable ? delta(prev.cpu_idle, cur.cpu_idle) : null;
  const cpuPercent =
    cpuDeltaTotal !== null && cpuDeltaIdle !== null && cpuDeltaTotal > 0
      ? Math.max(0, Math.min(100, ((cpuDeltaTotal - cpuDeltaIdle) / cpuDeltaTotal) * 100))
      : null;

  const rx = usable ? delta(prev.rx_bytes, cur.rx_bytes) : null;
  const tx = usable ? delta(prev.tx_bytes, cur.tx_bytes) : null;

  const memUsedKb = Math.max(0, cur.mem_total_kb - cur.mem_available_kb);
  return {
    hostname: cur.hostname,
    cpuPercent,
    load1: cur.load1,
    memUsedKb,
    memTotalKb: cur.mem_total_kb,
    memPercent: cur.mem_total_kb > 0 ? (memUsedKb / cur.mem_total_kb) * 100 : 0,
    swapUsedKb: Math.max(0, cur.swap_total_kb - cur.swap_free_kb),
    swapTotalKb: cur.swap_total_kb,
    rxPerSec: rx === null ? null : rx / dtSeconds,
    txPerSec: tx === null ? null : tx / dtSeconds,
    uptimeSeconds: cur.uptime_seconds,
    users: cur.users,
    disks: cur.disks,
  };
}
