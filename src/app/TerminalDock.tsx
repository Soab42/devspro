import { PointerEvent as ReactPointerEvent, Suspense, lazy, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, IconButton, Spinner } from "../ui";
import { OctagonX, PanelRightClose, RotateCw } from "../ui/icons";
import { clampDock } from "./terminalDockState";
import "./TerminalDock.css";

// xterm is a few hundred kB; it loads the first time the dock opens.
const Terminal = lazy(() => import("../ui/Terminal"));

export interface TerminalDockProps {
  open: boolean;
  width: number;
  onWidthChange: (width: number) => void;
  /** Hide the panel. The shell keeps running. */
  onClose: () => void;
  /** Narrow window: cover the module instead of sitting beside it. */
  overlay?: boolean;
}

const CWD_POLL_MS = 3000;

function newSessionId(): string {
  return `dock-${crypto.randomUUID()}`;
}

/**
 * One local shell docked on the right edge of the main window.
 *
 * Close HIDES the panel: the terminal stays mounted, so the shell, its
 * scrollback and any running command survive and reopening shows the same
 * session. "End session" is the explicit kill: it unmounts the terminal (which
 * runs `pty_close`) and the next open starts a fresh shell. Nothing is spawned
 * until the dock is first opened.
 */
export function TerminalDock({ open, width, onWidthChange, onClose, overlay = false }: TerminalDockProps) {
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [exited, setExited] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cwd, setCwd] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const widthRef = useRef(width);
  widthRef.current = width;

  // First open, and the first open after an ended session, starts a shell.
  useEffect(() => {
    if (open && sessionId === null) {
      setExited(false);
      setError(null);
      setCwd(null);
      setSessionId(newSessionId());
    }
  }, [open, sessionId]);

  // The shell's own directory, while someone can see it. OSC 7 (onCwd) is
  // faster where the shell emits it; this covers the ones that don't.
  useEffect(() => {
    if (!open || !sessionId || exited) return;
    let cancelled = false;
    const poll = () =>
      invoke<string | null>("pty_cwd", { id: sessionId })
        .then((path) => {
          if (!cancelled && path) setCwd(path);
        })
        .catch(() => undefined);
    const timer = window.setInterval(poll, CWD_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [open, sessionId, exited]);

  function restart() {
    setExited(false);
    setError(null);
    setCwd(null);
    setSessionId(newSessionId());
  }

  function endSession() {
    setSessionId(null);
    onClose();
  }

  function startDrag(e: ReactPointerEvent<HTMLDivElement>) {
    if (e.button !== 0) return;
    e.preventDefault();
    const handle = e.currentTarget;
    const origin = e.clientX;
    const start = widthRef.current;
    handle.setPointerCapture(e.pointerId);
    setDragging(true);
    // The dock is on the right, so dragging left makes it wider.
    const move = (ev: PointerEvent) => onWidthChange(clampDock(start + origin - ev.clientX, window.innerWidth));
    const stop = () => {
      setDragging(false);
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", stop);
      handle.removeEventListener("pointercancel", stop);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", stop);
    handle.addEventListener("pointercancel", stop);
  }

  const classes = ["terminal-dock", !open && "terminal-dock-hidden", overlay && "terminal-dock-overlay"]
    .filter(Boolean)
    .join(" ");

  return (
    <aside
      className={classes}
      aria-label="Terminal"
      aria-hidden={!open}
      style={overlay ? undefined : { width: clampDock(width, window.innerWidth) }}
    >
      <div
        className={`terminal-dock-handle${dragging ? " terminal-dock-handle-dragging" : ""}`}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize terminal"
        onPointerDown={startDrag}
      />
      <div className="terminal-dock-header">
        <span className="terminal-dock-title">Terminal</span>
        <span className="terminal-dock-cwd" title={cwd ?? undefined}>
          {cwd}
        </span>
        <IconButton icon={RotateCw} label="Restart shell" size="sm" onClick={restart} />
        <IconButton icon={OctagonX} label="End session" size="sm" onClick={endSession} />
        <IconButton icon={PanelRightClose} label="Hide terminal" size="sm" onClick={onClose} />
      </div>
      <div className="terminal-dock-body">
        {sessionId && (
          <Suspense fallback={<Spinner size="md" />}>
            <Terminal
              key={sessionId}
              id={sessionId}
              command="local"
              active={open}
              onExit={() => setExited(true)}
              onError={setError}
              onCwd={setCwd}
            />
          </Suspense>
        )}
        {(exited || error) && (
          <div className="terminal-dock-exited" role="status">
            <span>{error ?? "The shell exited."}</span>
            <Button size="sm" onClick={restart}>
              Restart
            </Button>
          </div>
        )}
      </div>
    </aside>
  );
}
