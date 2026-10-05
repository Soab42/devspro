import { useCallback, useEffect, useState } from "react";
import { getSetting, setSetting } from "../settings/api";
import type { DiffFilter } from "./envFile";

/**
 * A saved comparison: which two files, and how they were being looked at.
 *
 * Paths only, never contents. Env files are where secrets live, and these are
 * kept in the settings table, which goes into backups. Reading the files afresh
 * on open also means a session shows them as they are now, not as they were. A
 * side that was pasted rather than loaded from a file is saved as empty.
 */
export interface EnvSession {
  name: string;
  left: string | null;
  right: string | null;
  keysOnly: boolean;
  syncScroll: boolean;
  filter: DiffFilter;
}

export const SESSIONS_KEY = "env-diff.sessions";

const FILTERS: readonly DiffFilter[] = [
  "all",
  "diff",
  "same",
  "changed",
  "commented",
  "only-left",
  "only-right",
];

const pathOrNull = (value: unknown) => (typeof value === "string" && value ? value : null);

export function parseSessions(raw: string | null | undefined): EnvSession[] {
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.flatMap((item): EnvSession[] => {
      if (typeof item !== "object" || item === null) return [];
      const s = item as Record<string, unknown>;
      const left = pathOrNull(s.left);
      const right = pathOrNull(s.right);
      if (typeof s.name !== "string" || !s.name.trim() || (!left && !right)) return [];
      return [
        {
          name: s.name,
          left,
          right,
          keysOnly: s.keysOnly === true,
          syncScroll: s.syncScroll !== false,
          filter: FILTERS.includes(s.filter as DiffFilter) ? (s.filter as DiffFilter) : "diff",
        },
      ];
    });
  } catch {
    return [];
  }
}

/** The list with `session` added, replacing any of the same name, in name order. */
export function withSession(list: EnvSession[], session: EnvSession): EnvSession[] {
  return [...list.filter((s) => s.name !== session.name), session].sort((a, b) =>
    a.name.localeCompare(b.name),
  );
}

/**
 * A default name from the two paths. Always parent/name, since the usual pair
 * is two `.env` files from two checkouts, or `.env` beside `.env.example`.
 */
export function suggestName(left: string | null, right: string | null): string {
  const label = (path: string | null) => path?.split(/[\\/]/).filter(Boolean).slice(-2).join("/");
  return [label(left), label(right)].filter(Boolean).join(" vs ");
}

/** Saved sessions, loaded once and written back whole on every change. */
export function useEnvSessions() {
  const [list, setList] = useState<EnvSession[]>([]);

  useEffect(() => {
    let cancelled = false;
    getSetting(SESSIONS_KEY)
      .then((raw) => !cancelled && setList(parseSessions(raw)))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  // Rejects if the write fails, so the caller can say so; the list in memory
  // has already moved on, and stays usable for the rest of the visit.
  const persist = useCallback((next: EnvSession[]) => {
    setList(next);
    return setSetting(SESSIONS_KEY, JSON.stringify(next));
  }, []);

  const save = useCallback((session: EnvSession) => persist(withSession(list, session)), [list, persist]);
  const remove = useCallback(
    (name: string) => persist(list.filter((s) => s.name !== name)),
    [list, persist],
  );

  return { list, save, remove };
}
