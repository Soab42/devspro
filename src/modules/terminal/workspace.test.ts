import { describe, expect, it } from "vitest";
import { leafIds } from "../../ui/paneTree";
import {
  EMPTY_WORKSPACE,
  editorKey,
  focusedSession,
  sessionOrdinal,
  workspaceReducer,
  type TerminalSession,
  type TerminalWindow,
  type WorkspaceState,
} from "./workspace";

const term = (w: WorkspaceState["windows"][number]): TerminalWindow => {
  if (w.kind !== "terminal") throw new Error("expected a terminal window");
  return w;
};

const session = (id: string, command: TerminalSession["command"] = "local"): TerminalSession => ({
  id,
  title: id,
  command,
  exited: false,
  cwd: null,
});

function build(...steps: Array<(s: WorkspaceState) => WorkspaceState>): WorkspaceState {
  return steps.reduce((s, step) => step(s), EMPTY_WORKSPACE);
}

const open = (id: string, windowId = `w-${id}`) => (s: WorkspaceState) =>
  workspaceReducer(s, { type: "open-window", session: session(id), windowId });
const split = (id: string, direction: "row" | "column" = "row") => (s: WorkspaceState) =>
  workspaceReducer(s, { type: "split", session: session(id), direction, splitId: `split-${id}` });

describe("workspaceReducer", () => {
  it("opens windows with tmux-style ordinal names and activates the newest", () => {
    const state = build(open("a"), open("b"));
    expect(state.windows.map((w) => w.name)).toEqual(["1", "2"]);
    expect(state.activeWindow).toBe("w-b");
    expect(focusedSession(state)?.id).toBe("b");
  });

  it("reuses a freed ordinal for the next window", () => {
    const state = build(open("a"), open("b"), (s) => workspaceReducer(s, { type: "close-window", windowId: "w-a" }), open("c"));
    expect(state.windows.map((w) => w.name)).toEqual(["2", "1"]);
  });

  it("splits the focused pane of the active window and focuses the new pane", () => {
    const state = build(open("a"), split("b"), split("c", "column"));
    const win = term(state.windows[0]);
    expect(leafIds(win.root)).toEqual(["a", "b", "c"]);
    expect(win.focused).toBe("c");
    expect(Object.keys(state.sessions).sort()).toEqual(["a", "b", "c"]);
  });

  it("splitting with no window open makes one", () => {
    const state = build(split("a"));
    expect(state.windows).toHaveLength(1);
    expect(state.windows[0].kind === "terminal" && state.windows[0].root).toEqual({ kind: "leaf", id: "a" });
  });

  it("closing a pane refocuses a neighbour and closing the last closes the window", () => {
    let state = build(open("a"), split("b"), open("c"));
    state = workspaceReducer(state, { type: "activate-window", windowId: "w-a" });
    state = workspaceReducer(state, { type: "close-pane", id: "b" });
    expect(term(state.windows[0]).focused).toBe("a");
    expect(state.sessions.b).toBeUndefined();

    state = workspaceReducer(state, { type: "close-pane", id: "a" });
    expect(state.windows.map((w) => w.id)).toEqual(["w-c"]);
    expect(state.activeWindow).toBe("w-c");
  });

  it("closing the active window lands on the neighbouring tab", () => {
    let state = build(open("a"), open("b"), open("c"));
    state = workspaceReducer(state, { type: "activate-window", windowId: "w-b" });
    state = workspaceReducer(state, { type: "close-window", windowId: "w-b" });
    expect(state.activeWindow).toBe("w-c");
    state = workspaceReducer(state, { type: "close-window", windowId: "w-c" });
    expect(state.activeWindow).toBe("w-a");
    state = workspaceReducer(state, { type: "close-window", windowId: "w-a" });
    expect(state.activeWindow).toBeNull();
    expect(state.windows).toEqual([]);
  });

  it("detaching removes the pane without touching other sessions", () => {
    const state = workspaceReducer(build(open("a"), split("b")), { type: "detach", id: "a" });
    expect(leafIds(term(state.windows[0]).root)).toEqual(["b"]);
    expect(state.sessions.a).toBeUndefined();
    expect(state.sessions.b).toBeDefined();
  });

  it("records exits and working directories per session", () => {
    let state = build(open("a"));
    state = workspaceReducer(state, { type: "cwd", id: "a", cwd: "/tmp" });
    expect(state.sessions.a.cwd).toBe("/tmp");
    const same = workspaceReducer(state, { type: "cwd", id: "a", cwd: "/tmp" });
    expect(same).toBe(state);
    state = workspaceReducer(state, { type: "exited", id: "a" });
    expect(state.sessions.a.exited).toBe(true);
    expect(workspaceReducer(state, { type: "exited", id: "nope" })).toBe(state);
  });

  it("resizes only the named split of the named window", () => {
    let state = build(open("a"), split("b"));
    state = workspaceReducer(state, { type: "resize", windowId: "w-a", splitId: "split-b", sizes: [0.3, 0.7] });
    const root = term(state.windows[0]).root;
    expect(root.kind === "split" && root.sizes).toEqual([0.3, 0.7]);
  });

  it("numbers new sessions per host", () => {
    const state = build(open("a"), split("b"), (s) =>
      workspaceReducer(s, { type: "split", session: session("c", { ssh: "h1" }), direction: "row", splitId: "x" }),
    );
    expect(sessionOrdinal(state, "local")).toBe(3);
    expect(sessionOrdinal(state, { ssh: "h1" })).toBe(2);
    expect(sessionOrdinal(state, { ssh: "h2" })).toBe(1);
  });
});

describe("editor tabs", () => {
  const file = { path: "/etc/hosts", sftp: null, hostName: null };
  const other = { path: "/etc/motd", sftp: null, hostName: null };

  it("opens files into one editor tab, in front, and never twice", () => {
    let state = build(open("a"));
    state = workspaceReducer(state, { type: "open-editor", windowId: "e1", file });
    expect(state.windows.map((w) => w.kind)).toEqual(["terminal", "editor"]);
    expect(state.activeWindow).toBe("e1");
    expect(state.windows[1].name).toBe("2");
    state = workspaceReducer(state, { type: "activate-window", windowId: "w-a" });
    state = workspaceReducer(state, { type: "open-editor", windowId: "e2", file: other });
    expect(state.windows).toHaveLength(2);
    expect(state.activeWindow).toBe("e1");
    const editor = state.windows[1];
    if (editor.kind !== "editor") throw new Error("expected editor");
    expect(editor.files.map((f) => f.path)).toEqual(["/etc/hosts", "/etc/motd"]);
    expect(editor.active).toBe(editorKey(other));
    state = workspaceReducer(state, { type: "open-editor", windowId: "e3", file });
    const again = state.windows[1];
    expect(again.kind === "editor" && again.files).toHaveLength(2);
    expect(again.kind === "editor" && again.active).toBe(editorKey(file));
    // The same path on a host is a different file.
    state = workspaceReducer(state, { type: "open-editor", windowId: "e4", file: { ...file, sftp: "s1", hostName: "box" } });
    expect(state.windows[1].kind === "editor" && state.windows[1].files).toHaveLength(3);
  });

  it("keeps following the last terminal window while the editor is in front", () => {
    let state = build(open("a"), open("b"));
    state = workspaceReducer(state, { type: "open-editor", windowId: "e1", file });
    expect(focusedSession(state)?.id).toBe("b");
    // Splitting from the editor tab goes to that terminal window and shows it.
    state = split("c")(state);
    expect(state.activeWindow).toBe("w-b");
    expect(leafIds(term(state.windows[1]).root)).toEqual(["b", "c"]);
  });

  it("closes files one by one, tracks dirtiness, and drops the tab with the last file", () => {
    let state = build(open("a"));
    state = workspaceReducer(state, { type: "open-editor", windowId: "e1", file });
    state = workspaceReducer(state, { type: "open-editor", windowId: "e1", file: other });
    state = workspaceReducer(state, { type: "editor-dirty", key: editorKey(file), dirty: true });
    let editor = state.windows[1];
    expect(editor.kind === "editor" && editor.dirty[editorKey(file)]).toBe(true);
    state = workspaceReducer(state, { type: "close-file", key: editorKey(other) });
    editor = state.windows[1];
    expect(editor.kind === "editor" && editor.files.map((f) => f.path)).toEqual(["/etc/hosts"]);
    expect(editor.kind === "editor" && editor.active).toBe(editorKey(file));
    state = workspaceReducer(state, { type: "close-file", key: editorKey(file) });
    expect(state.windows.map((w) => w.id)).toEqual(["w-a"]);
    expect(state.activeWindow).toBe("w-a");
    expect(state.sessions.a).toBeDefined();
  });
});
