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

export interface DownloadEvent {
  id: string;
  url: string;
  path: string | null;
  state: "started" | "finished";
  success: boolean;
}

/** A login the page just submitted; nothing is stored until the user says so. */
export interface CredentialEvent {
  id: string;
  origin: string;
  username: string;
  password: string;
  /** A login for this origin+username exists with a different password. */
  update: boolean;
}

export interface Bookmark {
  id: string;
  url: string;
  title: string;
  position: number;
  created_at: string;
}

export interface HistoryEntry {
  id: string;
  url: string;
  title: string;
  visits: number;
  visited_at: string;
}

/** A saved login. The password never rides along; ask for it with `browserCredentialReveal`. */
export interface Credential {
  id: string;
  origin: string;
  username: string;
  updated_at: string;
}

export interface SystemBrowser {
  id: string;
  label: string;
  command: string;
}

export const TITLE_EVENT = "browser:title";
export const LOAD_EVENT = "browser:load";
export const NEW_WINDOW_EVENT = "browser:new-window";
export const DOWNLOAD_EVENT = "browser:download";
export const CREDENTIAL_EVENT = "browser:credential";

// ---- tabs (native views) ----
export const browserOpen = (id: string, url: string, bounds: Bounds, userAgent?: string) =>
  invoke<void>("browser_open", { id, url, bounds, userAgent: userAgent ?? null });
export const browserNavigate = (id: string, url: string) => invoke<void>("browser_navigate", { id, url });
export const browserBack = (id: string) => invoke<void>("browser_back", { id });
export const browserForward = (id: string) => invoke<void>("browser_forward", { id });
export const browserReload = (id: string) => invoke<void>("browser_reload", { id });
export const browserStop = (id: string) => invoke<void>("browser_stop", { id });
export const browserSetBounds = (id: string, bounds: Bounds) => invoke<void>("browser_set_bounds", { id, bounds });
export const browserSetVisible = (id: string, visible: boolean) =>
  invoke<void>("browser_set_visible", { id, visible });
export const browserSetZoom = (id: string, factor: number) => invoke<void>("browser_set_zoom", { id, factor });
export const browserFocus = (id: string) => invoke<void>("browser_focus", { id });
/** Keyboard back to the app's own UI after a click outside a page. */
export const browserFocusParent = () => invoke<void>("browser_focus_parent");
export const browserDevtools = (id: string, open: boolean) => invoke<void>("browser_devtools", { id, open });
export const browserPrint = (id: string) => invoke<void>("browser_print", { id });
export const browserFind = (id: string, text: string, backwards = false) =>
  invoke<void>("browser_find", { id, text, backwards });
export const browserClearData = () => invoke<void>("browser_clear_data");
export const browserClose = (id: string) => invoke<void>("browser_close", { id });
export const browserList = () => invoke<string[]>("browser_list");

// ---- bookmarks ----
export const browserBookmarksList = () => invoke<Bookmark[]>("browser_bookmarks_list");
export const browserBookmarkAdd = (url: string, title: string) =>
  invoke<Bookmark>("browser_bookmark_add", { url, title });
export const browserBookmarkUpdate = (id: string, url: string, title: string) =>
  invoke<void>("browser_bookmark_update", { id, url, title });
export const browserBookmarkDelete = (id: string) => invoke<void>("browser_bookmark_delete", { id });
export const browserBookmarksReorder = (ids: string[]) => invoke<void>("browser_bookmarks_reorder", { ids });

// ---- history ----
export const browserHistorySearch = (query: string, limit?: number) =>
  invoke<HistoryEntry[]>("browser_history_search", { query, limit: limit ?? null });
export const browserHistoryDelete = (id: string) => invoke<void>("browser_history_delete", { id });
export const browserHistoryClear = () => invoke<void>("browser_history_clear");

// ---- saved logins ----
export const browserCredentialsList = () => invoke<Credential[]>("browser_credentials_list");
export const browserCredentialsFor = (origin: string) => invoke<Credential[]>("browser_credentials_for", { origin });
export const browserCredentialSave = (origin: string, username: string, password: string) =>
  invoke<void>("browser_credential_save", { origin, username, password });
export const browserCredentialDelete = (id: string) => invoke<void>("browser_credential_delete", { id });
export const browserCredentialReveal = (id: string) => invoke<string>("browser_credential_reveal", { id });
export const browserCredentialFill = (id: string, credentialId: string) =>
  invoke<void>("browser_credential_fill", { id, credentialId });

// ---- real browsers on this machine ----
export const browserSystemBrowsers = () => invoke<SystemBrowser[]>("browser_system_browsers");
export const browserOpenIn = (browser: string, url: string) => invoke<void>("browser_open_in", { browser, url });
