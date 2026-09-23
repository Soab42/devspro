import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button, Callout, EmptyState, IconButton, Spinner, Stack } from "../../../ui";
import {
  ArrowUp,
  ChevronDown,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  Eye,
  EyeOff,
  File,
  Folder,
  FolderInput,
  FolderOpen,
  House,
  LocateFixed,
  RefreshCw,
  Server,
  Unplug,
  X,
} from "../../../ui/icons";
import { Icon } from "../../../ui/Icon";
import { TextInput } from "../../../ui";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { fmList, fmPlaces } from "../../file-manager/api";
import { ptyCwd, sftpHome, sftpList, type SftpEntry, type SshHost } from "../../ssh/api";
import { cdCommand, errorMessage, followHookCommand, parentPath, sortEntries } from "../../ssh/utils";
import type { TerminalSession } from "../workspace";

/** What the sidebar browses for one host: an open SFTP session, or the state of getting one. */
export interface RemoteBrowser {
  sessionId: string | null;
  connecting: boolean;
  /** Why the last attempt failed or was cancelled; null while it hasn't been tried or succeeded. */
  failure: string | null;
  connect: () => void;
  disconnect: () => void;
}

interface Row {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified: number;
  permissions: string;
  hidden: boolean;
}

const LOCAL_POLL_MS = 1500;
const HIDDEN_KEY = "devspro.terminal.showHidden";

/**
 * The follow-along file browser beside the terminal, MobaXterm-style: it
 * shows the folder the focused shell is in and moves when the shell does.
 *
 * Two ways of knowing where the shell is. A local shell is a child process
 * of ours, so its cwd is read straight from /proc while following is on.
 * A remote shell is on the other end of `ssh`; there the only signal is
 * the shell itself printing OSC 7 at each prompt, which the terminal
 * decodes and records on the session. Shells rarely do that out of the
 * box, so there is a one-click hook to type the necessary line into it.
 *
 * Navigation is two-way: the browser follows the shell, and "cd here"
 * sends the shell to the folder the browser is looking at.
 *
 * Nothing to set up: a remote pane connects its SFTP session as soon as it
 * is focused, and following is on by default — browsing away by hand
 * pauses it until the follow button is pressed again.
 */
export default function TerminalSidebar({
  session,
  host,
  remote,
  onCwd,
  onSend,
  onOpenFile,
  onClose,
}: {
  /** The focused pane's session, or null when nothing is focused. */
  session: TerminalSession | null;
  /** The saved host behind an ssh session. */
  host: SshHost | null;
  /** The remote browser for that host. */
  remote: RemoteBrowser | null;
  /** A local shell's cwd was observed. */
  onCwd: (sessionId: string, cwd: string) => void;
  /** Type into the focused terminal. */
  onSend: (sessionId: string, text: string) => void;
  /** A file was clicked: open it in an editor tab. */
  onOpenFile: (path: string) => void;
  onClose: () => void;
}) {
  const isLocal = session?.command === "local";
  const sftpId = !isLocal && remote ? remote.sessionId : null;
  // One browsing context per source: switching between a local pane and a
  // remote one, or between hosts, resets the path; switching between two
  // panes of the same host keeps it.
  const sourceKey = session ? (isLocal ? "local" : `sftp:${sftpId ?? "none"}`) : "none";
  const canBrowse = session !== null && (isLocal || sftpId !== null);

  // Connect the remote browser the moment a remote pane is focused. One
  // attempt per host; a failure or cancelled prompt shows a retry instead
  // of asking again every time focus moves.
  useEffect(() => {
    if (isLocal || !remote || remote.sessionId || remote.connecting || remote.failure !== null) return;
    remote.connect();
  }, [isLocal, remote]);

  const [follow, setFollow] = useState(true);
  const [showHidden, setShowHiddenState] = useState(() => {
    try {
      return localStorage.getItem(HIDDEN_KEY) === "1";
    } catch {
      return false;
    }
  });
  function setShowHidden(next: boolean) {
    setShowHiddenState(next);
    try {
      localStorage.setItem(HIDDEN_KEY, next ? "1" : "0");
    } catch {
      /* private mode */
    }
  }
  const [path, setPath] = useState<string | null>(null);
  const [entries, setEntries] = useState<Row[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [home, setHome] = useState<string>("/");
  // Folders unfolded inline beneath the current one, with their children.
  const [expanded, setExpanded] = useState<Record<string, Row[] | "loading">>({});
  const [editingPath, setEditingPath] = useState<string | null>(null);

  const list = useCallback(
    async (target: string): Promise<Row[]> => {
      if (isLocal) {
        return (await fmList(target)).map((e) => ({ ...e, hidden: e.is_hidden }));
      }
      if (!sftpId) return [];
      return (await sftpList(sftpId, target)).map((e: SftpEntry) => ({ ...e, hidden: e.name.startsWith(".") }));
    },
    [isLocal, sftpId],
  );

  const load = useCallback(
    async (target: string) => {
      setLoading(true);
      setError(null);
      try {
        setEntries(await list(target));
      } catch (e) {
        setError(errorMessage(e));
      } finally {
        setLoading(false);
      }
    },
    [list],
  );

  // A new source: find its home, then start at the shell's last known cwd
  // when we have one, else at home.
  const sessionCwd = session?.cwd ?? null;
  useEffect(() => {
    if (!canBrowse) {
      setPath(null);
      setEntries([]);
      return;
    }
    let cancelled = false;
    const start = isLocal ? fmPlaces().then((p) => p.home) : sftpHome(sftpId as string);
    start
      .catch(() => "/")
      .then((h) => {
        if (cancelled) return;
        setHome(h || "/");
        setPath(sessionCwd ?? h ?? "/");
      });
    return () => {
      cancelled = true;
    };
    // sessionCwd is read once, for the starting point; later changes are
    // handled by the follow effect below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sourceKey, canBrowse, isLocal, sftpId]);

  useEffect(() => {
    setExpanded({});
    if (path !== null && canBrowse) void load(path);
  }, [path, canBrowse, load]);

  async function toggleFolder(target: string) {
    if (expanded[target] !== undefined) {
      setExpanded((prev) => {
        const next = { ...prev };
        // Fold the folder and everything unfolded beneath it.
        for (const key of Object.keys(next)) if (key === target || key.startsWith(`${target}/`)) delete next[key];
        return next;
      });
      return;
    }
    setExpanded((prev) => ({ ...prev, [target]: "loading" }));
    try {
      const rows = await list(target);
      setExpanded((prev) => (prev[target] === undefined ? prev : { ...prev, [target]: rows }));
    } catch (e) {
      setError(errorMessage(e));
      setExpanded((prev) => {
        const next = { ...prev };
        delete next[target];
        return next;
      });
    }
  }

  /** Unfold every folder in the current directory, one level deep. */
  async function unfoldAll() {
    const dirs = entries.filter((e) => e.is_dir && (showHidden || !e.hidden));
    setExpanded((prev) => {
      const next = { ...prev };
      for (const d of dirs) if (next[d.path] === undefined) next[d.path] = "loading";
      return next;
    });
    const loaded = await Promise.all(
      dirs.map(async (d) => [d.path, await list(d.path).catch(() => [] as Row[])] as const),
    );
    setExpanded((prev) => {
      const next = { ...prev };
      for (const [dir, rows] of loaded) if (next[dir] === "loading") next[dir] = rows;
      return next;
    });
  }

  /** Browse a folder chosen by the user: a native picker locally, a typed path remotely. */
  async function chooseFolder() {
    if (isLocal) {
      const selected = await openDialog({ directory: true, multiple: false, title: "Open folder", defaultPath: path ?? home });
      if (selected && !Array.isArray(selected)) navigate(selected);
      return;
    }
    setEditingPath(path ?? "/");
  }

  // Follow a remote shell: the session's cwd is updated from OSC 7 reports.
  useEffect(() => {
    if (follow && sessionCwd && canBrowse) setPath(sessionCwd);
  }, [follow, sessionCwd, canBrowse]);

  // Follow a local shell: ask /proc where it is, on a timer.
  const pathRef = useRef(path);
  pathRef.current = path;
  useEffect(() => {
    if (!follow || !isLocal || !session || session.exited) return;
    const id = session.id;
    let stopped = false;
    const tick = async () => {
      try {
        const cwd = await ptyCwd(id);
        if (!stopped && cwd && cwd !== pathRef.current) {
          onCwd(id, cwd);
          setPath(cwd);
        }
      } catch {
        // The shell has gone; the exit event will retire the session.
      }
    };
    void tick();
    const timer = window.setInterval(() => void tick(), LOCAL_POLL_MS);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [follow, isLocal, session, onCwd]);

  const visible = useMemo(() => {
    const rows = showHidden ? entries : entries.filter((e) => !e.hidden);
    return sortEntries(rows, "name", "asc") as Row[];
  }, [entries, showHidden]);

  function navigate(target: string) {
    // Browsing away by hand is a request to stop following; the button
    // brings it back and jumps to the shell's folder.
    setFollow(false);
    setPath(target);
  }

  function resumeFollow() {
    setFollow(true);
    if (sessionCwd) setPath(sessionCwd);
  }

  const title = session ? (isLocal ? "Local files" : (host?.name ?? "Remote files")) : "Files";

  return (
    <div className="term-browser">
      <div className="term-browser-head">
        <Icon icon={isLocal || !session ? House : Server} size="sm" />
        <span className="term-browser-title" title={title}>
          {title}
        </span>
        <IconButton icon={X} label="Hide file browser" size="sm" onClick={onClose} />
      </div>

      {!session && (
        <EmptyState size="sm" icon={Folder} title="No terminal focused" description="Click a terminal pane to browse its folder." />
      )}

      {session && !isLocal && !sftpId && (
        <Stack gap={3} className="term-browser-connect">
          {remote?.failure ? (
            <>
              <Callout tone="danger">{remote.failure}</Callout>
              <Button variant="secondary" size="sm" icon={RefreshCw} onClick={() => remote.connect()}>
                Retry
              </Button>
            </>
          ) : (
            <span className="term-browser-hint term-browser-connecting">
              <Spinner size="sm" /> Connecting to {host?.name ?? "the host"}…
            </span>
          )}
        </Stack>
      )}

      {session && canBrowse && (
        <>
          <div className="term-browser-tools">
            <IconButton icon={ArrowUp} label="Parent folder" size="sm" disabled={!path || path === "/"} onClick={() => path && navigate(parentPath(path))} />
            <IconButton icon={House} label="Home folder" size="sm" onClick={() => navigate(home)} />
            <IconButton icon={FolderInput} label="Open folder…" size="sm" onClick={() => void chooseFolder()} />
            {Object.keys(expanded).length > 0 ? (
              <IconButton icon={ChevronsDownUp} label="Fold all folders" size="sm" onClick={() => setExpanded({})} />
            ) : (
              <IconButton
                icon={ChevronsUpDown}
                label="Unfold all folders"
                size="sm"
                disabled={!entries.some((e) => e.is_dir)}
                onClick={() => void unfoldAll()}
              />
            )}
            <IconButton
              icon={LocateFixed}
              label={follow ? "Following the terminal (click to stop)" : "Follow the terminal"}
              size="sm"
              className={follow ? "term-browser-follow-on" : undefined}
              aria-pressed={follow}
              onClick={() => (follow ? setFollow(false) : resumeFollow())}
            />
            <IconButton
              icon={FolderOpen}
              label="Change the terminal's directory to this folder"
              size="sm"
              disabled={!path || session.exited}
              onClick={() => path && onSend(session.id, cdCommand(path))}
            />
            <IconButton
              icon={showHidden ? EyeOff : Eye}
              label={showHidden ? "Hidden files shown (click to hide dotfiles)" : "Show hidden files (dotfiles)"}
              size="sm"
              aria-pressed={showHidden}
              className={showHidden ? "term-browser-follow-on" : undefined}
              onClick={() => setShowHidden(!showHidden)}
            />
            <IconButton icon={RefreshCw} label="Refresh" size="sm" loading={loading} onClick={() => path && void load(path)} />
            {!isLocal && remote && <IconButton icon={Unplug} label="Disconnect file browser" size="sm" onClick={remote.disconnect} />}
          </div>

          {editingPath !== null ? (
            <form
              className="term-browser-path-edit"
              onSubmit={(e) => {
                e.preventDefault();
                const target = editingPath.trim();
                setEditingPath(null);
                if (target) navigate(target.startsWith("/") ? target : `${path ?? ""}/${target}`);
              }}
            >
              <TextInput
                aria-label="Folder path"
                inputSize="sm"
                mono
                value={editingPath}
                autoFocus
                onChange={(e) => setEditingPath(e.target.value)}
                onBlur={() => setEditingPath(null)}
                onKeyDown={(e) => {
                  if (e.key === "Escape") setEditingPath(null);
                }}
              />
            </form>
          ) : (
            <button type="button" className="term-browser-path" title={`${path ?? ""} (click to type a path)`} onClick={() => setEditingPath(path ?? "/")}>
              {path ?? "…"}
            </button>
          )}

          {!isLocal && follow && !sessionCwd && !session.exited && (
            <div className="term-browser-hint term-browser-hint-boxed">
              Waiting for the shell to report its folder. If it never does,{" "}
              <Button variant="link" size="sm" onClick={() => onSend(session.id, followHookCommand())}>
                send the prompt hook again
              </Button>
              .
            </div>
          )}

          {error && (
            <Callout tone="danger" onDismiss={() => setError(null)}>
              {error}
            </Callout>
          )}

          <ul className="term-browser-list" aria-label={`Files in ${path ?? ""}`}>
            {loading && entries.length === 0 && (
              <li className="term-browser-loading">
                <Spinner size="sm" />
              </li>
            )}
            {!loading && visible.length === 0 && !error && <li className="term-browser-loading">Empty folder</li>}
            <TreeRows rows={visible} depth={0} expanded={expanded} showHidden={showHidden} onToggle={(p) => void toggleFolder(p)} onNavigate={navigate} onOpenFile={onOpenFile} />
          </ul>
        </>
      )}
    </div>
  );
}

/**
 * One level of the tree. A folder row has a chevron that unfolds its
 * children in place (loaded on first unfold) and a name that navigates
 * into it; a file row opens the editor.
 */
function TreeRows({
  rows,
  depth,
  expanded,
  showHidden,
  onToggle,
  onNavigate,
  onOpenFile,
}: {
  rows: Row[];
  depth: number;
  expanded: Record<string, Row[] | "loading">;
  showHidden: boolean;
  onToggle: (path: string) => void;
  onNavigate: (path: string) => void;
  onOpenFile: (path: string) => void;
}) {
  return (
    <>
      {rows.map((e) => {
        const children = expanded[e.path];
        const unfolded = children !== undefined;
        return (
          <li key={e.path}>
            <div className={`term-browser-row${e.is_dir ? " term-browser-row-dir" : ""} term-browser-depth-${Math.min(depth, 8)}`} title={e.path}>
              {e.is_dir ? (
                <button
                  type="button"
                  className="term-browser-fold"
                  aria-label={unfolded ? `Fold ${e.name}` : `Unfold ${e.name}`}
                  aria-expanded={unfolded}
                  onClick={() => onToggle(e.path)}
                >
                  {children === "loading" ? <Spinner size="sm" /> : <Icon icon={unfolded ? ChevronDown : ChevronRight} size="sm" />}
                </button>
              ) : (
                <span className="term-browser-fold" />
              )}
              <button
                type="button"
                className="term-browser-name-btn"
                onClick={() => (e.is_dir ? onNavigate(e.path) : onOpenFile(e.path))}
              >
                <Icon icon={e.is_dir ? Folder : File} size="sm" />
                <span className="term-browser-name">{e.name}</span>
              </button>
            </div>
            {Array.isArray(children) && (
              <ul className="term-browser-sublist">
                {children.filter((c) => showHidden || !c.hidden).length === 0 ? (
                  <li className={`term-browser-empty term-browser-depth-${Math.min(depth + 1, 8)}`}>Empty</li>
                ) : (
                  <TreeRows
                    rows={sortEntries(children.filter((c) => showHidden || !c.hidden), "name", "asc") as Row[]}
                    depth={depth + 1}
                    expanded={expanded}
                    showHidden={showHidden}
                    onToggle={onToggle}
                    onNavigate={onNavigate}
                    onOpenFile={onOpenFile}
                  />
                )}
              </ul>
            )}
          </li>
        );
      })}
    </>
  );
}
