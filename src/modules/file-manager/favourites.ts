import { useCallback, useEffect, useState } from "react";
import { getSetting, setSetting } from "../settings/api";
import { baseName } from "./utils";

export interface Favourite {
  label: string;
  path: string;
}

export const FAVOURITES_KEY = "fm.favourites";

export function parseFavourites(raw: string | null): Favourite[] {
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((f): f is Favourite => typeof f === "object" && f !== null && typeof (f as Favourite).path === "string")
      .map((f) => ({ path: f.path, label: typeof f.label === "string" && f.label ? f.label : baseName(f.path) || f.path }));
  } catch {
    return [];
  }
}

/**
 * Folders the user pinned to the sidebar, kept in the settings table so they
 * follow the app rather than the browser profile. Defaults are the Places;
 * these sit above them.
 */
export function useFavourites() {
  const [list, setList] = useState<Favourite[]>([]);

  useEffect(() => {
    let cancelled = false;
    getSetting(FAVOURITES_KEY)
      .then((raw) => !cancelled && setList(parseFavourites(raw)))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  const save = useCallback((next: Favourite[]) => {
    setList(next);
    setSetting(FAVOURITES_KEY, JSON.stringify(next)).catch(() => undefined);
  }, []);

  const has = useCallback((path: string) => list.some((f) => f.path === path), [list]);
  const add = useCallback(
    (path: string) => {
      if (list.some((f) => f.path === path)) return;
      save([...list, { path, label: baseName(path) || path }]);
    },
    [list, save],
  );
  const remove = useCallback((path: string) => save(list.filter((f) => f.path !== path)), [list, save]);
  const rename = useCallback((path: string, label: string) => save(list.map((f) => (f.path === path ? { ...f, label: label.trim() || f.label } : f))), [list, save]);

  return { list, has, add, remove, rename };
}
