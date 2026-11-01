import { useCallback, useEffect, useState } from "react";

/**
 * State of the docked terminal: open or closed, and how wide.
 *
 * A window-local UI preference, so it lives in localStorage beside the
 * sidebar pin and the zoom level (not in the `settings` table, which holds
 * what should travel with a backup). Storage can throw; the dock then just
 * isn't remembered.
 */
export const DOCK_OPEN_KEY = "devspro:terminal-dock-open";
export const DOCK_WIDTH_KEY = "devspro:terminal-dock-width";
export const DOCK_MIN = 280;
export const DOCK_DEFAULT = 480;
/** The dock never takes more than this share of the window; the module keeps the rest. */
export const DOCK_MAX_PERCENT = 70;

export function dockMax(windowWidth: number): number {
  return Math.max(DOCK_MIN, Math.floor((windowWidth * DOCK_MAX_PERCENT) / 100));
}

export function clampDock(width: number, windowWidth: number): number {
  if (!Number.isFinite(width)) return Math.min(DOCK_DEFAULT, dockMax(windowWidth));
  return Math.min(dockMax(windowWidth), Math.max(DOCK_MIN, Math.round(width)));
}

function readOpen(): boolean {
  try {
    return localStorage.getItem(DOCK_OPEN_KEY) === "1";
  } catch {
    return false;
  }
}

function readWidth(): number {
  try {
    const raw = Number(localStorage.getItem(DOCK_WIDTH_KEY));
    return raw > 0 ? raw : DOCK_DEFAULT;
  } catch {
    return DOCK_DEFAULT;
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Not remembered across restarts; the dock still works for this session.
  }
}

/** Ctrl+` (Cmd+` on macOS), the editor convention for a terminal panel. */
export function isDockShortcut(e: { key: string; code?: string; ctrlKey: boolean; metaKey?: boolean; shiftKey?: boolean; altKey?: boolean }): boolean {
  if (!(e.ctrlKey || e.metaKey) || e.shiftKey || e.altKey) return false;
  return e.key === "`" || e.code === "Backquote";
}

export function useTerminalDock(enabled = true) {
  const [open, setOpenState] = useState(readOpen);
  const [width, setWidthState] = useState(readWidth);

  const setOpen = useCallback((next: boolean) => {
    setOpenState(next);
    write(DOCK_OPEN_KEY, next ? "1" : "0");
  }, []);

  const setWidth = useCallback((next: number) => {
    const clamped = clampDock(next, window.innerWidth);
    setWidthState(clamped);
    write(DOCK_WIDTH_KEY, String(clamped));
  }, []);

  const toggle = useCallback(() => setOpen(!open), [open, setOpen]);

  // Capture phase: xterm swallows keys it has focus for, and this one has to
  // work from inside the terminal, to get back out of it.
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (!isDockShortcut(e)) return;
      e.preventDefault();
      e.stopPropagation();
      toggle();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [enabled, toggle]);

  return { open, width, setOpen, setWidth, toggle };
}
