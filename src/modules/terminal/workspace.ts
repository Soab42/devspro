import {
  leaf,
  neighbourLeaf,
  removeLeaf,
  setSplitSizes,
  splitLeaf,
  type PaneNode,
  type SplitDirection,
} from "../../ui/paneTree";
import type { PtyCommand, SshHost } from "../ssh/api";

/** What the terminal workspace opens: a shell, here or on a host. Programs run
 *  through `exec` belong to the AI agents module, which has its own sessions. */
export type ShellCommand = Exclude<PtyCommand, { exec: unknown }>;
import { baseName, terminalTitle } from "../ssh/utils";

/**
 * The terminal workspace, tmux-shaped: any number of *windows* (the tab
 * strip), each a tree of *panes*, each pane one shell session.
 *
 * A pane's id doubles as its PTY session id — the tree only ever refers to
 * sessions by id, and the session map holds what they are. A pane popped
 * out into its own OS window leaves the tree here but keeps its id, which
 * is what lets the new window pick the process up.
 */

export interface TerminalSession {
  id: string;
  title: string;
  command: ShellCommand;
  exited: boolean;
  /** Last working directory the shell reported, for the file browser to follow. */
  cwd: string | null;
  /** A folder to `cd` into once the prompt appears — "Open in Terminal" from Files. Local shells only. */
  startIn?: string;
}

export interface TerminalWindow {
  kind: "terminal";
  id: string;
  /** Ordinal, tmux-style; the pane's own title is in the pane bar. */
  name: string;
  root: PaneNode;
  focused: string;
}

/** A file open for editing: where it lives and how to reach it. */
export interface EditorFile {
  path: string;
  /** Null for a local file; otherwise the SFTP session that can read and write it. */
  sftp: string | null;
  /** The host's name, for the tab and the title bar; null when local. */
  hostName: string | null;
}

/** The one tab in the strip that holds editors: files as inner tabs, like an IDE's editor group. */
export interface EditorWindow {
  kind: "editor";
  id: string;
  name: string;
  files: EditorFile[];
  /** Key (see `editorKey`) of the file in front. */
  active: string;
  dirty: Record<string, boolean>;
}

/** Identifies a file across hosts: the same path on two hosts is two files. */
export function editorKey(file: EditorFile): string {
  return `${file.sftp ?? "local"}:${file.path}`;
}

export type WorkspaceWindow = TerminalWindow | EditorWindow;

export interface WorkspaceState {
  windows: WorkspaceWindow[];
  activeWindow: string | null;
  /** The terminal window last in front, so an editor tab still knows which shell it belongs with. */
  lastTerminalWindow: string | null;
  sessions: Record<string, TerminalSession>;
}

export const EMPTY_WORKSPACE: WorkspaceState = { windows: [], activeWindow: null, lastTerminalWindow: null, sessions: {} };

export type WorkspaceAction =
  | { type: "open-window"; session: TerminalSession; windowId: string }
  | { type: "split"; session: TerminalSession; direction: SplitDirection; splitId: string }
  | { type: "close-pane"; id: string }
  | { type: "focus"; windowId: string; paneId: string }
  | { type: "activate-window"; windowId: string }
  | { type: "close-window"; windowId: string }
  | { type: "rename-window"; windowId: string; name: string }
  /** Open a file in the editor tab (created on first use), or bring it to the front if already open. */
  | { type: "open-editor"; windowId: string; file: EditorFile }
  | { type: "activate-file"; key: string }
  /** Close one file; the editor tab goes with its last file. */
  | { type: "close-file"; key: string }
  | { type: "editor-dirty"; key: string; dirty: boolean }
  | { type: "resize"; windowId: string; splitId: string; sizes: number[] }
  | { type: "exited"; id: string }
  | { type: "cwd"; id: string; cwd: string }
  /** The pane left for its own OS window: drop it from the tree, forget the session, kill nothing. */
  | { type: "detach"; id: string };

function nextWindowName(windows: WorkspaceWindow[]): string {
  const taken = new Set(windows.map((w) => w.name));
  let n = 1;
  while (taken.has(String(n))) n += 1;
  return String(n);
}

function activeWindowOf(state: WorkspaceState): WorkspaceWindow | null {
  return state.windows.find((w) => w.id === state.activeWindow) ?? null;
}

/** The active window when it holds terminals; null for an editor tab or nothing. */
export function activeTerminalWindow(state: WorkspaceState): TerminalWindow | null {
  const win = activeWindowOf(state);
  return win?.kind === "terminal" ? win : null;
}

/**
 * The terminal window whose focused pane the sidebar follows: the active
 * one, or, while an editor tab is in front, the terminal window that was
 * most recently active (tracked as `lastTerminalWindow`).
 */
export function sidebarTerminalWindow(state: WorkspaceState): TerminalWindow | null {
  const active = activeTerminalWindow(state);
  if (active) return active;
  const last = state.windows.find((w) => w.id === state.lastTerminalWindow);
  return last?.kind === "terminal" ? last : null;
}

export function focusedSession(state: WorkspaceState): TerminalSession | null {
  const win = sidebarTerminalWindow(state);
  return win ? (state.sessions[win.focused] ?? null) : null;
}

/** Every session in the workspace, whichever window it is in. */
export function allSessions(state: WorkspaceState): TerminalSession[] {
  return Object.values(state.sessions);
}

/** How many sessions of the same kind exist already, to number a new title. */
export function sessionOrdinal(state: WorkspaceState, command: ShellCommand): number {
  const same = allSessions(state).filter((s) =>
    command === "local" ? s.command === "local" : s.command !== "local" && s.command.ssh === command.ssh,
  ).length;
  return same + 1;
}

export function newSession(state: WorkspaceState, command: ShellCommand, hosts: SshHost[], startIn?: string): TerminalSession {
  const inFolder = command === "local" && startIn ? startIn : undefined;
  return {
    id: crypto.randomUUID(),
    command,
    // A shell opened on a folder is named after it, so two "Local" tabs don't
    // hide which project each one is in.
    title: inFolder ? baseName(inFolder) || "/" : terminalTitle(command, hosts, sessionOrdinal(state, command)),
    exited: false,
    cwd: null,
    ...(inFolder ? { startIn: inFolder } : {}),
  };
}

/** Removes a pane from whichever window holds it, closing the window when it empties. */
function withoutPane(state: WorkspaceState, id: string): WorkspaceState {
  const windows: WorkspaceWindow[] = [];
  let activeWindow = state.activeWindow;
  for (const win of state.windows) {
    if (win.kind === "editor") {
      windows.push(win);
      continue;
    }
    const root = removeLeaf(win.root, id);
    if (root === null) {
      if (activeWindow === win.id) activeWindow = null;
      continue;
    }
    if (root === win.root) {
      windows.push(win);
      continue;
    }
    const focused = win.focused === id ? (neighbourLeaf(win.root, id) ?? win.focused) : win.focused;
    windows.push({ ...win, root, focused });
  }
  if (activeWindow === null && windows.length > 0) {
    // Land on the neighbour of the window that closed, as a tab strip would.
    const index = state.windows.findIndex((w) => w.id === state.activeWindow);
    activeWindow = (windows[index] ?? windows[index - 1] ?? windows[0]).id;
  }
  const sessions = { ...state.sessions };
  delete sessions[id];
  return { ...state, windows, activeWindow, sessions, lastTerminalWindow: lastTerminal(windows, activeWindow, state.lastTerminalWindow) };
}

function lastTerminal(windows: WorkspaceWindow[], activeWindow: string | null, previous: string | null): string | null {
  const active = windows.find((w) => w.id === activeWindow);
  if (active?.kind === "terminal") return active.id;
  if (windows.some((w) => w.id === previous && w.kind === "terminal")) return previous;
  return windows.find((w) => w.kind === "terminal")?.id ?? null;
}

/** Closes a whole window; a tab that holds no terminals just leaves the strip. */
function withoutWindow(state: WorkspaceState, windowId: string): WorkspaceState {
  const win = state.windows.find((w) => w.id === windowId);
  if (!win) return state;
  if (win.kind === "terminal") {
    let next = state;
    for (const id of paneIdsOf(win.root)) next = withoutPane(next, id);
    return next;
  }
  const index = state.windows.indexOf(win);
  const windows = state.windows.filter((w) => w.id !== windowId);
  let activeWindow = state.activeWindow;
  if (activeWindow === windowId) activeWindow = (windows[index] ?? windows[index - 1] ?? null)?.id ?? null;
  return { ...state, windows, activeWindow, lastTerminalWindow: lastTerminal(windows, activeWindow, state.lastTerminalWindow) };
}

export function workspaceReducer(state: WorkspaceState, action: WorkspaceAction): WorkspaceState {
  switch (action.type) {
    case "open-window": {
      const win: TerminalWindow = {
        kind: "terminal",
        id: action.windowId,
        name: nextWindowName(state.windows),
        root: leaf(action.session.id),
        focused: action.session.id,
      };
      return {
        windows: [...state.windows, win],
        activeWindow: win.id,
        lastTerminalWindow: win.id,
        sessions: { ...state.sessions, [action.session.id]: action.session },
      };
    }
    case "split": {
      const win = sidebarTerminalWindow(state);
      if (!win) return workspaceReducer(state, { type: "open-window", session: action.session, windowId: action.splitId });
      const root = splitLeaf(win.root, win.focused, action.direction, leaf(action.session.id), action.splitId);
      return {
        ...state,
        activeWindow: win.id,
        lastTerminalWindow: win.id,
        windows: state.windows.map((w) => (w.id === win.id ? { ...w, root, focused: action.session.id } : w)),
        sessions: { ...state.sessions, [action.session.id]: action.session },
      };
    }
    case "close-pane":
    case "detach":
      return withoutPane(state, action.id);
    case "focus":
      return {
        ...state,
        activeWindow: action.windowId,
        lastTerminalWindow: action.windowId,
        windows: state.windows.map((w) =>
          w.id === action.windowId && w.kind === "terminal" ? { ...w, focused: action.paneId } : w,
        ),
      };
    case "activate-window": {
      const win = state.windows.find((w) => w.id === action.windowId);
      if (!win) return state;
      return {
        ...state,
        activeWindow: win.id,
        lastTerminalWindow: win.kind === "terminal" ? win.id : state.lastTerminalWindow,
      };
    }
    case "close-window":
      return withoutWindow(state, action.windowId);
    case "open-editor": {
      const key = editorKey(action.file);
      const editor = state.windows.find((w): w is EditorWindow => w.kind === "editor");
      if (!editor) {
        const win: EditorWindow = {
          kind: "editor",
          id: action.windowId,
          name: nextWindowName(state.windows),
          files: [action.file],
          active: key,
          dirty: {},
        };
        return { ...state, windows: [...state.windows, win], activeWindow: win.id };
      }
      const files = editor.files.some((f) => editorKey(f) === key) ? editor.files : [...editor.files, action.file];
      return {
        ...state,
        activeWindow: editor.id,
        windows: state.windows.map((w) => (w.id === editor.id ? { ...editor, files, active: key } : w)),
      };
    }
    case "activate-file":
      return {
        ...state,
        windows: state.windows.map((w) =>
          w.kind === "editor" && w.files.some((f) => editorKey(f) === action.key) ? { ...w, active: action.key } : w,
        ),
      };
    case "close-file": {
      const editor = state.windows.find((w): w is EditorWindow => w.kind === "editor");
      if (!editor) return state;
      const index = editor.files.findIndex((f) => editorKey(f) === action.key);
      if (index < 0) return state;
      const files = editor.files.filter((_, i) => i !== index);
      if (files.length === 0) return withoutWindow(state, editor.id);
      const dirty = { ...editor.dirty };
      delete dirty[action.key];
      const active =
        editor.active === action.key ? editorKey(files[index] ?? files[index - 1]) : editor.active;
      return { ...state, windows: state.windows.map((w) => (w.id === editor.id ? { ...editor, files, active, dirty } : w)) };
    }
    case "editor-dirty":
      return {
        ...state,
        windows: state.windows.map((w) =>
          w.kind === "editor" && (w.dirty[action.key] ?? false) !== action.dirty
            ? { ...w, dirty: { ...w.dirty, [action.key]: action.dirty } }
            : w,
        ),
      };
    case "rename-window":
      return {
        ...state,
        windows: state.windows.map((w) => (w.id === action.windowId ? { ...w, name: action.name } : w)),
      };
    case "resize":
      return {
        ...state,
        windows: state.windows.map((w) =>
          w.id === action.windowId && w.kind === "terminal"
            ? { ...w, root: setSplitSizes(w.root, action.splitId, action.sizes) }
            : w,
        ),
      };
    case "exited": {
      const session = state.sessions[action.id];
      if (!session) return state;
      return { ...state, sessions: { ...state.sessions, [action.id]: { ...session, exited: true } } };
    }
    case "cwd": {
      const session = state.sessions[action.id];
      if (!session || session.cwd === action.cwd) return state;
      return { ...state, sessions: { ...state.sessions, [action.id]: { ...session, cwd: action.cwd } } };
    }
  }
}

function paneIdsOf(node: PaneNode): string[] {
  return node.kind === "leaf" ? [node.id] : node.children.flatMap(paneIdsOf);
}
