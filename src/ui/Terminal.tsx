import { ReactNode, useEffect, useRef } from "react";
import { Terminal as XTerm, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen } from "@tauri-apps/api/event";
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

export type PtyCommand = "local" | { ssh: string };

export interface TerminalProps {
  /** Client-chosen session id (see above). */
  id: string;
  command: PtyCommand;
  /** Hidden terminals stay mounted so their scrollback survives tab switches. */
  active?: boolean;
  onExit?: (code: number | null) => void;
  onError?: (message: string) => void;
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
  hidden = false,
  children,
}: {
  /** Keeps the sessions mounted (processes, scrollback) while another view is shown. */
  hidden?: boolean;
  /** The session strip; grows to take the toolbar's spare width. */
  tabs?: ReactNode;
  /** Right-hand controls: close, new terminal. */
  actions?: ReactNode;
  /** Shown centred in the panel when there is nothing to render. */
  empty?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className={`term-workspace${hidden ? " term-hidden" : ""}`}>
      {(tabs || actions) && (
        <div className="term-toolbar">
          <div className="term-toolbar-tabs">{tabs}</div>
          {actions}
        </div>
      )}
      <div className="term-panel">
        {empty ? <div className="term-empty">{empty}</div> : children}
      </div>
    </div>
  );
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

export default function Terminal({ id, command, active = true, onExit, onError }: TerminalProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const termRef = useRef<XTerm | null>(null);
  const onExitRef = useRef(onExit);
  const onErrorRef = useRef(onError);
  onExitRef.current = onExit;
  onErrorRef.current = onError;

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
    term.loadAddon(fit);
    term.open(host);
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;

    let opened = false;
    let disposed = false;
    const unlisteners: Array<() => void> = [];

    // Ordered so nothing is missed: subscribe, then start the process, then
    // wire input/resize (writes before the process exists would just fail).
    (async () => {
      try {
        const offOutput = await listen<string>(`pty:output:${id}`, (event) => {
          term.write(decodeBase64(event.payload));
        });
        unlisteners.push(offOutput);
        const offExit = await listen<{ code: number | null }>(`pty:exit:${id}`, (event) => {
          opened = false;
          term.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n");
          onExitRef.current?.(event.payload.code);
        });
        unlisteners.push(offExit);
        if (disposed) return;

        await invoke("pty_open", { id, cols: term.cols, rows: term.rows, command });
        opened = true;
        if (disposed) {
          await invoke("pty_close", { id }).catch(() => undefined);
          return;
        }

        const data = term.onData((text) => {
          if (!opened) return;
          invoke("pty_write", { id, data: text }).catch((e) => onErrorRef.current?.(String(e)));
        });
        const resize = term.onResize(({ cols, rows }) => {
          if (!opened) return;
          invoke("pty_resize", { id, cols, rows }).catch(() => undefined);
        });
        unlisteners.push(() => data.dispose(), () => resize.dispose());
        // Fit once more now that the process exists, so it starts at the
        // real size rather than the 80x24 default.
        fit.fit();
        term.focus();
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
      observer.disconnect();
      themeObserver.disconnect();
      for (const off of unlisteners) off();
      if (opened) invoke("pty_close", { id }).catch(() => undefined);
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
      termRef.current?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [active]);

  return <div ref={hostRef} className={`term-host${active ? "" : " term-hidden"}`} />;
}
