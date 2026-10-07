import { useCallback, useRef, useSyncExternalStore } from "react";
import { clearSnapshot, getSnapshot, setSnapshot } from "./api";

/**
 * What each Cleaner tab remembers, outside its component: results, filters,
 * selection and position. Components read from here and write to here, so
 * switching tabs or leaving the module loses nothing, and a scan that is still
 * running when its tab unmounts finishes into the store.
 *
 * The persisted slots are also saved through the backend, so they survive a
 * restart. Selection and in-flight scan state are never saved.
 */

export type TabId = "overview" | "caches" | "docker" | "large-files" | "storage-map" | "projects";
/** "page" holds the Cleaner page's own state and is never saved. */
export type StoreKey = TabId | "page";

type Check = (v: unknown) => boolean;

const isArr: Check = Array.isArray;
const isObj: Check = (v) => typeof v === "object" && v !== null && !Array.isArray(v);
const isStr: Check = (v) => typeof v === "string";
const isNum: Check = (v) => typeof v === "number" && Number.isFinite(v);
const orNull =
  (c: Check): Check =>
  (v) =>
    v === null || c(v);
const isStrArr: Check = (v) => Array.isArray(v) && v.every((x) => typeof x === "string");
const hasFindings: Check = (v) => isObj(v) && Array.isArray((v as { findings?: unknown }).findings);

/** The slots that are saved, each with the check a restored value must pass. */
const PERSISTED: Record<StoreKey, Record<string, Check>> = {
  page: {},
  overview: {
    results: (v) => isObj(v) && Object.values(v as object).every(hasFindings),
    scannedAt: orNull(isNum),
  },
  caches: {
    caches: isArr,
    system: isArr,
    error: orNull(isStr),
    systemError: orNull(isStr),
    scannedAt: orNull(isNum),
  },
  docker: { result: orNull(hasFindings), error: orNull(isStr), scannedAt: orNull(isNum) },
  "large-files": {
    view: isStr,
    minSize: isStr,
    kind: isStr,
    age: isStr,
    files: orNull(isArr),
    scannedFilters: orNull(isObj),
    scannedAt: orNull(isNum),
    foldersRoot: isStr,
    foldersPath: isStr,
  },
  "storage-map": {
    root: isStr,
    tree: orNull((v) => isObj(v) && isObj((v as { root?: unknown }).root)),
    zoomPath: isStr,
    expanded: isStrArr,
    scannedAt: orNull(isNum),
  },
  projects: { rows: orNull(isArr), error: orNull(isStr), scannedAt: orNull(isNum) },
};

/** Largest snapshot the backend accepts; bigger ones stay in memory only. */
export const MAX_SNAPSHOT_CHARS = 8 * 1024 * 1024;
const SAVE_DELAY_MS = 400;
export const STALE_AFTER_MS = 24 * 60 * 60 * 1000;

const slots = new Map<StoreKey, Record<string, unknown>>();
const listeners = new Set<() => void>();
const loads = new Map<StoreKey, Promise<void>>();
const loaded = new Set<StoreKey>();
const timers = new Map<StoreKey, ReturnType<typeof setTimeout>>();
/** Epoch bumps on reset so a late load or save from before it is ignored. */
let epoch = 0;

function notify() {
  for (const l of [...listeners]) l();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function hasSlot(tab: StoreKey, key: string): boolean {
  const s = slots.get(tab);
  return s !== undefined && key in s;
}

export function getSlot<T>(tab: StoreKey, key: string, fallback: T): T {
  const s = slots.get(tab);
  return s && key in s ? (s[key] as T) : fallback;
}

export type Update<T> = T | ((prev: T) => T);

export function setSlot<T>(tab: StoreKey, key: string, update: Update<T>, fallback: T): void {
  const prev = getSlot(tab, key, fallback);
  const next = typeof update === "function" ? (update as (p: T) => T)(prev) : update;
  const s = slots.get(tab) ?? {};
  if (key in s && Object.is(s[key], next)) return;
  slots.set(tab, { ...s, [key]: next });
  if (key in PERSISTED[tab] || key === "scanning") schedulePersist(tab);
  notify();
}

/** One slot as React state. `initial` is only used until something is stored. */
export function useTabSlot<T>(tab: StoreKey, key: string, initial: T): [T, (update: Update<T>) => void] {
  const fallback = useRef(initial).current;
  const value = useSyncExternalStore(
    subscribe,
    () => getSlot(tab, key, fallback),
    () => fallback,
  );
  const set = useCallback((update: Update<T>) => setSlot(tab, key, update, fallback), [tab, key, fallback]);
  return [value, set];
}

/** True once the tab's saved snapshot has been looked at; starts that on first use. */
export function useTabRestore(tab: StoreKey): boolean {
  void loadTab(tab);
  return useSyncExternalStore(
    subscribe,
    () => loaded.has(tab),
    () => true,
  );
}

/** Fills slots nothing has touched yet from the saved snapshot. Never throws. */
export function loadTab(tab: StoreKey): Promise<void> {
  const existing = loads.get(tab);
  if (existing) return existing;
  const mine = epoch;
  const done = (async () => {
    if (Object.keys(PERSISTED[tab]).length > 0) {
      try {
        const json = await getSnapshot(tab);
        if (mine === epoch && typeof json === "string") hydrate(tab, json);
      } catch {
        /* No backend, or an unreadable snapshot: start empty. */
      }
    }
    if (mine !== epoch) return;
    loaded.add(tab);
    notify();
    // Anything done before the snapshot arrived is saved now.
    if (slots.has(tab)) schedulePersist(tab);
  })();
  loads.set(tab, done);
  return done;
}

function hydrate(tab: StoreKey, json: string) {
  let doc: unknown;
  try {
    doc = JSON.parse(json);
  } catch {
    return;
  }
  const saved = isObj(doc) ? (doc as { slots?: unknown }).slots : null;
  if (!isObj(saved)) return;
  const next = { ...(slots.get(tab) ?? {}) };
  for (const [key, check] of Object.entries(PERSISTED[tab])) {
    const v = (saved as Record<string, unknown>)[key];
    if (!(key in next) && v !== undefined && check(v)) next[key] = v;
  }
  slots.set(tab, next);
}

function schedulePersist(tab: StoreKey) {
  if (Object.keys(PERSISTED[tab]).length === 0) return;
  const old = timers.get(tab);
  if (old) clearTimeout(old);
  timers.set(
    tab,
    setTimeout(() => {
      timers.delete(tab);
      void persist(tab);
    }, SAVE_DELAY_MS),
  );
}

async function persist(tab: StoreKey): Promise<void> {
  const mine = epoch;
  await loadTab(tab);
  const s = slots.get(tab);
  // A scan in flight leaves the slots half-updated; the end of the scan saves.
  if (mine !== epoch || !s || s.scanning === true) return;
  const out: Record<string, unknown> = {};
  for (const key of Object.keys(PERSISTED[tab])) if (key in s) out[key] = s[key];
  try {
    const json = JSON.stringify({ v: 1, slots: out });
    if (json.length > MAX_SNAPSHOT_CHARS || !(await setSnapshot(tab, json))) {
      // Too big to keep: drop the older one so a restart does not bring it back.
      await clearSnapshot(tab);
    }
  } catch {
    /* Memory only for now; the next change tries again. */
  }
}

/** Saves whatever is waiting for its delay. Used when the window is closing, and by tests. */
export async function flushSnapshots(): Promise<void> {
  const pending = [...timers.keys()];
  for (const t of timers.values()) clearTimeout(t);
  timers.clear();
  await Promise.all(pending.map((t) => persist(t)));
}

/** Forgets everything in memory. For tests; saved snapshots are untouched. */
export function resetTabState(): void {
  epoch += 1;
  for (const t of timers.values()) clearTimeout(t);
  timers.clear();
  slots.clear();
  loads.clear();
  loaded.clear();
  notify();
}
