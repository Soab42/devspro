import { useCallback, useEffect, useRef, useState } from "react";
import { getSetting, setSetting } from "../settings/api";

/**
 * Remote folders the user starred, by host: a path on one server means nothing
 * on another. Keyed by the saved host's id, not its address, so editing a
 * host's name or port keeps its favourites. Kept in the settings table, which
 * goes into backups; folder paths are not secret.
 */
export type SftpFavourites = Record<string, string[]>;

export const FAVOURITES_KEY = "sftp.favourites";

/** "/srv/app/" and "/srv/app" are one folder; the root stays "/". */
export function normalisePath(path: string): string {
  return path.trim().replace(/\/+$/, "") || "/";
}

/** Plain code-unit order, so a folder sorts straight above its subfolders. */
const sorted = (paths: string[]) => [...new Set(paths)].sort();

export function parseFavourites(raw: string | null | undefined): SftpFavourites {
  if (!raw) return {};
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return {};
    const out: SftpFavourites = {};
    for (const [host, paths] of Object.entries(parsed)) {
      if (!host || !Array.isArray(paths)) continue;
      // Only absolute paths: a relative one would resolve against wherever
      // the session happened to start.
      const kept = paths
        .filter((p): p is string => typeof p === "string" && p.trim().startsWith("/"))
        .map(normalisePath);
      if (kept.length > 0) out[host] = sorted(kept);
    }
    return out;
  } catch {
    return {};
  }
}

export function withFavourite(all: SftpFavourites, host: string, path: string): SftpFavourites {
  return { ...all, [host]: sorted([...(all[host] ?? []), normalisePath(path)]) };
}

/** The list without `path`; a host left with none is dropped altogether. */
export function withoutFavourite(all: SftpFavourites, host: string, path: string): SftpFavourites {
  const target = normalisePath(path);
  const { [host]: paths = [], ...rest } = all;
  const kept = paths.filter((p) => p !== target);
  return kept.length > 0 ? { ...rest, [host]: kept } : rest;
}

/** Favourite folders for every host, loaded once and written back whole on every change. */
export function useSftpFavourites() {
  const [byHost, setByHost] = useState<SftpFavourites>({});
  // The map a change builds on. Kept in a ref, and changes wait for the first
  // load: a save that raced it would otherwise write a map missing every
  // favourite already stored, and then have the load overwrite the save.
  const latest = useRef<SftpFavourites>({});
  const loaded = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    let cancelled = false;
    loaded.current = getSetting(FAVOURITES_KEY)
      .then((raw) => {
        if (cancelled) return;
        latest.current = parseFavourites(raw);
        setByHost(latest.current);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  // Rejects if the write fails, so the caller can say so; the map in memory
  // has already moved on, and stays usable for the rest of the visit.
  const update = useCallback(async (change: (all: SftpFavourites) => SftpFavourites) => {
    await loaded.current;
    latest.current = change(latest.current);
    setByHost(latest.current);
    await setSetting(FAVOURITES_KEY, JSON.stringify(latest.current));
  }, []);

  const add = useCallback(
    (host: string, path: string) => update((all) => withFavourite(all, host, path)),
    [update],
  );
  const remove = useCallback(
    (host: string, path: string) => update((all) => withoutFavourite(all, host, path)),
    [update],
  );

  return { byHost, add, remove };
}

export type SftpFavouritesStore = ReturnType<typeof useSftpFavourites>;
