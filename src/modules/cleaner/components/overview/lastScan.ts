import type { Risk } from "../../api";

export const LAST_SCAN_KEY = "cleaner:lastScan";

export interface LastScan {
  /** RFC3339. */
  at: string;
  /** Bytes the scan found, by risk. */
  totals: Record<Risk, number>;
}

/** The saved scan, or null when there is none or storage is unavailable or garbled. */
export function readLastScan(): LastScan | null {
  try {
    const raw = localStorage.getItem(LAST_SCAN_KEY);
    if (!raw) return null;
    const v = JSON.parse(raw) as Partial<LastScan> | null;
    if (!v || typeof v.at !== "string" || !v.totals) return null;
    const t = v.totals;
    if ([t.safe, t.review, t.danger].some((n) => typeof n !== "number")) return null;
    return { at: v.at, totals: { safe: t.safe, review: t.review, danger: t.danger } };
  } catch {
    return null;
  }
}

export function writeLastScan(scan: LastScan): void {
  try {
    localStorage.setItem(LAST_SCAN_KEY, JSON.stringify(scan));
  } catch {
    /* Storage full or blocked; the summary just won't persist. */
  }
}

/** "5 minutes ago"; anything unparseable reads as "a while ago". */
export function timeAgo(iso: string, now: number = Date.now()): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return "a while ago";
  const secs = Math.max(0, Math.round((now - then) / 1000));
  if (secs < 60) return "just now";
  const units: [number, string][] = [
    [86400, "day"],
    [3600, "hour"],
    [60, "minute"],
  ];
  for (const [size, name] of units) {
    if (secs >= size) {
      const n = Math.floor(secs / size);
      return `${n} ${name}${n === 1 ? "" : "s"} ago`;
    }
  }
  return "just now";
}
