import {
  createContext,
  ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { getSetting, setSetting } from "../modules/settings/api";
import { moduleById, modules, type ModuleDefinition } from "./ModuleRegistry";

/**
 * Which modules the user has switched off.
 *
 * Stored as the DISABLED set, not the enabled one, so a module added in a
 * later release shows up switched on without a migration. The source of truth
 * is the `settings` table (key below) so it travels with backups; localStorage
 * holds a copy purely so the first render already knows the answer instead of
 * flashing every module in and then removing half of them.
 */
export const DISABLED_MODULES_SETTING = "disabled_modules";
const CACHE_KEY = "devspro:disabled-modules";

interface ModulesState {
  /** Registry order, minus anything switched off. */
  enabled: ModuleDefinition[];
  disabled: ReadonlySet<string>;
  isEnabled: (id: string) => boolean;
  /** No-op for core modules: they cannot be switched off. */
  setEnabled: (id: string, enabled: boolean) => Promise<void>;
}

const ModulesContext = createContext<ModulesState | null>(null);

/** Drops unknown ids and core modules, so a stale or hand-edited value can't hide the Dashboard. */
export function sanitiseDisabled(ids: unknown): Set<string> {
  if (!Array.isArray(ids)) return new Set();
  return new Set(
    ids.filter((id): id is string => {
      if (typeof id !== "string") return false;
      const m = moduleById(id);
      return !!m && !m.core;
    }),
  );
}

function parseDisabled(raw: string | null | undefined): Set<string> {
  if (!raw) return new Set();
  try {
    return sanitiseDisabled(JSON.parse(raw));
  } catch {
    return new Set();
  }
}

function readCache(): Set<string> {
  try {
    return parseDisabled(localStorage.getItem(CACHE_KEY));
  } catch {
    return new Set();
  }
}

function writeCache(disabled: ReadonlySet<string>) {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify([...disabled]));
  } catch {
    /* fine: the next launch just reads the database a beat later */
  }
}

export function ModulesProvider({ children }: { children: ReactNode }) {
  const [disabled, setDisabled] = useState<ReadonlySet<string>>(readCache);
  // The latest set, readable synchronously from the toggle handler. React does
  // not promise to run a setState updater synchronously — after the first
  // update the fiber's alternate still carries lanes, so the eager path is
  // skipped and an updater-based "compute next" would run only at render time,
  // too late to know what to persist. That bug dropped every toggle but the
  // first: the switch flipped on screen and nothing reached the database.
  const latest = useRef<ReadonlySet<string>>(disabled);
  // A write that is still in flight when the initial read resolves must win,
  // otherwise a quick toggle after launch gets reverted by stale data.
  const dirty = useRef(false);

  useEffect(() => {
    let cancelled = false;
    getSetting(DISABLED_MODULES_SETTING)
      .then((raw) => {
        if (cancelled || dirty.current) return;
        const fromDb = parseDisabled(raw);
        latest.current = fromDb;
        setDisabled(fromDb);
        writeCache(fromDb);
      })
      .catch(() => {
        /* no backend (plain `vite` dev) — the cached value stands */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const setEnabled = useCallback(async (id: string, enabled: boolean) => {
    const m = moduleById(id);
    if (!m || m.core) return;
    dirty.current = true;
    const next = new Set(latest.current);
    if (enabled) next.delete(id);
    else next.add(id);
    latest.current = next;
    setDisabled(next);
    writeCache(next);
    try {
      await setSetting(DISABLED_MODULES_SETTING, JSON.stringify([...next]));
    } catch (e) {
      console.error(e);
    }
  }, []);

  const value = useMemo<ModulesState>(
    () => ({
      enabled: modules.filter((m) => !disabled.has(m.id)),
      disabled,
      isEnabled: (id) => !disabled.has(id),
      setEnabled,
    }),
    [disabled, setEnabled],
  );

  return <ModulesContext.Provider value={value}>{children}</ModulesContext.Provider>;
}

export function useModules(): ModulesState {
  const ctx = useContext(ModulesContext);
  if (!ctx) throw new Error("useModules must be used within ModulesProvider");
  return ctx;
}
