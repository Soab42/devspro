import { DragEvent, Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { listen } from "@tauri-apps/api/event";
import { SESSIONS_ENDED_EVENT } from "../../../app/backgroundSessions";
import { openTerminalWindow } from "../../../app/popout";
import { Button, IconButton, Select, Spinner } from "../../../ui";
import {
  ChevronRight,
  LibraryBig,
  Monitor,
  PanelLeft,
  PictureInPicture2,
  Plus,
  SquareSplitHorizontal,
  SquareSplitVertical,
  Server,
  X,
} from "../../../ui/icons";
import { Icon } from "../../../ui/Icon";
import { PaneLayout } from "../../../ui/PaneLayout";
import { TerminalWorkspace, clampAside, hideNextHook } from "../../../ui/Terminal";
import { sftpConnect, sftpDisconnect, sftpUpload, sftpUploadTemp, termRemoteCwd, writePty, type SshHost } from "../../ssh/api";
import HostPicker from "../../ssh/components/HostPicker";
import { useConnectFlow } from "../../ssh/components/useConnectFlow";
import { elementAtPoint, endFileDrag, hasFileDrag, readFileDrag, useOsFileDrop, type Point } from "../../ssh/dnd";
import { baseName, cdCommand, errorMessage, followHookCommand, hostLabel, joinPath, shellQuote } from "../../ssh/utils";
import {
  EMPTY_WORKSPACE,
  activeTerminalWindow,
  editorKey,
  focusedSession,
  newSession,
  sidebarTerminalWindow,
  workspaceReducer,
  type EditorFile,
  type ShellCommand as PtyCommand,
  type TerminalSession,
  type WorkspaceState,
} from "../workspace";
import { createWorkspaceStore, keptWorkspace } from "../workspaceStore";
import HostStatsBar from "./HostStatsBar";
import TerminalSidebar, { type RemoteBrowser } from "./TerminalSidebar";

/* Lazy: xterm.js and CodeMirror are a few hundred kB each that only this
   view needs. The wrapper (TerminalWorkspace) is a plain layout component
   and is imported eagerly. */
const Terminal = lazy(() => import("../../../ui/Terminal"));
const EditorGroup = lazy(() => import("./EditorGroup"));

const CommandsPanel = lazy(() => import("../../commands/components/CommandsPanel"));

const ASIDE_KEY = "devspro.terminal.aside";
const ASIDE_WIDTH_KEY = "devspro.terminal.asideWidth";
const COMMANDS_KEY = "devspro.terminal.commands";
const COMMANDS_WIDTH_KEY = "devspro.terminal.commandsWidth";

/** The command library on the right: shut until asked for, unlike the file browser. */
function readCommandsPanel(): { open: boolean; width: number } {
  try {
    return {
      open: localStorage.getItem(COMMANDS_KEY) === "1",
      width: clampAside(Number(localStorage.getItem(COMMANDS_WIDTH_KEY)) || 300),
    };
  } catch {
    return { open: false, width: 300 };
  }
}

function readAside(): { open: boolean; width: number } {
  try {
    return {
      // Open unless it was closed deliberately: the browser is the point.
      open: localStorage.getItem(ASIDE_KEY) !== "0",
      width: clampAside(Number(localStorage.getItem(ASIDE_WIDTH_KEY)) || 260),
    };
  } catch {
    return { open: true, width: 260 };
  }
}

/** What the page can ask the workspace to do; handed back through `onReady`. */
export interface WorkspaceHandle {
  /** A new window with one terminal running `command`; a local one can start in `startIn`. */
  openWindow: (command: PtyCommand, startIn?: string) => void;
  /** Split the focused pane with a new terminal running `command`. */
  split: (command: PtyCommand, direction: "row" | "column") => void;
  /** Move the focused pane into its own window. Resolves false when there is no pane to move. */
  popOutFocused: () => Promise<boolean>;
}

/**
 * The tmux-shaped terminal view: windows in a strip, panes in each, an
 * optional file browser that follows the focused shell, editor tabs for
 * files opened from it, and pop-out of any pane into its own OS window.
 *
 * Every terminal in every window stays mounted: hidden windows are
 * `display: none`, never unmounted, so their shells and scrollback survive.
 */
export default function TerminalsTab({
  hosts,
  hidden,
  initial,
  solo = false,
  actionsPortal,
  tabsPortal,
  onReady,
  onCountChange,
  onError,
  onAddHost,
  keep,
}: {
  hosts: SshHost[];
  /** Keeps the sessions mounted while another view is shown. */
  hidden: boolean;
  /** A session started elsewhere to adopt on mount (a popped-out window). */
  initial?: { session: TerminalSession; attached: true };
  /** A popped-out window: no pop-out button, no window strip until a second window exists. */
  solo?: boolean;
  /** Where the toolbar buttons go — the page's top bar. In the workspace toolbar when absent. */
  actionsPortal?: Element | null;
  /** Where the window strip goes — beside the page title. In the workspace toolbar when absent. */
  tabsPortal?: Element | null;
  onReady?: (handle: WorkspaceHandle) => void;
  onCountChange?: (open: number) => void;
  onError: (message: string) => void;
  /** Where "Add a host" goes when no SSH host is saved; without it the launcher just says so. */
  onAddHost?: () => void;
  /**
   * Keep this workspace, and its shells, running while the page is away,
   * under this key; the page picks it up again when it comes back. Without
   * it, leaving ends every shell, as a popped-out window's should.
   */
  keep?: string;
}) {
  const [store] = useState(() =>
    keep
      ? keptWorkspace(keep, "terminal")
      : createWorkspaceStore(
          initial
            ? workspaceReducer(EMPTY_WORKSPACE, { type: "open-window", session: initial.session, windowId: crypto.randomUUID() })
            : EMPTY_WORKSPACE,
        ),
  );
  const state: WorkspaceState = useSyncExternalStore(store.subscribe, store.get);
  // The store's own dispatch: a shell that exits, or changes folder, while
  // the page is away still updates the kept workspace.
  const dispatch = store.dispatch;

  // A closed pane ends its kept shell. (Leaving the page closes no pane.)
  const sessionIds = useRef(Object.keys(state.sessions));
  useEffect(() => {
    const now = new Set(Object.keys(state.sessions));
    const gone = sessionIds.current.filter((id) => !now.has(id));
    sessionIds.current = [...now];
    if (keep && gone.length > 0) {
      void import("../../../ui/Terminal").then(({ endTerminalSession }) => gone.forEach(endTerminalSession));
    }
  }, [keep, state.sessions]);
  const [aside, setAside] = useState(readAside);
  const [commandsPanel, setCommandsPanel] = useState(readCommandsPanel);
  const connectFlow = useConnectFlow();

  // Read through refs so the handle given to the page is stable: a new
  // session's title depends on what is open, but the page needn't re-render
  // for that.
  const stateRef = useRef(state);
  stateRef.current = state;
  const hostsRef = useRef(hosts);
  hostsRef.current = hosts;

  // Every new ssh terminal — the host picker, a host row's `?host=`, a
  // split — starts through here, so a key file ssh would refuse is caught
  // before ssh runs rather than read about in its output.
  const { prepareOpenSsh } = connectFlow;
  const whenReady = useCallback(
    (command: PtyCommand, start: () => void) => {
      if (command === "local") return start();
      prepareOpenSsh(command.ssh).then(
        (ok) => ok && start(),
        (e) => onError(errorMessage(e)),
      );
    },
    [prepareOpenSsh, onError],
  );

  const openWindow = useCallback(
    (command: PtyCommand, startIn?: string) =>
      whenReady(command, () =>
        dispatch({ type: "open-window", session: newSession(stateRef.current, command, hostsRef.current, startIn), windowId: crypto.randomUUID() }),
      ),
    [whenReady],
  );
  const split = useCallback(
    (command: PtyCommand, direction: "row" | "column") =>
      whenReady(command, () =>
        dispatch({
          type: "split",
          session: newSession(stateRef.current, command, hostsRef.current),
          direction,
          splitId: crypto.randomUUID(),
        }),
      ),
    [whenReady],
  );
  const popOutFocused = useCallback(async () => {
    const session = focusedSession(stateRef.current);
    if (!session) return false;
    await openTerminalWindow(session.id, session.command, session.title);
    return true;
  }, []);
  useEffect(() => onReady?.({ openWindow, split, popOutFocused }), [onReady, openWindow, split, popOutFocused]);

  const openCount = Object.values(state.sessions).filter((s) => !s.exited).length;
  useEffect(() => onCountChange?.(openCount), [onCountChange, openCount]);

  function toggleAside() {
    setAside((a) => {
      const next = { ...a, open: !a.open };
      try {
        localStorage.setItem(ASIDE_KEY, next.open ? "1" : "0");
      } catch {
        /* private mode */
      }
      return next;
    });
  }
  // A host picked from the empty page logs the file browser in alongside the
  // terminal. The browser connects whenever a remote pane is focused and its
  // panel is open, so opening the panel is all that takes.
  function logIn(host: SshHost) {
    if (!aside.open) toggleAside();
    openWindow({ ssh: host.id });
  }
  function resizeAside(width: number) {
    setAside((a) => ({ ...a, width }));
    try {
      localStorage.setItem(ASIDE_WIDTH_KEY, String(width));
    } catch {
      /* private mode */
    }
  }
  function toggleCommands() {
    setCommandsPanel((c) => {
      const next = { ...c, open: !c.open };
      try {
        localStorage.setItem(COMMANDS_KEY, next.open ? "1" : "0");
      } catch {
        /* private mode */
      }
      return next;
    });
  }
  function resizeCommands(width: number) {
    setCommandsPanel((c) => ({ ...c, width }));
    try {
      localStorage.setItem(COMMANDS_WIDTH_KEY, String(width));
    } catch {
      /* private mode */
    }
  }

  /* ---- the follow-along browser's remote sessions, one per host ---- */
  const [sftp, setSftp] = useState<Record<string, string>>(() => ({ ...store.sftp }));
  useEffect(() => {
    store.sftp = sftp;
  }, [store, sftp]);
  const [sftpConnecting, setSftpConnecting] = useState<string | null>(null);
  const [sftpFailed, setSftpFailed] = useState<Record<string, string>>({});
  const attachedId = initial?.session.id ?? null;

  /** Resolves with the session, or null when it failed or was cancelled. */
  async function connectBrowser(host: SshHost): Promise<string | null> {
    setSftpConnecting(host.id);
    setSftpFailed((prev) => {
      const next = { ...prev };
      delete next[host.id];
      return next;
    });
    try {
      const id = await connectFlow.run(host, (creds) => sftpConnect(host.id, creds));
      if (id !== null) setSftp((prev) => ({ ...prev, [host.id]: id }));
      else setSftpFailed((prev) => ({ ...prev, [host.id]: "Connection cancelled." }));
      return id;
    } catch (e) {
      setSftpFailed((prev) => ({ ...prev, [host.id]: errorMessage(e) }));
      return null;
    } finally {
      setSftpConnecting(null);
    }
  }
  function disconnectBrowser(hostId: string) {
    const id = sftp[hostId];
    setSftp((prev) => {
      const next = { ...prev };
      delete next[hostId];
      return next;
    });
    if (id) void sftpDisconnect(id).catch(() => undefined);
  }
  // The main window was closed (to the tray): the shells were ended by the
  // backend, and the browser's connections end with them.
  useEffect(() => {
    let off: (() => void) | null = null;
    let gone = false;
    void listen(SESSIONS_ENDED_EVENT, () => {
      for (const id of Object.values(sftpRef.current)) void sftpDisconnect(id).catch(() => undefined);
      store.sftp = {};
      setSftp({});
    }).then((unlisten) => (gone ? unlisten() : (off = unlisten)));
    return () => {
      gone = true;
      off?.();
    };
  }, [store]);

  // Browser sessions are ours; drop them with the page, unless the
  // workspace is kept, when they carry on with its shells.
  const sftpRef = useRef(sftp);
  sftpRef.current = sftp;
  useEffect(() => {
    return () => {
      if (keep) return;
      for (const id of Object.values(sftpRef.current)) void sftpDisconnect(id).catch(() => undefined);
    };
  }, [keep]);

  const focused = focusedSession(state);
  const focusedHostId = focused && focused.command !== "local" ? focused.command.ssh : null;
  const focusedHost = focusedHostId ? (hosts.find((h) => h.id === focusedHostId) ?? null) : null;
  const remote: RemoteBrowser | null = useMemo(
    () =>
      focusedHostId
        ? {
            sessionId: sftp[focusedHostId] ?? null,
            connecting: sftpConnecting === focusedHostId,
            failure: sftpFailed[focusedHostId] ?? null,
            connect: () => {
              if (focusedHost) void connectBrowser(focusedHost);
            },
            disconnect: () => disconnectBrowser(focusedHostId),
          }
        : null,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [focusedHostId, focusedHost, sftp, sftpConnecting, sftpFailed],
  );

  const send = useCallback(
    (id: string, text: string) => {
      void writePty(id, text).catch((e) => onError(errorMessage(e)));
    },
    [onError],
  );
  const recordCwd = useCallback((id: string, cwd: string) => dispatch({ type: "cwd", id, cwd }), []);

  // The command library types into the focused pane, then gives it the
  // keyboard back, so Enter (or an edit first) lands in the shell.
  const typeTarget = focused && !focused.exited ? focused.id : null;
  const typeIntoFocused = useCallback(
    (text: string) => {
      if (!typeTarget) return;
      send(typeTarget, text);
      document
        .querySelector<HTMLTextAreaElement>(`[data-session-id="${CSS.escape(typeTarget)}"] .xterm-helper-textarea`)
        ?.focus();
    },
    [typeTarget, send],
  );

  /* ---- drops onto a terminal pane ---- */
  // A file row from the browser (or the SFTP page) types its path, quoted,
  // into the shell; a run of plain text is typed as it is. Files from the
  // desktop type their paths into a local shell; a remote shell can't see
  // them, so they are uploaded to where the shell is first and the remote
  // paths typed instead.
  const [dropPane, setDropPane] = useState<string | null>(null);

  function paneDragOver(e: DragEvent<HTMLDivElement>) {
    const types = Array.from(e.dataTransfer.types);
    if (!hasFileDrag(e.dataTransfer) && !types.includes("text/plain")) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
  }

  function paneDrop(e: DragEvent<HTMLDivElement>, id: string) {
    const payload = readFileDrag(e.dataTransfer);
    const text = payload ? `${shellQuote(payload.path)} ` : e.dataTransfer.getData("text/plain");
    endFileDrag();
    if (!text) return;
    e.preventDefault();
    send(id, text);
  }

  async function dropFilesOnPane(id: string, paths: string[]) {
    const s = stateRef.current.sessions[id];
    if (!s || s.exited) return;
    if (s.command === "local") {
      send(id, `${paths.map(shellQuote).join(" ")} `);
      return;
    }
    const sftpId = sftpRef.current[s.command.ssh];
    if (!sftpId) {
      onError("Connect the file browser to this host first, so the files can be uploaded to it.");
      return;
    }
    if (!s.cwd) {
      onError("The shell has not reported its folder yet, so there is nowhere to upload to.");
      return;
    }
    try {
      const uploaded: string[] = [];
      for (const p of paths) {
        const target = joinPath(s.cwd, baseName(p));
        await sftpUpload(sftpId, p, target);
        uploaded.push(target);
      }
      send(id, `${uploaded.map(shellQuote).join(" ")} `);
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  // An image pasted into a remote shell is copied to the host first, over
  // the file browser's connection (made now if there isn't one), and the
  // remote path typed instead of the local one.
  async function pasteImageInto(id: string, local: string): Promise<string | null> {
    const s = stateRef.current.sessions[id];
    if (!s || s.command === "local") return local;
    const hostId = s.command.ssh;
    const host = hostsRef.current.find((h) => h.id === hostId);
    const sftpId = sftpRef.current[hostId] ?? (host ? await connectBrowser(host) : null);
    if (!sftpId) {
      onError("The image could not be pasted: connect the file browser to this host, so it can be uploaded there.");
      return null;
    }
    return sftpUploadTemp(sftpId, local);
  }

  const paneAt = (point: Point) => elementAtPoint<HTMLDivElement>(document.body, point, ".term-pane")?.dataset.sessionId ?? null;
  useOsFileDrop({
    onHover: (point) => setDropPane(point ? paneAt(point) : null),
    onDrop: (paths, point) => {
      const id = paneAt(point);
      if (id) void dropFilesOnPane(id, paths);
    },
  });

  // The OSC 7 hook is typed into a remote shell exactly once per session,
  // whatever remounts the terminal component goes through. An adopted
  // session (pop-out) already had it from the window it came from.
  const hooked = useRef(store.hooked);
  if (attachedId) hooked.current.add(attachedId);
  // The same set as state, for the Files panel: its "send the hook again"
  // offer only makes sense once the shell is up and the first one went in.
  const [hookedIds, setHookedIds] = useState<ReadonlySet<string>>(() => new Set(hooked.current));
  const installHook = useCallback(
    (id: string) => {
      if (hooked.current.has(id)) return false;
      hooked.current.add(id);
      setHookedIds(new Set(hooked.current));
      send(id, followHookCommand());
      return true;
    },
    [send],
  );
  const resendHook = useCallback(
    (id: string) => {
      hideNextHook(id);
      send(id, followHookCommand());
    },
    [send],
  );

  // A remote shell's folder is read in the background when the server allows
  // it (see remote_cwd.rs), so nothing has to be typed into the terminal.
  // Only when that finds nothing is the hook typed, hidden, as before.
  const [trackedIds, setTrackedIds] = useState<ReadonlySet<string>>(() => new Set(store.tracked));
  const followRemote = useCallback(
    async (id: string, hostId: string) => {
      if (hooked.current.has(id) || store.tracked.has(id)) return;
      // The browser's connection is usually up by the first prompt; give it
      // a moment if not, then fall back.
      let sftpId: string | undefined;
      for (let waited = 0; waited < 4000 && !(sftpId = sftpRef.current[hostId]); waited += 250) {
        await new Promise((resolve) => window.setTimeout(resolve, 250));
      }
      const cwd = sftpId ? await termRemoteCwd(sftpId, id).catch(() => null) : null;
      if (cwd) {
        store.tracked.add(id);
        setTrackedIds(new Set(store.tracked));
        recordCwd(id, cwd);
        return;
      }
      hideNextHook(id);
      installHook(id);
    },
    [store, recordCwd, installHook],
  );

  // The focused tracked shell's folder, followed while this page is up.
  const trackedFocus = focused && focusedHostId && trackedIds.has(focused.id) ? focused.id : null;
  const trackedSftp = focusedHostId ? (sftp[focusedHostId] ?? null) : null;
  useEffect(() => {
    if (!trackedFocus || !trackedSftp) return;
    let stopped = false;
    const tick = async () => {
      if (document.hidden) return;
      const cwd = await termRemoteCwd(trackedSftp, trackedFocus).catch(() => null);
      if (!stopped && cwd && stateRef.current.sessions[trackedFocus]?.cwd !== cwd) recordCwd(trackedFocus, cwd);
    };
    const timer = window.setInterval(() => void tick(), 1500);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [trackedFocus, trackedSftp, recordCwd]);

  const openFile = useCallback(
    (path: string) => {
      const session = focusedSession(stateRef.current);
      const file: EditorFile =
        session && session.command !== "local"
          ? { path, sftp: sftpRef.current[session.command.ssh] ?? null, hostName: focusedHost?.name ?? null }
          : { path, sftp: null, hostName: null };
      if (session && session.command !== "local" && !file.sftp) {
        onError("The file browser is not connected yet.");
        return;
      }
      dispatch({ type: "open-editor", windowId: crypto.randomUUID(), file });
    },
    [focusedHost, onError],
  );

  async function popOut(session: TerminalSession) {
    try {
      await openTerminalWindow(session.id, session.command, session.title);
      // The pane leaves the tree once the new window has adopted the
      // session — see Terminal's onDetached.
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  const activeWindow = state.windows.find((w) => w.id === state.activeWindow) ?? null;
  const terminalWindow = sidebarTerminalWindow(state);
  const showStrip = state.windows.length > 0 && (!solo || state.windows.length > 1);

  const strip = showStrip && (
    <div className="term-windows" role="tablist" aria-label="Terminal windows">
      {state.windows.map((w) => {
        const activeFile = w.kind === "editor" ? w.files.find((f) => editorKey(f) === w.active) : undefined;
        const label =
          w.kind === "editor"
            ? `${Object.values(w.dirty).some(Boolean) ? "\u25cf " : ""}${activeFile ? baseName(activeFile.path) : "Editor"}${w.files.length > 1 ? ` +${w.files.length - 1}` : ""}`
            : (state.sessions[w.focused]?.title ?? "Terminal");
        return (
          <button
            key={w.id}
            type="button"
            role="tab"
            aria-selected={w.id === state.activeWindow}
            className={`term-window-tab${w.id === state.activeWindow ? " term-window-tab-active" : ""}`}
            onClick={() => dispatch({ type: "activate-window", windowId: w.id })}
            onAuxClick={(e) => {
              if (e.button === 1) dispatch({ type: "close-window", windowId: w.id });
            }}
            title={w.kind === "editor" ? `${w.files.length} open file${w.files.length === 1 ? "" : "s"}` : `Window ${w.name}: ${label}`}
          >
            <span className="term-window-ordinal">{w.name}</span>
            <span className="term-window-label">{label}</span>
            <span
              role="button"
              tabIndex={-1}
              aria-label="Close"
              className="term-window-close"
              onClick={(e) => {
                e.stopPropagation();
                dispatch({ type: "close-window", windowId: w.id });
              }}
            >
              <Icon icon={X} size="xs" />
            </span>
          </button>
        );
      })}
    </div>
  );

  const actions = (
    <>
      <Button
        size="sm"
        icon={PanelLeft}
        title={aside.open ? "Hide the file browser" : "Show the file browser beside the terminal"}
        aria-pressed={aside.open}
        className={aside.open ? "term-toggle-on" : undefined}
        onClick={toggleAside}
      >
        Files
      </Button>
      <Button
        size="sm"
        icon={LibraryBig}
        title={commandsPanel.open ? "Hide the command library" : "Show the command library beside the terminal"}
        aria-pressed={commandsPanel.open}
        className={commandsPanel.open ? "term-toggle-on" : undefined}
        onClick={toggleCommands}
      >
        Commands
      </Button>
      {terminalWindow && (
        <>
          <IconButton icon={SquareSplitHorizontal} label="Split side by side" onClick={() => split(focused?.command ?? "local", "row")} />
          <IconButton icon={SquareSplitVertical} label="Split top and bottom" onClick={() => split(focused?.command ?? "local", "column")} />
        </>
      )}
      {activeWindow && (
        <IconButton
          icon={X}
          label={activeWindow.kind === "editor" ? "Close editor" : "Close window"}
          onClick={() => dispatch({ type: "close-window", windowId: activeWindow.id })}
        />
      )}
      <Button variant="secondary" size="sm" icon={Plus} onClick={() => openWindow("local")}>
        New window
      </Button>
      <Select
        aria-label="Open a terminal on a saved SSH host"
        size="sm"
        value=""
        placeholder={hosts.length === 0 ? "No SSH hosts saved" : "SSH host…"}
        disabled={hosts.length === 0}
        onChange={(id) => {
          if (hosts.some((h) => h.id === id)) openWindow({ ssh: id });
        }}
        options={hosts.map((h) => ({ value: h.id, label: `${h.name} (${hostLabel(h)})` }))}
        menuWidth="content"
      />
    </>
  );

  return (
    <>
      {/* Outside the workspace: it shows `empty` in place of its children, and
          the header's buttons (the SSH host picker among them) must stay up
          with no terminal open. */}
      {actionsPortal && createPortal(<div className="term-page-actions">{actions}</div>, actionsPortal)}
      {tabsPortal && strip && createPortal(strip, tabsPortal)}
      <TerminalWorkspace
        hidden={hidden}
        tabs={tabsPortal ? undefined : strip}
        actions={actionsPortal ? undefined : actions}
        aside={
          aside.open && (
            <TerminalSidebar
              session={focused}
              host={focusedHost}
              remote={remote}
              onCwd={recordCwd}
              onSend={send}
              hookSent={focused ? hookedIds.has(focused.id) : false}
              onResendHook={resendHook}
              onOpenFile={openFile}
              onClose={toggleAside}
            />
          )
        }
        asideWidth={aside.width}
        onAsideResize={resizeAside}
        rightAside={
          commandsPanel.open && (
            <Suspense fallback={<Spinner />}>
              <CommandsPanel write={typeTarget ? typeIntoFocused : null} onClose={toggleCommands} onError={onError} />
            </Suspense>
          )
        }
        rightAsideWidth={commandsPanel.width}
        onRightAsideResize={resizeCommands}
        footer={<HostStatsBar session={focused} sftpId={focusedHostId ? (sftp[focusedHostId] ?? null) : null} />}
        empty={
          state.windows.length === 0 && (
            <TerminalLauncher
              hosts={hosts}
              onLocal={() => openWindow("local")}
              onHost={logIn}
              onAddHost={onAddHost}
            />
          )
        }
      >
        <Suspense fallback={<Spinner />}>
          {state.windows.map((w) =>
            w.kind === "editor" ? (
              <EditorGroup
                key={w.id}
                window={w}
                hidden={w.id !== state.activeWindow || hidden}
                onActivate={(key) => dispatch({ type: "activate-file", key })}
                onClose={(key) => dispatch({ type: "close-file", key })}
                onDirty={(key, dirty) => dispatch({ type: "editor-dirty", key, dirty })}
              />
            ) : (
              <PaneLayout
                key={w.id}
                root={w.root}
                hidden={w.id !== state.activeWindow}
                focusedId={w.focused}
                ariaLabel={`Terminal window ${w.name}`}
                onFocus={(paneId) => dispatch({ type: "focus", windowId: w.id, paneId })}
                onSizes={(splitId, sizes) => dispatch({ type: "resize", windowId: w.id, splitId, sizes })}
                renderPane={(paneId) => {
                  const s = state.sessions[paneId];
                  if (!s) return null;
                  const isFocused = w.focused === paneId && w.id === activeTerminalWindow(state)?.id;
                  return (
                    <div
                      className={["term-pane", s.exited && "term-pane-exited", dropPane === s.id && "term-pane-drop"].filter(Boolean).join(" ")}
                      data-session-id={s.id}
                      onDragOver={paneDragOver}
                      onDrop={(e) => paneDrop(e, s.id)}
                    >
                      <div className="term-pane-bar">
                        <span className="term-pane-title" title={s.title}>
                          {s.title}
                          {s.exited && " (exited)"}
                        </span>
                        <span className="term-pane-bar-actions">
                          <IconButton icon={SquareSplitHorizontal} label="Split side by side" size="sm" onClick={() => split(s.command, "row")} />
                          <IconButton icon={SquareSplitVertical} label="Split top and bottom" size="sm" onClick={() => split(s.command, "column")} />
                          {!solo && !s.exited && (
                            <IconButton icon={PictureInPicture2} label="Pop out into its own window" size="sm" onClick={() => void popOut(s)} />
                          )}
                          <IconButton icon={X} label="Close pane" size="sm" onClick={() => dispatch({ type: "close-pane", id: s.id })} />
                        </span>
                      </div>
                      <Terminal
                        id={s.id}
                        command={s.command}
                        active={w.id === state.activeWindow && !hidden}
                        focused={isFocused}
                        attached={s.id === attachedId}
                        persist={!!keep}
                        onExit={() => dispatch({ type: "exited", id: s.id })}
                        onError={onError}
                        onCwd={(cwd) => recordCwd(s.id, cwd)}
                        onPrompt={
                          s.command === "local"
                            ? s.startIn
                              ? () => send(s.id, cdCommand(s.startIn as string))
                              : undefined
                            : () => {
                                const command = s.command;
                                if (command !== "local") void followRemote(s.id, command.ssh);
                              }
                        }
                        onDetached={() => dispatch({ type: "detach", id: s.id })}
                        onPasteImage={(local) => pasteImageInto(s.id, local)}
                      />
                    </div>
                  );
                }}
              />
            ),
          )}
        </Suspense>
        {connectFlow.element}
      </TerminalWorkspace>
    </>
  );
}

/**
 * What an empty workspace offers: the two places a terminal can run, as two
 * matching cards. SSH opens the saved-host picker SFTP uses; with no host
 * saved it goes to add one instead.
 */
function TerminalLauncher({
  hosts,
  onLocal,
  onHost,
  onAddHost,
}: {
  hosts: SshHost[];
  onLocal: () => void;
  onHost: (host: SshHost) => void;
  onAddHost?: () => void;
}) {
  const [picking, setPicking] = useState(false);
  const noHosts = hosts.length === 0;
  const sshDetail = noHosts
    ? onAddHost
      ? "No saved hosts yet. Add one"
      : "No saved hosts yet. Save one in SSH"
    : `${hosts.length} saved ${hosts.length === 1 ? "host" : "hosts"}`;

  return (
    <div className="term-launch">
      <div className="term-launch-head">
        <p className="term-launch-title">Open a terminal</p>
        <p className="term-launch-desc">Choose where it runs. You can split it, add windows, or pop a pane out later.</p>
      </div>
      <div className="term-launch-grid">
        <button type="button" className="term-launch-option" onClick={onLocal}>
          <span className="term-launch-icon">
            <Icon icon={Monitor} size="md" />
          </span>
          <span className="term-launch-text">
            <strong>Local terminal</strong>
            <span>A shell on this computer</span>
          </span>
          <Icon icon={ChevronRight} size="sm" className="term-launch-chevron" />
        </button>
        <button
          type="button"
          className="term-launch-option"
          disabled={noHosts && !onAddHost}
          onClick={() => (noHosts ? onAddHost?.() : setPicking(true))}
        >
          <span className="term-launch-icon">
            <Icon icon={Server} size="md" />
          </span>
          <span className="term-launch-text">
            <strong>SSH server</strong>
            <span>{sshDetail}</span>
          </span>
          <Icon icon={noHosts ? Plus : ChevronRight} size="sm" className="term-launch-chevron" />
        </button>
      </div>
      {picking && (
        <HostPicker
          hosts={hosts}
          description="Pick a saved host to open a terminal on it."
          onPick={(h) => {
            setPicking(false);
            onHost(h);
          }}
          onClose={() => setPicking(false)}
        />
      )}
    </div>
  );
}
