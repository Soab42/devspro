import { describe, expect, it } from "vitest";
import type { HostSample } from "../ssh/api";
import { deriveStats } from "./hostStats";

const base: HostSample = {
  hostname: "box",
  cpu_idle: 800,
  cpu_total: 1000,
  mem_total_kb: 1000,
  mem_available_kb: 400,
  swap_total_kb: 100,
  swap_free_kb: 90,
  load1: 0.5,
  uptime_seconds: 86400,
  users: 2,
  rx_bytes: 1000,
  tx_bytes: 500,
  disks: [{ mount: "/", total_kb: 100, used_kb: 59, percent: 59 }],
};

describe("deriveStats", () => {
  it("has no rates from a single sample but does have memory and disks", () => {
    const s = deriveStats(null, base, 2);
    expect(s.cpuPercent).toBeNull();
    expect(s.rxPerSec).toBeNull();
    expect(s.memUsedKb).toBe(600);
    expect(s.memPercent).toBe(60);
    expect(s.swapUsedKb).toBe(10);
    expect(s.disks[0].mount).toBe("/");
  });

  it("turns counter deltas into cpu percent and byte rates", () => {
    const cur = { ...base, cpu_idle: 850, cpu_total: 1200, rx_bytes: 5000, tx_bytes: 700 };
    const s = deriveStats(base, cur, 2);
    // 200 ticks, 50 idle → 75% busy.
    expect(s.cpuPercent).toBe(75);
    expect(s.rxPerSec).toBe(2000);
    expect(s.txPerSec).toBe(100);
  });

  it("treats a counter that went backwards as unknown for that tick", () => {
    const cur = { ...base, cpu_idle: 10, cpu_total: 20, rx_bytes: 10 };
    const s = deriveStats(base, cur, 2);
    expect(s.cpuPercent).toBeNull();
    expect(s.rxPerSec).toBeNull();
    expect(s.txPerSec).toBe(0);
  });
});
