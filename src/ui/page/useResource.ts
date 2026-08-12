import { useCallback, useEffect, useRef, useState } from "react";

export interface ResourceOptions {
  /** Poll interval in ms. Omit for a single fetch. */
  pollMs?: number;
  /** Set false to hold off fetching until a precondition is met. */
  enabled?: boolean;
}

export interface Resource<T> {
  data: T | null;
  error: string | null;
  /** First load only — see the note below. */
  loading: boolean;
  /** A later load, with data already on screen. */
  refreshing: boolean;
  refresh: () => Promise<void>;
  setData: (next: T) => void;
}

/**
 * Async data with polling.
 *
 * The important rule: `loading` means "we have nothing to show yet"
 * (`data === null`), NOT "a request is in flight". Every polling page in this
 * app previously flipped a single `loading` flag on each tick, so the Dashboard
 * (2s), Workers (5s), and the Sites tunnel poll (2s) tore their content down
 * and rebuilt it several times a minute. Distinguishing the two means the first
 * load shows a skeleton and every subsequent one leaves the table in place.
 */
export function useResource<T>(
  fetcher: () => Promise<T>,
  { pollMs, enabled = true }: ResourceOptions = {},
): Resource<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  // Kept in a ref so `refresh` stays referentially stable and callers can put
  // it in dependency arrays without re-creating their poll interval.
  const fetcherRef = useRef(fetcher);
  fetcherRef.current = fetcher;

  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    setRefreshing(true);
    try {
      const next = await fetcherRef.current();
      if (!mounted.current) return;
      setData(next);
      setError(null);
    } catch (e) {
      if (!mounted.current) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (mounted.current) setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    if (!enabled) return;
    void refresh();
    if (!pollMs) return;
    const id = setInterval(() => void refresh(), pollMs);
    return () => clearInterval(id);
  }, [enabled, pollMs, refresh]);

  return {
    data,
    error,
    loading: data === null && enabled,
    refreshing,
    refresh,
    setData,
  };
}
