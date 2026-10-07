import type { Finding, Risk } from "./api";

export const RISKS: Risk[] = ["safe", "review", "danger"];

export const RISK_LABEL: Record<Risk, string> = { safe: "Safe", review: "Review", danger: "Danger" };

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${Math.max(0, Math.round(bytes))} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[unit]}`;
}

/** Bytes found per risk level. Blocked findings can't be cleaned, so they don't count. */
export function totalsByRisk(findings: Finding[]): Record<Risk, number> {
  const totals: Record<Risk, number> = { safe: 0, review: 0, danger: 0 };
  for (const f of findings) if (!f.blocked_by) totals[f.risk] += f.bytes;
  return totals;
}

/** The ids the scan pre-ticked, leaving out anything blocked. */
export function suggestedIds(findings: Finding[]): string[] {
  return findings.filter((f) => f.suggested && !f.blocked_by).map((f) => f.id);
}

export interface FindingGroup {
  group: string;
  bytes: number;
  findings: Finding[];
}

/** By group, the group with the most bytes first; findings largest first inside. */
export function groupFindings(findings: Finding[]): FindingGroup[] {
  const groups = new Map<string, FindingGroup>();
  for (const f of findings) {
    const g = groups.get(f.group) ?? { group: f.group, bytes: 0, findings: [] };
    g.bytes += f.bytes;
    g.findings.push(f);
    groups.set(f.group, g);
  }
  const out = [...groups.values()];
  for (const g of out) g.findings.sort((a, b) => b.bytes - a.bytes);
  return out.sort((a, b) => b.bytes - a.bytes || a.group.localeCompare(b.group));
}

let counter = 0;

/** A fresh id for one scan, so progress events and cancel find it. */
export function newScanId(): string {
  counter += 1;
  const rand =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID()
      : Math.random().toString(36).slice(2);
  return `scan-${counter}-${rand}`;
}
