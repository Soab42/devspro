import { useCallback, useEffect, useSyncExternalStore } from "react";
import { getSetting, setSetting } from "../settings/api";
import { isUnder } from "./utils";

/**
 * The files open as tabs beside the folder view, editor-style: which ones, in
 * what order, which is in front, and which hold unsaved edits.
 *
 * Kept outside the page so leaving the File Manager for another module and
 * coming back finds the same tabs, with unsaved text intact. The tab list (not
 * the edits) is also saved in settings, so it survives a restart.
 */

export interface OpenFile {
  path: string;
  /** What the tab says. The file's title on a Drive mount, where the path is an ID. */
  name: string;
}

export interface OpenFiles {
  files: OpenFile[];
  /** The tab in front; null is the folder view. */
  active: string | null;
  /** Tabs with unsaved edits. Never saved. */
  dirty: string[];
}

export const NO_OPEN_FILES: OpenFiles = { files: [], active: null, dirty: [] };

export const OPEN_FILES_KEY = "fm.open-files";


/** Brings `file` to the front, opening it beside the current tab if it is not open yet. */
export function openTab(s: OpenFiles, file: OpenFile): OpenFiles {
  if (s.files.some((f) => f.path === file.path)) return { ...s, active: file.path };
  const at = s.active === null ? s.files.length : s.files.findIndex((f) => f.path === s.active) + 1;
  const files = [...s.files.slice(0, at), file, ...s.files.slice(at)];
  return { ...s, files, active: file.path };
}

/** Closes one tab; when it was in front, the tab that slides into its place takes over. */
export function closeTab(s: OpenFiles, path: string): OpenFiles {
  const i = s.files.findIndex((f) => f.path === path);
  if (i < 0) return s;
  const files = s.files.filter((f) => f.path !== path);
  const active = s.active === path ? (files[i] ?? files[i - 1])?.path ?? null : s.active;
  return { files, active, dirty: s.dirty.filter((p) => p !== path) };
}

/** A dragged tab dropped before another, or at the end when `before` is null. */
export function moveTab(s: OpenFiles, path: string, before: string | null): OpenFiles {
  const file = s.files.find((f) => f.path === path);
  if (!file || path === before) return s;
  const rest = s.files.filter((f) => f.path !== path);
  const at = before === null ? rest.length : rest.findIndex((f) => f.path === before);
  if (at < 0) return s;
  return { ...s, files: [...rest.slice(0, at), file, ...rest.slice(at)] };
}

/** Ctrl+Tab: the next tab round the ring, the folder view included. */
export function cycleTab(s: OpenFiles, delta: 1 | -1): OpenFiles {
  const ring = [null, ...s.files.map((f) => f.path)];
  const i = ring.indexOf(s.active);
  return { ...s, active: ring[(i + delta + ring.length) % ring.length] };
}

/** Follows a rename or a move: tabs on `from`, or anything under it, now point under `to`. */
export function retargetTabs(s: OpenFiles, from: string, to: string, name?: string): OpenFiles {
  const move = (p: string) => (isUnder(p, from) ? to + p.slice(from.length) : p);
  if (!s.files.some((f) => isUnder(f.path, from))) return s;
  return {
    files: s.files.map((f) => ({ path: move(f.path), name: f.path === from && name ? name : f.name })),
    active: s.active === null ? null : move(s.active),
    dirty: s.dirty.map(move),
  };
}

/** Closes every tab on or under the given paths — they were moved to the trash. */
export function closeTabsUnder(s: OpenFiles, roots: string[]): OpenFiles {
  return s.files.filter((f) => roots.some((r) => isUnder(f.path, r))).reduce((acc, f) => closeTab(acc, f.path), s);
}

export function setDirty(s: OpenFiles, path: string, dirty: boolean): OpenFiles {
  const has = s.dirty.includes(path);
  if (has === dirty) return s;
  return { ...s, dirty: dirty ? [...s.dirty, path] : s.dirty.filter((p) => p !== path) };
}

export function parseOpenFiles(raw: string | null): OpenFiles {
  if (!raw) return NO_OPEN_FILES;
  try {
    const v = JSON.parse(raw) as { files?: unknown; active?: unknown };
    const files = Array.isArray(v.files)
      ? v.files
          .filter((f): f is OpenFile => typeof f === "object" && f !== null && typeof (f as OpenFile).path === "string" && typeof (f as OpenFile).name === "string")
          .map((f) => ({ path: f.path, name: f.name }))
      : [];
    const active = typeof v.active === "string" && files.some((f) => f.path === v.active) ? v.active : null;
    return { files, active, dirty: [] };
  } catch {
    return NO_OPEN_FILES;
  }
}

/* ---- the store ---------------------------------------------------------- */

let state: OpenFiles = NO_OPEN_FILES;
let restored: Promise<void> | null = null;
let touched = false;
let saveTimer: number | undefined;
const listeners = new Set<() => void>();

function emit(next: OpenFiles) {
  if (next === state) return;
  const listChanged = next.files !== state.files || next.active !== state.active;
  state = next;
  listeners.forEach((l) => l());
  if (!listChanged) return;
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    setSetting(OPEN_FILES_KEY, JSON.stringify({ files: state.files, active: state.active })).catch(() => undefined);
  }, 300);
}

function restore() {
  restored ??= getSetting(OPEN_FILES_KEY)
    .then((raw) => {
      // Something opened before the saved list arrived wins over it.
      if (!touched) emit(parseOpenFiles(raw));
    })
    .catch(() => undefined);
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

/** The open tabs, and the one way to change them. */
export function useOpenFiles(): [OpenFiles, (change: (s: OpenFiles) => OpenFiles) => void] {
  const current = useSyncExternalStore(subscribe, () => state);
  useEffect(restore, []);
  const update = useCallback((change: (s: OpenFiles) => OpenFiles) => {
    touched = true;
    emit(change(state));
  }, []);
  return [current, update];
}

/* ---- unsaved text --------------------------------------------------------- */

/**
 * Edits not yet saved, by path, so a text tab that unmounts (the page was
 * left) comes back with them. Only dirty buffers live here; a clean tab
 * re-reads the file when it mounts.
 */
export const drafts = new Map<string, { original: string; contents: string }>();

export function retargetDrafts(from: string, to: string) {
  for (const [p, d] of [...drafts]) {
    if (!isUnder(p, from)) continue;
    drafts.delete(p);
    drafts.set(to + p.slice(from.length), d);
  }
}
