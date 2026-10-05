import { useCallback, useEffect, useRef, useState } from "react";
import { getSetting, setSetting } from "../settings/api";
import type { DiffFilter } from "./envFile";

export type SideName = "left" | "right";

/**
 * A saved comparison: which two files, and how they were being looked at.
 *
 * A side loaded from a file is saved as its path and read from disk again on
 * open, so it always shows the file as it is now. A side that was pasted is
 * saved as its text, but not here: these records live in the settings table,
 * which goes into every backup, and env files are where secrets live. Pasted
 * text is kept apart, on this machine only (see `PASTED_KEY`).
 */
export interface EnvSession {
  name: string;
  left: string | null;
  right: string | null;
  /** Sides saved as pasted text rather than a path. */
  pasted: SideName[];
  keysOnly: boolean;
  syncScroll: boolean;
  filter: DiffFilter;
}

export const SESSIONS_KEY = "env-diff.sessions";
/** localStorage, not settings: what is stored here must stay out of backups. */
export const PASTED_KEY = "devspro.envDiff.pasted";

export type PastedText = Partial<Record<SideName, string>>;

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
      // A side is a path or pasted text, never both.
      const pasted = (["left", "right"] as const).filter(
        (side) => Array.isArray(s.pasted) && s.pasted.includes(side) && !(side === "left" ? left : right),
      );
      if (typeof s.name !== "string" || !s.name.trim()) return [];
      if (!left && !right && pasted.length === 0) return [];
      return [
        {
          name: s.name,
          left,
          right,
          pasted,
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

/* Pasted text, by session name. Storage can be missing or refuse a write (a
   full quota, a locked-down webview), so every access is guarded. */

function readAllPasted(): Record<string, PastedText> {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(PASTED_KEY) ?? "{}");
    return typeof parsed === "object" && parsed !== null ? (parsed as Record<string, PastedText>) : {};
  } catch {
    return {};
  }
}

export function readPasted(name: string): PastedText {
  return readAllPasted()[name] ?? {};
}

/** Stores `text` for the session `name`, or forgets it when there is none. */
export function writePasted(name: string, text: PastedText | null): void {
  const all = readAllPasted();
  if (text && Object.keys(text).length > 0) all[name] = text;
  else delete all[name];
  localStorage.setItem(PASTED_KEY, JSON.stringify(all));
}

/** Saved sessions, loaded once and written back whole on every change. */
export function useEnvSessions() {
  const [list, setList] = useState<EnvSession[]>([]);
  // The list a change builds on. Kept in a ref, and changes wait for the first
  // load: a save that raced it would otherwise write a list missing every
  // session already stored, and then have the load overwrite the save.
  const latest = useRef<EnvSession[]>([]);
  const loaded = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    let cancelled = false;
    loaded.current = getSetting(SESSIONS_KEY)
      .then((raw) => {
        if (cancelled) return;
        latest.current = parseSessions(raw);
        setList(latest.current);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  // Rejects if the write fails, so the caller can say so; the list in memory
  // has already moved on, and stays usable for the rest of the visit.
  const update = useCallback(async (change: (list: EnvSession[]) => EnvSession[]) => {
    await loaded.current;
    latest.current = change(latest.current);
    setList(latest.current);
    await setSetting(SESSIONS_KEY, JSON.stringify(latest.current));
  }, []);

  const save = useCallback(
    (session: EnvSession, text: PastedText) => {
      // Text first: a session must never be listed with pasted sides it can't fill.
      writePasted(session.name, text);
      return update((current) => withSession(current, session));
    },
    [update],
  );
  const remove = useCallback(
    async (name: string) => {
      await update((current) => current.filter((s) => s.name !== name));
      try {
        writePasted(name, null);
      } catch {
        // Orphaned text is harmless: nothing lists it, and a session saved
        // under the same name later overwrites it.
      }
    },
    [update],
  );

  return { list, save, remove };
}
