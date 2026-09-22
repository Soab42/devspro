import { invoke } from "@tauri-apps/api/core";

/** Logical (CSS px) rectangle of the content area, relative to the window. */
export interface Bounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface TitleEvent {
  id: string;
  title: string;
}

export interface LoadEvent {
  id: string;
  url: string;
  state: "started" | "finished";
}

export interface NewWindowEvent {
  id: string;
  url: string;
}

export const TITLE_EVENT = "browser:title";
export const LOAD_EVENT = "browser:load";
export const NEW_WINDOW_EVENT = "browser:new-window";

export const browserOpen = (id: string, url: string, bounds: Bounds) =>
  invoke<void>("browser_open", { id, url, bounds });
export const browserNavigate = (id: string, url: string) => invoke<void>("browser_navigate", { id, url });
export const browserBack = (id: string) => invoke<void>("browser_back", { id });
export const browserForward = (id: string) => invoke<void>("browser_forward", { id });
export const browserReload = (id: string) => invoke<void>("browser_reload", { id });
export const browserStop = (id: string) => invoke<void>("browser_stop", { id });
export const browserSetBounds = (id: string, bounds: Bounds) => invoke<void>("browser_set_bounds", { id, bounds });
export const browserSetVisible = (id: string, visible: boolean) =>
  invoke<void>("browser_set_visible", { id, visible });
export const browserSetZoom = (id: string, factor: number) => invoke<void>("browser_set_zoom", { id, factor });
export const browserClose = (id: string) => invoke<void>("browser_close", { id });
export const browserList = () => invoke<string[]>("browser_list");
