import { PointerEvent as ReactPointerEvent, ReactNode, useEffect, useRef } from "react";
import { Terminal as XTerm, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import { emit, listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import "@xterm/xterm/css/xterm.css";
import "./Terminal.css";

/**
 * An xterm.js terminal bound to a PTY session in the Rust backend.
 *
 * Deliberately NOT re-exported from src/ui/index.ts — like CodeEditor, xterm is
 * a few hundred kB that only one page needs. Import it directly and lazy() the
 * component that uses it:
 *   `const Terminal = lazy(() => import("../../../ui/Terminal"))`
 *
 * The session id is generated here, on the client, and the output listener is
 * registered BEFORE `pty_open` is invoked. A shell prints its prompt within
 * milliseconds of starting; if the id came back from Rust and the listener
 * were attached afterwards, those first bytes would be lost every time.
 */

export type PtyCommand = "local" | { ssh: string } | { exec: { program: string; args: string[]; cwd: string | null } };

export interface TerminalProps {
  /** Client-chosen session id (see above). */
  id: string;
  command: PtyCommand;
  /** Hidden terminals stay mounted so their scrollback survives tab switches. */
  active?: boolean;
  /** The pane the keyboard goes to. Only one of several visible terminals is focused. */
  focused?: boolean;
  /**
   * The session is already running, started by another window: adopt it
   * instead of spawning a new process. That window hands over its screen
   * contents on request (see `pty:attach` / `pty:handoff` below).
   */
  attached?: boolean;
  /**
   * Keep the session running when this component unmounts (its page was left),
   * and pick it up again when one with the same id mounts. It ends with
   * `endTerminalSession`, or when the app closes.
   */
  persist?: boolean;
  onExit?: (code: number | null) => void;
  onError?: (message: string) => void;
  /**
   * The shell reported its working directory (OSC 7, `\e]7;file://host/path\a`).
   * Shells don't all emit it by default; the workspace can install a hook.
   */
  onCwd?: (path: string) => void;
  /** Another window adopted this session; this terminal no longer owns it and should go. */
  onDetached?: () => void;
  /**
   * Fired once, the first time the screen settles on something that looks
   * like a shell prompt (and not a password prompt). What the follow-along
   * browser uses to know it is safe to type its hook into a remote shell.
   * Return true if what was just typed is such a hook: its echo, up to the
   * first OSC 7 it produces, is then wiped so the user never sees it.
   */
  onPrompt?: () => boolean | void;
  /**
   * An image was pasted, and saved on this machine at `localPath`. Resolve
   * with the path to type instead (where a remote shell can read it), or null
   * to type nothing. Without it the local path is typed, which is right for a
   * local shell.
   */
  onPasteImage?: (localPath: string) => Promise<string | null>;
}

const PROMPT_END = /[$#%>\u276f]\s*$/;
const SECRET_PROMPT = /passw|passphrase|verification code|otp/i;

/** Does the cursor line read like a shell waiting for a command? */
export function looksLikePrompt(line: string): boolean {
  const text = line.trimEnd();
  return text.length > 0 && PROMPT_END.test(text) && !SECRET_PROMPT.test(text);
}

/** Cross-window events used when a terminal is popped out. */
interface Handoff {
  cols: number;
  rows: number;
  /** The screen and scrollback as an escape-sequence stream (xterm's serialize addon). */
  data: string;
}
export const ptyAttachEvent = (id: string) => `pty:attach:${id}`;

const HIDE_HOOK_EVENT = "devspro:terminal-hide-hook";

/**
 * Call just before typing the follow hook into session `id` by hand (the
 * Files panel's "send the prompt hook again"): its echo is then wiped like
 * the one typed automatically at the first prompt.
 */
export function hideNextHook(id: string) {
  window.dispatchEvent(new CustomEvent(HIDE_HOOK_EVENT, { detail: id }));
}
export const ptyHandoffEvent = (id: string) => `pty:handoff:${id}`;

/** The path from an OSC 7 payload such as `file://box/home/me`; null when it isn't one. */
export function osc7Path(data: string): string | null {
  const match = /^file:\/\/[^/]*(\/.*)$/.exec(data.trim());
  if (!match) return null;
  try {
    return decodeURIComponent(match[1]);
  } catch {
    return match[1];
  }
}

/** The most base64 an OSC 52 copy may carry: about 768 KB of text. */
export const OSC52_MAX = 1 << 20;

/**
 * The text an OSC 52 payload (`c;aGVsbG8=`) puts on the clipboard; null for a
 * read request (`c;?`), an empty one, one over OSC52_MAX, or anything that
 * isn't base64 UTF-8. Control characters other than tab and newline are
 * dropped, so a copy can't carry escape sequences into wherever it's pasted.
 */
export function osc52Text(data: string): string | null {
  const split = data.indexOf(";");
  if (split < 0) return null;
  const payload = data.slice(split + 1);
  if (!payload || payload === "?" || payload.length > OSC52_MAX) return null;
  try {
    const text = new TextDecoder("utf-8", { fatal: true }).decode(decodeBase64(payload));
    // eslint-disable-next-line no-control-regex
    return text.replace(/\r\n?/g, "\n").replace(/[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/g, "") || null;
  } catch {
    return null;
  }
}

/**
 * A saved image's path as typed into the terminal: bare when that reads the
 * same to a shell, quoted otherwise, and followed by a space as a dropped
 * file's is. Claude Code attaches an image whose path is pasted.
 */
export function pastedPath(path: string): string {
  return /^[\w@%+=:,./-]+$/.test(path) ? `${path} ` : `'${path.replace(/'/g, `'\\''`)}' `;
}

/**
 * Ctrl+V on its own, which a terminal sends as ^V and Claude Code takes as
 * "paste an image". Matched by key code as xterm does, so it holds on any
 * keyboard layout.
 */
export function isCtrlV(event: KeyboardEvent): boolean {
  return event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey && (event.key === "v" || event.keyCode === 86);
}

/**
 * Wrapper that makes a terminal fill the page: `toolbar` above, the terminal
 * body below taking every remaining pixel. Kept here rather than in a module
 * stylesheet because the layout is part of what makes a terminal usable.
 */
export function TerminalWorkspace({
  tabs,
  actions,
  empty,
  aside,
  asideWidth = 260,
  onAsideResize,
  rightAside,
  rightAsideWidth = 300,
  onRightAsideResize,
  hidden = false,
  footer,
  children,
}: {
  /** An optional column to the right of the panel — the command library. */
  rightAside?: ReactNode;
  rightAsideWidth?: number;
  onRightAsideResize?: (width: number) => void;
  /** A strip under the panel — the host status bar. */
  footer?: ReactNode;
  /** Keeps the sessions mounted (processes, scrollback) while another view is shown. */
  hidden?: boolean;
  /** The session strip; grows to take the toolbar's spare width. */
  tabs?: ReactNode;
  /** Right-hand controls: close, new terminal. */
  actions?: ReactNode;
  /** Shown centred in the panel when there is nothing to render. */
  empty?: ReactNode;
  /** An optional column to the left of the panel — a file browser that follows the shell. */
  aside?: ReactNode;
  asideWidth?: number;
  /** The aside's divider was dragged to a new width. Without it the aside is fixed. */
  onAsideResize?: (width: number) => void;
  children?: ReactNode;
}) {
  // `side` is which way the panel grows: dragging the left one's divider
  // right widens it, the right one's narrows it.
  function startAsideDrag(e: ReactPointerEvent<HTMLDivElement>, side: 1 | -1) {
    const resize = side === 1 ? onAsideResize : onRightAsideResize;
    if (!resize || e.button !== 0) return;
    e.preventDefault();
    const origin = e.clientX;
    const start = side === 1 ? asideWidth : rightAsideWidth;
    const divider = e.currentTarget;
    divider.setPointerCapture(e.pointerId);
    divider.classList.add("term-aside-divider-dragging");
    const move = (ev: PointerEvent) => resize(clampAside(start + side * (ev.clientX - origin)));
    const stop = () => {
      divider.classList.remove("term-aside-divider-dragging");
      divider.removeEventListener("pointermove", move);
      divider.removeEventListener("pointerup", stop);
      divider.removeEventListener("pointercancel", stop);
    };
    divider.addEventListener("pointermove", move);
    divider.addEventListener("pointerup", stop);
    divider.addEventListener("pointercancel", stop);
  }

  return (
    <div className={`term-workspace${hidden ? " term-hidden" : ""}`}>
      {(tabs || actions) && (
        <div className="term-toolbar">
          <div className="term-toolbar-tabs">{tabs}</div>
          {actions}
        </div>
      )}
      <div className="term-body">
        {aside && (
          <>
            <aside className="term-aside" style={{ width: clampAside(asideWidth) }}>
              {aside}
            </aside>
            <div
              className={`term-aside-divider${onAsideResize ? "" : " term-aside-divider-fixed"}`}
              role="separator"
              aria-orientation="vertical"
              onPointerDown={(e) => startAsideDrag(e, 1)}
            />
          </>
        )}
        <div className="term-panel">{empty ? <div className="term-empty">{empty}</div> : children}</div>
        {rightAside && (
          <>
            <div
              className={`term-aside-divider${onRightAsideResize ? "" : " term-aside-divider-fixed"}`}
              role="separator"
              aria-orientation="vertical"
              onPointerDown={(e) => startAsideDrag(e, -1)}
            />
            <aside className="term-aside" style={{ width: clampAside(rightAsideWidth) }}>
              {rightAside}
            </aside>
          </>
        )}
      </div>
      {footer}
    </div>
  );
}

export const ASIDE_MIN = 180;
export const ASIDE_MAX = 560;

export function clampAside(width: number): number {
  return Math.min(ASIDE_MAX, Math.max(ASIDE_MIN, Math.round(width)));
}

function cssColor(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
}

/* The design tokens carry no ANSI palette, so those sixteen colours are fixed
   here, one set per theme. Everything structural — background, foreground,
   cursor, selection — comes from the tokens so the terminal sits in the page
   rather than on it. */
const ANSI_DARK = {
  black: "#1e1e21", red: "#f26d67", green: "#5fcf82", yellow: "#e6b455",
  blue: "#7d95ec", magenta: "#c88ae0", cyan: "#5fc8d8", white: "#d4d4d8",
  brightBlack: "#71717a", brightRed: "#ff8a85", brightGreen: "#7ee0a0", brightYellow: "#f5c76f",
  brightBlue: "#9cb0f5", brightMagenta: "#dba7ef", brightCyan: "#80dbe8", brightWhite: "#fafafa",
};

const ANSI_LIGHT = {
  black: "#18181b", red: "#c8322b", green: "#157f3d", yellow: "#b25e09",
  blue: "#3b5bd0", magenta: "#8b3fbd", cyan: "#1d7f96", white: "#a1a1aa",
  brightBlack: "#52525b", brightRed: "#a92823", brightGreen: "#106b33", brightYellow: "#8f4a06",
  brightBlue: "#2b46ac", brightMagenta: "#6e2f98", brightCyan: "#186a7d", brightWhite: "#e4e4e7",
};

function readTheme(): ITheme {
  const dark = document.documentElement.getAttribute("data-theme") === "dark";
  const accent = cssColor("--color-accent", dark ? "#7d95ec" : "#3b5bd0");
  return {
    background: cssColor("--color-surface", dark ? "#18181b" : "#ffffff"),
    foreground: cssColor("--color-text", dark ? "#fafafa" : "#18181b"),
    cursor: accent,
    cursorAccent: cssColor("--color-surface", dark ? "#18181b" : "#ffffff"),
    selectionBackground: cssColor("--color-accent-border", dark ? "#2f3f7a" : "#c3cef5"),
    ...(dark ? ANSI_DARK : ANSI_LIGHT),
  };
}

/** Where `ESC ] 7 ;` starts in a chunk of output, or -1. */
function indexOfOsc7(bytes: Uint8Array): number {
  for (let i = 0; i + 3 < bytes.length; i++) {
    if (bytes[i] === 0x1b && bytes[i + 1] === 0x5d && bytes[i + 2] === 0x37 && bytes[i + 3] === 0x3b) return i;
  }
  return -1;
}

function decodeBase64(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/** What a session reports back; the mounted component's latest props. */
interface TerminalCallbacks {
  onExit?: (code: number | null) => void;
  onError?: (message: string) => void;
  onCwd?: (path: string) => void;
  onDetached?: () => void;
  onPrompt?: () => boolean | void;
  onPasteImage?: (localPath: string) => Promise<string | null>;
  /** On screen, as opposed to a hidden tab kept mounted for its scrollback. */
  active?: boolean;
}

/** One live terminal: the xterm screen, its element, and the PTY behind it. */
interface LiveTerminal {
  term: XTerm;
  fit: FitAddon;
  element: HTMLDivElement;
  /** Swapped for the new component's callbacks when the screen is re-mounted. */
  holder: { cb: { current: TerminalCallbacks } };
  /** Closes the PTY (if this window owns it) and frees the screen. */
  dispose: () => void;
}

/**
 * Sessions kept running while their page is away (`persist`). The screen
 * keeps taking output into its scrollback and the shell keeps running; when
 * the page comes back the same screen is put back in place. A session leaves
 * when its pane is closed (`endTerminalSession`), or when it was never kept.
 */
const live = new Map<string, LiveTerminal>();
const mounted = new Set<string>();

/** Ends a kept session whose pane is gone. A mounted one ends when it unmounts. */
export function endTerminalSession(id: string) {
  const session = live.get(id);
  if (!session || mounted.has(id)) return;
  live.delete(id);
  session.dispose();
}

function createSession(
  host: HTMLElement,
  id: string,
  command: PtyCommand,
  attached: boolean,
  holder: { cb: { current: TerminalCallbacks } },
): LiveTerminal {
  const term = new XTerm({
    cursorBlink: true,
    fontFamily: cssColor("--font-mono", "monospace"),
    fontSize: 13,
    lineHeight: 1.2,
    scrollback: 5000,
    theme: readTheme(),
    allowProposedApi: false,
  });
  const fit = new FitAddon();
  const serialize = new SerializeAddon();
  term.loadAddon(fit);
  term.loadAddon(serialize);
  // xterm lives in its own element, so the screen can move between hosts:
  // detached while its page is away, put back when it returns.
  const element = document.createElement("div");
  element.className = "term-surface";
  // Opened in place: xterm measures its font when it opens, which a detached
  // element can't do.
  host.appendChild(element);
  term.open(element);

  term.parser.registerOscHandler(7, (data) => {
    const path = osc7Path(data);
    if (path) holder.cb.current.onCwd?.(path);
    return true;
  });

  // Copy on select, as in most Linux terminals and Termius. xterm reports a
  // mouse selection once, on mouseup (and again, empty, when it is cleared),
  // so a drag puts one entry on the clipboard rather than every partial one.
  term.onSelectionChange(() => {
    const text = term.getSelection();
    if (text) writeText(text).catch(() => undefined);
  });

  // A program in the terminal setting the clipboard: how Claude Code, tmux
  // and vim copy, and over ssh the only way they can. Reading it back ("?")
  // is refused; the far end has no business seeing what was copied here.
  // Only the pane holding the keyboard, in the window the user is in, may
  // set it, so output scrolling past in a background one (a log, a file
  // being cat'ed) can't swap it underneath them. Read from the DOM rather
  // than the `focused` prop, which the dock and AI page leave at its default;
  // and on screen too, in case a hidden tab's input kept focus. A kept
  // session whose page is away has no mounted component, so it counts as off
  // screen.
  term.parser.registerOscHandler(52, (data) => {
    if (!mounted.has(id) || holder.cb.current.active === false) return true;
    if (!document.hasFocus() || document.activeElement !== term.textarea) return true;
    const text = osc52Text(data);
    if (text) writeText(text).catch(() => undefined);
    return true;
  });

  let opened = false;
  // False once another window has adopted the session: from then on this
  // terminal is a stale view and must not kill the process on unmount.
  let owns = true;
  let disposed = false;
  const unlisteners: Array<() => void> = [];

  const write = (text: string) => {
    if (!opened || !owns) return;
    invoke("pty_write", { id, data: text }).catch((e) => holder.cb.current.onError?.(String(e)));
  };

  // Pasting from the clipboard, read through the backend. An image is
  // saved to a file and its path typed in (see onPasteImage). Ctrl+V pastes
  // an image when the clipboard holds one and is otherwise sent as the ^V
  // it always was; a right click pastes text, or else an image, as in
  // Termius and PuTTY; so does a paste event with no text in it
  // (Ctrl+Shift+V of a screenshot). Keystrokes typed while the clipboard is
  // being read wait, so Ctrl+V then j in vim still reaches it in that order.
  let reading = 0;
  let typedMeanwhile: string[] = [];
  let read = Promise.resolve();
  const pasteClipboard = ({ text, otherwise }: { text: boolean; otherwise: string | null }) => {
    reading += 1;
    read = read.then(async () => {
      let pasted: string | null = null;
      let image: string | null = null;
      try {
        if (text) pasted = await invoke<string | null>("terminal_clipboard_text");
        if (!pasted) image = await invoke<string | null>("terminal_clipboard_image");
      } catch (e) {
        holder.cb.current.onError?.(String(e));
      }
      reading -= 1;
      if (pasted) term.paste(pasted);
      else if (!image && otherwise) write(otherwise);
      if (reading === 0) {
        const typed = typedMeanwhile;
        typedMeanwhile = [];
        for (const keys of typed) write(keys);
      }
      if (!image) return;
      // Not awaited: an upload to a remote host shouldn't hold up the next paste.
      const onPasteImage = holder.cb.current.onPasteImage;
      (onPasteImage ? onPasteImage(image) : Promise.resolve(image)).then(
        (path) => {
          if (path && !disposed) term.paste(pastedPath(path));
        },
        (e) => holder.cb.current.onError?.(e instanceof Error ? e.message : String(e)),
      );
    });
  };
  term.attachCustomKeyEventHandler((event) => {
    if (!isCtrlV(event)) return true;
    if (event.type === "keydown") {
      // Or the webview pastes the clipboard's text as well.
      event.preventDefault();
      pasteClipboard({ text: false, otherwise: "\x16" });
    }
    return false;
  });
  // Both ahead of xterm's own handlers: its paste reads text alone, and its
  // right click only readies the webview's menu. On the session's own
  // element, which travels with it between hosts.
  const onPaste = (event: ClipboardEvent) => {
    if (event.clipboardData?.getData("text/plain")) return;
    event.preventDefault();
    event.stopPropagation();
    pasteClipboard({ text: false, otherwise: null });
  };
  const onContextMenu = (event: MouseEvent) => {
    event.preventDefault();
    event.stopPropagation();
    term.focus();
    pasteClipboard({ text: true, otherwise: null });
  };
  element.addEventListener("paste", onPaste, true);
  element.addEventListener("contextmenu", onContextMenu, true);
  unlisteners.push(
    () => element.removeEventListener("paste", onPaste, true),
    () => element.removeEventListener("contextmenu", onContextMenu, true),
  );

  // Output that arrives before an adopted session's screen has been handed
  // over is held back, or it would land on a blank screen and be painted
  // over by the handoff. Null once live.
  let pending: Uint8Array[] | null = attached ? [] : null;
  let handoffTimer: number | null = null;

  // Prompt detection: once the output has been quiet for a moment, look at
  // the cursor line. Reported once per session.
  let promptSeen = false;
  let promptTimer: number | null = null;
  const watchForPrompt = () => {
    if (promptSeen || !holder.cb.current.onPrompt) return;
    if (promptTimer !== null) window.clearTimeout(promptTimer);
    promptTimer = window.setTimeout(() => {
      const buffer = term.buffer.active;
      const line = buffer.getLine(buffer.baseY + buffer.cursorY)?.translateToString(true) ?? "";
      if (!promptSeen && looksLikePrompt(line)) {
        promptSeen = true;
        if (holder.cb.current.onPrompt?.() === true) {
          hookLine = buffer.baseY + buffer.cursorY;
          // A shell that never answers (fish, say) must not leave this armed.
          hookTimer = window.setTimeout(() => (hookLine = null), 5000);
        }
      }
    }, 400);
  };

  // Hiding a typed hook: the prompt line it was typed at, until its first
  // OSC 7 arrives. At that point everything from that line down (prompt,
  // echo, wrapped rows) is erased, and the shell draws a fresh prompt there.
  // Output that arrives while the erase is being positioned is held back.
  let hookLine: number | null = null;
  let hookTimer: number | null = null;
  let held: Uint8Array[] | null = null;
  const armHookHide = (e: Event) => {
    if ((e as CustomEvent<string>).detail !== id) return;
    const buffer = term.buffer.active;
    hookLine = buffer.baseY + buffer.cursorY;
    if (hookTimer !== null) window.clearTimeout(hookTimer);
    hookTimer = window.setTimeout(() => (hookLine = null), 5000);
  };
  window.addEventListener(HIDE_HOOK_EVENT, armHookHide);
  unlisteners.push(() => window.removeEventListener(HIDE_HOOK_EVENT, armHookHide));
  const show = (bytes: Uint8Array) => {
    if (held) {
      held.push(bytes);
      return;
    }
    const at = hookLine === null ? -1 : indexOfOsc7(bytes);
    if (at < 0 || hookLine === null) {
      term.write(bytes, watchForPrompt);
      return;
    }
    const from = hookLine;
    hookLine = null;
    if (hookTimer !== null) window.clearTimeout(hookTimer);
    held = [];
    term.write(bytes.subarray(0, at), () => {
      const buffer = term.buffer.active;
      const up = buffer.baseY + buffer.cursorY - from;
      term.write(`\r${up > 0 ? `\x1b[${up}A` : ""}\x1b[J`);
      term.write(bytes.subarray(at));
      const rest = held ?? [];
      held = null;
      for (const chunk of rest) show(chunk);
    });
  };

  const goLive = () => {
    const queued = pending ?? [];
    pending = null;
    if (handoffTimer !== null) window.clearTimeout(handoffTimer);
    for (const chunk of queued) term.write(chunk);
    // Only now tell the process its new size. The shell redraws its prompt
    // on SIGWINCH, which is right only once the cursor is where it thinks.
    fit.fit();
    if (opened) invoke("pty_resize", { id, cols: term.cols, rows: term.rows }).catch(() => undefined);
    term.focus();
  };

  // Ordered so nothing is missed: subscribe, then start the process, then
  // wire input/resize (writes before the process exists would just fail).
  (async () => {
    try {
      const offOutput = await listen<string>(`pty:output:${id}`, (event) => {
        const bytes = decodeBase64(event.payload);
        if (pending) pending.push(bytes);
        else show(bytes);
      });
      unlisteners.push(offOutput);
      const offExit = await listen<{ code: number | null }>(`pty:exit:${id}`, (event) => {
        opened = false;
        if (pending) goLive();
        term.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n");
        holder.cb.current.onExit?.(event.payload.code);
      });
      unlisteners.push(offExit);

      if (attached) {
        // Take the screen the previous window hands over, then go live.
        // The handoff listener goes first so the reply to our attach
        // announcement can't slip past.
        const offHandoff = await listen<Handoff>(ptyHandoffEvent(id), (event) => {
          if (!pending) return;
          // Replay at the size it was captured at, so wrapped rows land
          // where they were; the fit in goLive then reflows to this window.
          term.resize(event.payload.cols, event.payload.rows);
          term.write(event.payload.data, goLive);
        });
        unlisteners.push(offHandoff);
      } else {
        const offAttach = await listen(ptyAttachEvent(id), () => {
          if (!owns) return;
          owns = false;
          const handoff: Handoff = { cols: term.cols, rows: term.rows, data: serialize.serialize({ scrollback: 2000 }) };
          void emit(ptyHandoffEvent(id), handoff);
          holder.cb.current.onDetached?.();
        });
        unlisteners.push(offAttach);
      }
      if (disposed) return;

      if (attached) {
        await invoke("pty_adopt", { id });
        opened = true;
        await emit(ptyAttachEvent(id));
        // The previous window may already be gone; don't wait forever.
        handoffTimer = window.setTimeout(() => {
          if (pending) goLive();
        }, 2000);
      } else {
        await invoke("pty_open", { id, cols: term.cols, rows: term.rows, command });
        opened = true;
      }
      if (disposed) {
        await invoke("pty_close", { id }).catch(() => undefined);
        return;
      }

      const data = term.onData((text) => {
        if (reading > 0) typedMeanwhile.push(text);
        else write(text);
      });
      const resize = term.onResize(({ cols, rows }) => {
        if (!opened || !owns || pending) return;
        invoke("pty_resize", { id, cols, rows }).catch(() => undefined);
      });
      unlisteners.push(() => data.dispose(), () => resize.dispose());
      if (!attached) {
        // Fit once more now that the process exists, so it starts at the
        // real size rather than the 80x24 default.
        fit.fit();
        term.focus();
      }
    } catch (e) {
      holder.cb.current.onError?.(e instanceof Error ? e.message : String(e));
    }
  })();

  // Re-theme when the app theme flips; reading a dozen custom properties is
  // cheap enough to do on every attribute change of <html>.
  const themeObserver = new MutationObserver(() => {
    term.options.theme = readTheme();
  });
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });

  const dispose = () => {
    disposed = true;
    if (handoffTimer !== null) window.clearTimeout(handoffTimer);
    if (promptTimer !== null) window.clearTimeout(promptTimer);
    if (hookTimer !== null) window.clearTimeout(hookTimer);
    themeObserver.disconnect();
    for (const off of unlisteners) off();
    if (opened && owns) invoke("pty_close", { id }).catch(() => undefined);
    term.dispose();
  };
  return { term, fit, element, holder, dispose };
}

export default function Terminal({
  id,
  command,
  active = true,
  focused = true,
  attached = false,
  persist = false,
  onExit,
  onError,
  onCwd,
  onDetached,
  onPrompt,
  onPasteImage,
}: TerminalProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const termRef = useRef<XTerm | null>(null);
  const cbRef = useRef<TerminalCallbacks>({});
  cbRef.current = { onExit, onError, onCwd, onDetached, onPrompt, onPasteImage, active };
  const persistRef = useRef(persist);
  persistRef.current = persist;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let session = live.get(id);
    if (session) session.holder.cb = cbRef;
    else {
      session = createSession(host, id, command, attached, { cb: cbRef });
      live.set(id, session);
    }
    const { term, fit, element } = session;
    host.appendChild(element);
    mounted.add(id);
    termRef.current = term;
    fitRef.current = fit;
    fit.fit();
    // Back from the background: repaint what arrived while it was away.
    term.refresh(0, term.rows - 1);

    // Re-fit whenever the panel changes size: window resize, sidebar toggle,
    // or this terminal becoming visible again.
    const observer = new ResizeObserver(() => {
      if (host.offsetWidth > 0 && host.offsetHeight > 0) fit.fit();
    });
    observer.observe(host);

    const current = session;
    return () => {
      observer.disconnect();
      mounted.delete(id);
      termRef.current = null;
      fitRef.current = null;
      if (element.parentElement === host) host.removeChild(element);
      if (persistRef.current && live.get(id) === current) return;
      live.delete(id);
      current.dispose();
    };
    // `command` is fixed for the life of a session; changing it means a new id.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  // A display:none terminal can't measure itself, so fit when it comes back.
  useEffect(() => {
    if (!active) return;
    const frame = requestAnimationFrame(() => {
      fitRef.current?.fit();
      if (focused) termRef.current?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [active, focused]);

  return <div ref={hostRef} className={`term-host${active ? "" : " term-hidden"}`} />;
}
