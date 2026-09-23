import type { MeterSeverity } from "../../ui";
import type { TemperatureReading } from "./api";

export function formatBytes(bytes: number, decimals = 1): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  const exp = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** exp;
  return `${value.toFixed(exp === 0 ? 0 : decimals)} ${units[exp]}`;
}

export function formatKb(kb: number, decimals = 1): string {
  return formatBytes(kb * 1024, decimals);
}

export function formatRate(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`;
}

export function formatUptime(totalSeconds: number): string {
  const days = Math.floor(totalSeconds / 86400);
  const hours = Math.floor((totalSeconds % 86400) / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const parts: string[] = [];
  if (days > 0) parts.push(`${days}d`);
  if (days > 0 || hours > 0) parts.push(`${hours}h`);
  parts.push(`${minutes}m`);
  return parts.join(" ");
}

export function percent(used: number, total: number): number {
  if (total <= 0) return 0;
  return (used / total) * 100;
}

/**
 * Thresholds to judge a sensor against when its driver reports none, which is
 * the common case. GPUs are built to run hotter than CPUs and sit in the
 * eighties under sustained load, so flagging them at the CPU's number would
 * mean a permanent warning on any machine doing actual work.
 */
export interface TempLimits {
  warnAt: number;
  critAt: number;
}

export const CPU_TEMP_LIMITS: TempLimits = { warnAt: 80, critAt: 95 };
export const GPU_TEMP_LIMITS: TempLimits = { warnAt: 85, critAt: 95 };

export function formatCelsius(celsius: number): string {
  return `${Math.round(celsius)}°C`;
}

/**
 * A temperature rendered as a `Meter`: the track runs from 0 °C to the point
 * the chip is in trouble, so a glance at the fill reads as "how much headroom
 * is left". Vendor thresholds win where they exist, but only when they are
 * stricter than ours — Intel's `temp*_max` is usually the throttle point and
 * equal to `temp*_crit`, which on its own would leave nothing between "fine"
 * and "shutting down".
 */
export function tempScale(reading: TemperatureReading, limits: TempLimits) {
  const critAt = Math.min(reading.critical_c ?? limits.critAt, limits.critAt);
  const warnAt = Math.min(reading.high_c ?? limits.warnAt, limits.warnAt, critAt - 5);
  return {
    percent: Math.max(0, Math.min(100, (reading.celsius / critAt) * 100)),
    /* Meter thresholds are percentages of the same track. */
    warningAt: (warnAt / critAt) * 100,
    criticalAt: 100,
    warnAt,
    critAt,
  };
}

export function tempSeverity(reading: TemperatureReading, limits: TempLimits): MeterSeverity {
  const { warnAt, critAt } = tempScale(reading, limits);
  if (reading.celsius >= critAt) return "critical";
  if (reading.celsius >= warnAt) return "warning";
  return "good";
}
