import { useSyncExternalStore } from "react";
import { CUSTOM_DEVICES_KEY, CustomDevice, Skin, Viewport, isSkin, parseCustomDevices } from "./utils";

export interface Tab {
  id: string;
  url: string;
  title: string;
  loading: boolean;
  zoom: number;
  /** The UA the native view was created with; a mismatch with the wanted one means "reopen". */
  userAgent: string;
}

export interface BrowserState {
  tabs: Tab[];
  /** The tab in the (left) pane that the address bar and shortcuts act on. */
  activeId: string | null;
  /** A second tab shown in a right-hand pane, or null when not split. */
  splitId: string | null;
  /** Which pane the address bar and shortcuts act on. Only "split" while split. */
  focus: "main" | "split";
  skin: Skin;
  /** The chosen preset; decides the user-agent. */
  deviceId: string;
  /** The pinned viewport size, or null for full size. Starts from the preset; drag or type to change. */
  viewport: Viewport | null;
  customDevices: CustomDevice[];
}

const SKIN_KEY = "devspro.browser.skin";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* private mode or blocked storage: the choice just won't persist */
  }
}

/**
 * Tabs outlive the page. The native webviews are owned by the window, not by
 * React, so when the user visits Docker and comes back, the same tabs must be
 * re-adopted rather than re-created — a half-filled form on a page shouldn't
 * vanish because a sidebar link was clicked. This store is that memory: one
 * per window (a popped-out tab window has its own JS context and so its own).
 */
let state: BrowserState = {
  tabs: [],
  activeId: null,
  splitId: null,
  focus: "main",
  skin: ((v) => (isSkin(v) ? v : "chrome"))(read(SKIN_KEY)),
  // Deliberately not remembered: a device preset is a testing mode, and a
  // window that opens at a pinned 1280×800 looks broken, not configured.
  deviceId: "responsive",
  viewport: null,
  customDevices: parseCustomDevices(read(CUSTOM_DEVICES_KEY)),
};

const listeners = new Set<() => void>();

export function getState(): BrowserState {
  return state;
}

export function setState(patch: Partial<BrowserState> | ((s: BrowserState) => Partial<BrowserState>)) {
  const next = typeof patch === "function" ? patch(state) : patch;
  state = { ...state, ...next };
  if (next.skin) write(SKIN_KEY, next.skin);
  if (next.customDevices) write(CUSTOM_DEVICES_KEY, JSON.stringify(next.customDevices));
  for (const l of listeners) l();
}

export function updateTab(id: string, patch: Partial<Tab>) {
  setState((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? { ...t, ...patch } : t)) }));
}

/** The tab the toolbar acts on: the split pane's while that pane is focused, else the main one. */
export function focusedTab(s: BrowserState = state): Tab | null {
  const id = s.focus === "split" && s.splitId ? s.splitId : s.activeId;
  return id ? (s.tabs.find((t) => t.id === id) ?? null) : null;
}

export function tabById(id: string | null): Tab | null {
  return id ? (state.tabs.find((t) => t.id === id) ?? null) : null;
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
}

export function useBrowserState(): BrowserState {
  return useSyncExternalStore(subscribe, getState, getState);
}

/** Test seam. */
export function resetStore() {
  state = { tabs: [], activeId: null, splitId: null, focus: "main", skin: "chrome", deviceId: "responsive", viewport: null, customDevices: [] };
  for (const l of listeners) l();
}
