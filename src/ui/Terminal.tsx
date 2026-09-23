import { PointerEvent as ReactPointerEvent, ReactNode, useEffect, useRef } from "react";
import { Terminal as XTerm, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import { emit, listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
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
   */
  onPrompt?: () => void;
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
  hidden = false,
  footer,
  children,
}: {
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
  function startAsideDrag(e: ReactPointerEvent<HTMLDivElement>) {
    if (!onAsideResize || e.button !== 0) return;
    e.preventDefault();
    const origin = e.clientX;
    const start = asideWidth;
    const divider = e.currentTarget;
    divider.setPointerCapture(e.pointerId);
    divider.classList.add("term-aside-divider-dragging");
    const move = (ev: PointerEvent) => onAsideResize(clampAside(start + ev.clientX - origin));
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
              onPointerDown={startAsideDrag}
            />
          </>
        )}
        <div className="term-panel">{empty ? <div className="term-empty">{empty}</div> : children}</div>
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

function decodeBase64(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

export default function Terminal({
  id,
  command,
  active = true,
  focused = true,
  attached = false,
  onExit,
  onError,
  onCwd,
  onDetached,
  onPrompt,
}: TerminalProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const termRef = useRef<XTerm | null>(null);
  const onExitRef = useRef(onExit);
  const onErrorRef = useRef(onError);
  const onCwdRef = useRef(onCwd);
  const onDetachedRef = useRef(onDetached);
  const onPromptRef = useRef(onPrompt);
  onExitRef.current = onExit;
  onErrorRef.current = onError;
  onCwdRef.current = onCwd;
  onDetachedRef.current = onDetached;
  onPromptRef.current = onPrompt;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

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
    term.open(host);
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;

    term.parser.registerOscHandler(7, (data) => {
      const path = osc7Path(data);
      if (path) onCwdRef.current?.(path);
      return true;
    });

    let opened = false;
    // False once another window has adopted the session: from then on this
    // terminal is a stale view and must not kill the process on unmount.
    let owns = true;
    let disposed = false;
    const unlisteners: Array<() => void> = [];

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
      if (promptSeen || !onPromptRef.current) return;
      if (promptTimer !== null) window.clearTimeout(promptTimer);
      promptTimer = window.setTimeout(() => {
        const buffer = term.buffer.active;
        const line = buffer.getLine(buffer.baseY + buffer.cursorY)?.translateToString(true) ?? "";
        if (!promptSeen && looksLikePrompt(line)) {
          promptSeen = true;
          onPromptRef.current?.();
        }
      }, 400);
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
          else term.write(bytes, watchForPrompt);
        });
        unlisteners.push(offOutput);
        const offExit = await listen<{ code: number | null }>(`pty:exit:${id}`, (event) => {
          opened = false;
          if (pending) goLive();
          term.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n");
          onExitRef.current?.(event.payload.code);
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
            onDetachedRef.current?.();
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
          if (!opened || !owns) return;
          invoke("pty_write", { id, data: text }).catch((e) => onErrorRef.current?.(String(e)));
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
        onErrorRef.current?.(e instanceof Error ? e.message : String(e));
      }
    })();

    // Re-fit whenever the panel changes size: window resize, sidebar toggle,
    // or this terminal becoming visible again.
    const observer = new ResizeObserver(() => {
      if (host.offsetWidth > 0 && host.offsetHeight > 0) fit.fit();
    });
    observer.observe(host);

    // Re-theme when the app theme flips; reading a dozen custom properties is
    // cheap enough to do on every attribute change of <html>.
    const themeObserver = new MutationObserver(() => {
      term.options.theme = readTheme();
    });
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });

    return () => {
      disposed = true;
      if (handoffTimer !== null) window.clearTimeout(handoffTimer);
      if (promptTimer !== null) window.clearTimeout(promptTimer);
      observer.disconnect();
      themeObserver.disconnect();
      for (const off of unlisteners) off();
      if (opened && owns) invoke("pty_close", { id }).catch(() => undefined);
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
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
