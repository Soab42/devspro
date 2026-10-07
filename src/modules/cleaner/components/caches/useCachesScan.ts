import { useCallback } from "react";
import { cancelScan, onScanProgress, scanCaches, scanSystem } from "../../api";
import type { Finding, ScanProgress } from "../../api";
import { newScanId } from "../../cleaner";
import { getSlot, setSlot, useTabRestore, useTabSlot } from "../../tabState";

const TAB = "caches";
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export interface CachesScan {
  caches: Finding[];
  system: Finding[];
  /** Why the caches scan failed. */
  error: string | null;
  /** Why the system scan failed; the caches tab still works without it. */
  systemError: string | null;
  /** When the shown results were scanned, epoch ms; null before any scan. */
  scannedAt: number | null;
  scanned: boolean;
  scanning: boolean;
  progress: ScanProgress | null;
  scan: () => Promise<void>;
  cancel: () => void;
}

const set = <T>(key: string, value: T, fallback: T) => setSlot(TAB, key, value, fallback);

/**
 * Runs the caches and system scans together and tracks the caches scan's progress.
 * The state lives in the tab store, so a scan keeps going, and keeps its results,
 * when the tab is left.
 */
export function useCachesScan(): CachesScan {
  useTabRestore(TAB);
  const [caches] = useTabSlot<Finding[]>(TAB, "caches", []);
  const [system] = useTabSlot<Finding[]>(TAB, "system", []);
  const [error] = useTabSlot<string | null>(TAB, "error", null);
  const [systemError] = useTabSlot<string | null>(TAB, "systemError", null);
  const [scannedAt] = useTabSlot<number | null>(TAB, "scannedAt", null);
  const [scanning] = useTabSlot(TAB, "scanning", false);
  const [progress] = useTabSlot<ScanProgress | null>(TAB, "progress", null);

  const scan = useCallback(async () => {
    if (getSlot(TAB, "scanning", false)) return;
    const cachesId = newScanId();
    const systemId = newScanId();
    set("scanIds", [cachesId, systemId], [] as string[]);
    set("scanning", true, false);
    set<ScanProgress | null>("progress", null, null);
    set<string | null>("error", null, null);
    set<string | null>("systemError", null, null);
    const unlisten = await onScanProgress((p) => {
      if (p.scan_id === cachesId) set<ScanProgress | null>("progress", p, null);
    }).catch(() => () => {});
    const [c, s] = await Promise.allSettled([scanCaches(cachesId), scanSystem(systemId)]);
    unlisten();
    set("scanIds", [], [] as string[]);
    if (c.status === "fulfilled") set("caches", c.value, [] as Finding[]);
    else {
      set("caches", [], [] as Finding[]);
      set<string | null>("error", message(c.reason), null);
    }
    if (s.status === "fulfilled") set("system", s.value, [] as Finding[]);
    else {
      set("system", [], [] as Finding[]);
      set<string | null>("systemError", message(s.reason), null);
    }
    set<number | null>("scannedAt", Date.now(), null);
    set("scanning", false, false);
  }, []);

  const cancel = useCallback(() => {
    for (const id of getSlot<string[]>(TAB, "scanIds", [])) void cancelScan(id).catch(() => {});
  }, []);

  return { caches, system, error, systemError, scannedAt, scanned: scannedAt !== null, scanning, progress, scan, cancel };
}
