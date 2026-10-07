import { useSyncExternalStore } from "react";

/** Allowed UI zoom levels, in percent. */
export const ZOOM_LEVELS = [67, 75, 80, 90, 100, 110, 125, 150, 175, 200] as const;
export const DEFAULT_ZOOM = 100;
export const ZOOM_STORAGE_KEY = "devspro:zoom";
/** Minimum gap between Ctrl+wheel steps, so one trackpad flick is one step. */
export const WHEEL_THROTTLE_MS = 150;

/** Snap any number to the nearest allowed level. */
export function clampZoom(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_ZOOM;
  let best: number = ZOOM_LEVELS[0];
  for (const level of ZOOM_LEVELS) {
    if (Math.abs(level - value) < Math.abs(best - value)) best = level;
  }
  return best;
}

/** The next level up, or the current one at the ceiling. */
export function nextZoom(current: number): number {
  const level = clampZoom(current);
  return ZOOM_LEVELS.find((l) => l > level) ?? level;
}

/** The next level down, or the current one at the floor. */
export function previousZoom(current: number): number {
  const level = clampZoom(current);
  return [...ZOOM_LEVELS].reverse().find((l) => l < level) ?? level;
}

/** Wheel up (negative deltaY) zooms in, like every browser. */
export function wheelZoom(current: number, deltaY: number): number {
  if (deltaY < 0) return nextZoom(current);
  if (deltaY > 0) return previousZoom(current);
  return clampZoom(current);
}

export type ZoomAction = "in" | "out" | "reset";

/**
 * Which zoom command a key press is, if any. Needs Ctrl (or Cmd) and no Alt.
 * The one matcher for both the UI zoom and the Browser's page zoom. The numpad
 * keys match on `code`, since NumLock-off Numpad0 reports key "Insert".
 */
export function zoomKeyAction(e: {
  key: string;
  code?: string;
  ctrlKey: boolean;
  metaKey?: boolean;
  altKey?: boolean;
}): ZoomAction | null {
  if (!(e.ctrlKey || e.metaKey) || e.altKey) return null;
  if (e.key === "+" || e.key === "=" || e.code === "NumpadAdd") return "in";
  if (e.key === "-" || e.key === "_" || e.code === "NumpadSubtract") return "out";
  if (e.key === "0" || e.code === "Numpad0" || e.code === "Digit0") return "reset";
  return null;
}

export function applyAction(current: number, action: ZoomAction): number {
  if (action === "in") return nextZoom(current);
  if (action === "out") return previousZoom(current);
  return DEFAULT_ZOOM;
}

// ---- store -----------------------------------------------------------------

/** Storage can throw (disabled, sandboxed, quota); zoom then just isn't remembered. */
function readStored(): number {
  try {
    const raw = Number(localStorage.getItem(ZOOM_STORAGE_KEY));
    return raw ? clampZoom(raw) : DEFAULT_ZOOM;
  } catch {
    return DEFAULT_ZOOM;
  }
}

function writeStored(value: number) {
  try {
    if (value === DEFAULT_ZOOM) localStorage.removeItem(ZOOM_STORAGE_KEY);
    else localStorage.setItem(ZOOM_STORAGE_KEY, String(value));
  } catch {
    // Not remembered across restarts; the zoom still applies for this session.
  }
}

/** Marks a region whose own zoom keys (the Browser's per-page zoom) win over the UI zoom. */
export const OWN_ZOOM_ATTR = "data-own-zoom-keys";

function inOwnZoomScope(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(`[${OWN_ZOOM_ATTR}]`) !== null;
}

let level = readStored();
const listeners = new Set<(announce: boolean) => void>();

/** The current zoom as a factor (1.1 for 110%), for scaling DOM measurements. */
export function getZoomFactor(): number {
  return level / 100;
}

export function getZoom(): number {
  return level;
}

/**
 * Applies the level to this webview with Tauri's native zoom. The page's CSS
 * pixels then stay logical while the webview renders them larger, which is
 * what makes text, forms and tables all scale with no per-component work. The
 * cost is that DOM measurements are no longer device pixels; the Browser
 * module multiplies by getZoomFactor() before positioning its native views.
 * Imported lazily so the plain vite server and tests never need a Tauri runtime.
 */
async function applyToWebview(percent: number) {
  try {
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    await getCurrentWebview().setZoom(percent / 100);
  } catch {
    // No Tauri runtime (vite dev server, tests).
  }
}

function commit(next: number, announce: boolean, persist: boolean) {
  const changed = next !== level;
  level = next;
  if (persist) writeStored(next);
  void applyToWebview(next);
  if (changed) listeners.forEach((l) => l(announce));
}

/** Set the zoom for this window and, through localStorage, every other one. */
export function setZoom(percent: number) {
  commit(clampZoom(percent), true, true);
}

export function subscribeZoom(listener: (announce: boolean) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useZoom(): { zoom: number; setZoom: (p: number) => void; reset: () => void } {
  const zoom = useSyncExternalStore(
    (cb) => subscribeZoom(() => cb()),
    getZoom,
  );
  return { zoom, setZoom, reset: () => setZoom(DEFAULT_ZOOM) };
}

/**
 * Wires keyboard, wheel and cross-window sync, and applies the stored level.
 * Call once per window. Returns a teardown.
 *
 * The listeners are in the capture phase and stop propagation: xterm turns
 * Ctrl+- into a control byte for the shell and CodeMirror binds some of these
 * keys, so the zoom has to claim them first. The terminal still gets Ctrl+C,
 * Ctrl+wheel in a scrolling terminal is the only wheel behaviour given up.
 */
export function installZoom(): () => void {
  level = readStored();
  void applyToWebview(level);

  const onKey = (e: KeyboardEvent) => {
    const action = zoomKeyAction(e);
    if (!action) return;
    // Inside the Browser, these keys zoom the web page, as they always did.
    if (inOwnZoomScope(e.target)) return;
    e.preventDefault();
    e.stopPropagation();
    setZoom(applyAction(level, action));
  };

  let lastWheel = 0;
  const onWheel = (e: WheelEvent) => {
    if (!(e.ctrlKey || e.metaKey)) return;
    // Non-passive so this stops the webview's own pinch/ctrl-wheel zoom.
    e.preventDefault();
    e.stopPropagation();
    const now = Date.now();
    if (now - lastWheel < WHEEL_THROTTLE_MS) return;
    lastWheel = now;
    setZoom(wheelZoom(level, e.deltaY));
  };

  // Another window changed it: follow, with the indicator.
  const onStorage = (e: StorageEvent) => {
    if (e.key !== ZOOM_STORAGE_KEY && e.key !== null) return;
    commit(readStored(), true, false);
  };

  window.addEventListener("keydown", onKey, true);
  window.addEventListener("wheel", onWheel, { capture: true, passive: false });
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener("keydown", onKey, true);
    window.removeEventListener("wheel", onWheel, true);
    window.removeEventListener("storage", onStorage);
  };
}
