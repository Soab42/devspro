import type { ModuleDefinition } from "./ModuleRegistry";

/**
 * Pop a module out into its own OS window.
 *
 * The window loads the same bundle at `#/<module path>`. Only the window label
 * tells it apart, and the shell reads that to drop the sidebar and show just
 * the page. One window per module: asking for one that is already open just
 * brings it to the front.
 *
 * Imported lazily so the plain `vite` dev server and the test environment,
 * which have no Tauri runtime, never touch `@tauri-apps/api/webviewWindow`.
 */
/** A module window: `module-<id>` for the one fresh window per module, `module-<id>/<uuid>` for a window that carries state. */
const LABEL_PREFIX = "module-";
/** A single browser tab in its own window: label `browser-tab-<uuid>`, route `/browser?url=…`. */
const TAB_PREFIX = "browser-tab-";
/** A single terminal session in its own window: label `term-<pty id>`, route `/terminal?pty=…`. */
const TERM_PREFIX = "term-";
/** Where a popped-out browser tab portals its toolbar controls (an element id in the popout shell's top bar). */
export const POPOUT_ACTIONS_SLOT = "popout-topbar-actions";
/** Where a popped-out terminal puts its window strip: beside the title in the top bar, like browser tabs. */
export const POPOUT_TABS_SLOT = "popout-topbar-tabs";

function currentLabel(): string | null {
  try {
    const label: unknown = (window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentWebview?: { label?: string } } } })
      .__TAURI_INTERNALS__?.metadata?.currentWebview?.label;
    return typeof label === "string" ? label : null;
  } catch {
    return null;
  }
}

export function popoutLabel(moduleId: string): string {
  return `${LABEL_PREFIX}${moduleId}`;
}

/** Is this the popped-out window for a module, rather than the main one? */
export function currentPopoutModuleId(): string | null {
  const label = currentLabel();
  if (!label) return null;
  if (label.startsWith(LABEL_PREFIX)) return label.slice(LABEL_PREFIX.length).split("/")[0];
  if (label.startsWith(TAB_PREFIX)) return "browser";
  if (label.startsWith(TERM_PREFIX)) return "terminal";
  return null;
}

/** Is this window a single popped-out terminal (one workspace, no host/key tabs)? */
export function isTerminalWindow(): boolean {
  return currentLabel()?.startsWith(TERM_PREFIX) ?? false;
}

/** Is this window a single popped-out browser tab (no tab strip, one page)? */
export function isBrowserTabWindow(): boolean {
  return currentLabel()?.startsWith(TAB_PREFIX) ?? false;
}

/**
 * Pop one browser tab out into its own window. The page can't be moved
 * between windows — it is an X11 child of the one it was born in — so the
 * new window opens the same URL; cookies and sessions are shared, so it
 * comes up logged in. Unlike module windows, every call makes a new one.
 */
export async function openBrowserTabWindow(url: string, title?: string): Promise<void> {
  const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  const label = `${TAB_PREFIX}${crypto.randomUUID()}`;
  const win = new WebviewWindow(label, {
    url: `${window.location.pathname}#/browser?url=${encodeURIComponent(url)}`,
    title: title?.trim() ? `${title.trim()} — DevsPro` : "Browser — DevsPro",
    width: 1100,
    height: 760,
    minWidth: 480,
    minHeight: 360,
  });
  await new Promise<void>((resolve, reject) => {
    void win.once("tauri://created", () => resolve());
    void win.once("tauri://error", (e) => reject(new Error(String(e.payload))));
  });
}

/**
 * Pop a running terminal out into its own window. The window label carries
 * the session id so the Rust side can kill the shell when the window goes,
 * and the new page adopts the session (`pty_adopt`) rather than starting
 * one. `command` and `title` ride along in the URL so the new window can
 * label the pane and pick the right file browser.
 */
export async function openTerminalWindow(
  ptyId: string,
  command: "local" | { ssh: string } | { exec: unknown },
  title: string,
): Promise<void> {
  const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  const params = new URLSearchParams({ pty: ptyId, title });
  if (command !== "local" && "ssh" in command) params.set("host", command.ssh);
  const win = new WebviewWindow(`${TERM_PREFIX}${ptyId}`, {
    url: `${window.location.pathname}#/terminal?${params.toString()}`,
    title: `${title} \u2014 DevsPro`,
    width: 960,
    height: 620,
    minWidth: 480,
    minHeight: 320,
  });
  await new Promise<void>((resolve, reject) => {
    void win.once("tauri://created", () => resolve());
    void win.once("tauri://error", (e) => reject(new Error(String(e.payload))));
  });
}

/**
 * Open a module in its own window.
 *
 * Without `params` this is the fresh window the launcher opens: one per
 * module, and asking again just brings it to the front. With `params` it is
 * a continuation of the page the user is on (see `popoutState.ts`): the
 * route carries the state and every call makes a new window, because two
 * folders or two hosts are two windows.
 */
export async function openModuleWindow(m: ModuleDefinition, params?: Record<string, string>): Promise<void> {
  const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  const carried = params && Object.keys(params).length > 0;
  const label = carried ? `${popoutLabel(m.id)}/${crypto.randomUUID()}` : popoutLabel(m.id);

  if (!carried) {
    const existing = await WebviewWindow.getByLabel(label);
    if (existing) {
      await existing.setFocus();
      return;
    }
  }

  const query = carried ? `?${new URLSearchParams(params).toString()}` : "";
  const win = new WebviewWindow(label, {
    url: `${window.location.pathname}#${m.path}${query}`,
    title: `${m.label} — DevsPro`,
    width: 1100,
    height: 720,
    minWidth: 640,
    minHeight: 480,
  });
  await new Promise<void>((resolve, reject) => {
    void win.once("tauri://created", () => resolve());
    void win.once("tauri://error", (e) => reject(new Error(String(e.payload))));
  });
}
