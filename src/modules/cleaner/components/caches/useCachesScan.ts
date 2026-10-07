import { useCallback, useEffect, useRef, useState } from "react";
import { cancelScan, onScanProgress, scanCaches, scanSystem } from "../../api";
import type { Finding, ScanProgress } from "../../api";
import { newScanId } from "../../cleaner";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export interface CachesScan {
  caches: Finding[];
  system: Finding[];
  /** Why the caches scan failed. */
  error: string | null;
  /** Why the system scan failed; the caches tab still works without it. */
  systemError: string | null;
  scanned: boolean;
  scanning: boolean;
  progress: ScanProgress | null;
  scan: () => Promise<void>;
  cancel: () => void;
}

/** Runs the caches and system scans together and tracks the caches scan's progress. */
export function useCachesScan(): CachesScan {
  const [caches, setCaches] = useState<Finding[]>([]);
  const [system, setSystem] = useState<Finding[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [systemError, setSystemError] = useState<string | null>(null);
  const [scanned, setScanned] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const running = useRef<string[]>([]);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      for (const id of running.current) void cancelScan(id).catch(() => {});
    };
  }, []);

  const scan = useCallback(async () => {
    const cachesId = newScanId();
    const systemId = newScanId();
    running.current = [cachesId, systemId];
    setScanning(true);
    setProgress(null);
    setError(null);
    setSystemError(null);
    const unlisten = await onScanProgress((p) => {
      if (p.scan_id === cachesId && mounted.current) setProgress(p);
    }).catch(() => () => {});
    const [c, s] = await Promise.allSettled([scanCaches(cachesId), scanSystem(systemId)]);
    unlisten();
    running.current = [];
    if (!mounted.current) return;
    if (c.status === "fulfilled") setCaches(c.value);
    else {
      setCaches([]);
      setError(message(c.reason));
    }
    if (s.status === "fulfilled") setSystem(s.value);
    else {
      setSystem([]);
      setSystemError(message(s.reason));
    }
    setScanned(true);
    setScanning(false);
  }, []);

  const cancel = useCallback(() => {
    for (const id of running.current) void cancelScan(id).catch(() => {});
  }, []);

  return { caches, system, error, systemError, scanned, scanning, progress, scan, cancel };
}
