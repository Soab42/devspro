import { DragEvent, MouseEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button, Callout, EmptyState, IconButton, MenuItem, Spinner, Stack } from "../../../ui";
import {
  ArrowUp,
  ChevronDown,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  ClipboardPaste,
  Copy,
  Eye,
  EyeOff,
  File,
  FilePlus,
  FileText,
  Folder,
  FolderInput,
  FolderOpen,
  FolderPlus,
  HardDrive,
  House,
  LocateFixed,
  Pencil,
  RefreshCw,
  Scissors,
  Server,
  Trash2,
  Unplug,
  X,
} from "../../../ui/icons";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useConfirm } from "../../../app/ConfirmContext";
import ContextMenu, { ContextMenuState } from "../../file-manager/components/ContextMenu";
import NameDialog from "../../file-manager/components/NameDialog";
import { Icon } from "../../../ui/Icon";
import { TextInput } from "../../../ui";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { fmCopy, fmCreateFile, fmList, fmMkdir, fmMove, fmPlaces, fmRename, fmTrash } from "../../file-manager/api";
import { ptyCwd, sftpDelete, sftpHome, sftpList, sftpMkdir, sftpRename, sftpUpload, sftpWriteFile, type SftpEntry, type SshHost } from "../../ssh/api";
import {
  canMoveInto,
  currentFileDrag,
  dirAtPoint,
  dropDirOf,
  endFileDrag,
  hasFileDrag,
  readFileDrag,
  sameOrigin,
  startFileDrag,
  useOsFileDrop,
  type FileDragPayload,
} from "../../ssh/dnd";
import { baseName, cdCommand, errorMessage, followHookCommand, joinPath, parentPath, shellQuote, sortEntries } from "../../ssh/utils";
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

/** What Cut or Copy put aside, waiting for a Paste. One per source, so a
    remote cut cannot be pasted into a local folder. */
interface Clipboard {
  mode: "cut" | "copy";
  sourceKey: string;
  items: Pick<Row, "path" | "name" | "is_dir">[];
}

type Dialog =
  | { kind: "mkdir"; dir: string }
  | { kind: "newfile"; dir: string }
  | { kind: "rename"; row: Row }
  | { kind: "moveto"; row: Row };

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
 * Rows drag: onto a terminal pane to type their path, onto a folder row
 * here to move them. Files from the desktop dropped on a remote browser
 * upload into the folder they land on.
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
  const confirmAction = useConfirm();
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [clipboard, setClipboard] = useState<Clipboard | null>(null);
  // The row the last right-click landed on, kept lit while its menu is up.
  const [selected, setSelected] = useState<string | null>(null);

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

  /* ---- drag and drop ---- */
  const origin = useMemo<Pick<FileDragPayload, "source" | "session">>(
    () => (isLocal ? { source: "local" } : { source: "remote", session: sftpId ?? undefined }),
    [isLocal, sftpId],
  );
  const listRef = useRef<HTMLUListElement>(null);
  const dragDepth = useRef(0);
  const [dropDir, setDropDir] = useState<string | null>(null);
  const [uploading, setUploading] = useState<string | null>(null);

  /** Reload the current folder and every folder unfolded beneath it, keeping the tree shape. */
  const refreshTree = useCallback(async () => {
    if (path === null) return;
    await load(path);
    const open = Object.keys(expanded).filter((k) => Array.isArray(expanded[k]));
    const loaded = await Promise.all(open.map(async (d) => [d, await list(d).catch(() => null)] as const));
    setExpanded((prev) => {
      const next = { ...prev };
      for (const [d, rows] of loaded) if (rows && Array.isArray(next[d])) next[d] = rows;
      return next;
    });
  }, [path, load, list, expanded]);

  /** Where a drag over `target` would land, or null when a drop would do nothing. */
  function dropTargetOf(target: EventTarget | null): string | null {
    if (!canBrowse) return null;
    const dir = dropDirOf(target, path);
    if (dir === null) return null;
    const drag = currentFileDrag();
    if (drag && sameOrigin(drag, origin) && !canMoveInto(drag, dir)) return null;
    return dir;
  }

  function onDragEnter(e: DragEvent<HTMLUListElement>) {
    if (!hasFileDrag(e.dataTransfer)) return;
    dragDepth.current += 1;
    onDragOver(e);
  }

  function onDragOver(e: DragEvent<HTMLUListElement>) {
    if (!hasFileDrag(e.dataTransfer)) return;
    const dir = dropTargetOf(e.target);
    if (dir === null) {
      if (dropDir !== null) setDropDir(null);
      return;
    }
    e.preventDefault();
    const drag = currentFileDrag();
    e.dataTransfer.dropEffect = drag && sameOrigin(drag, origin) ? "move" : "copy";
    if (dir !== dropDir) setDropDir(dir);
  }

  function onDragLeave(e: DragEvent<HTMLUListElement>) {
    if (!hasFileDrag(e.dataTransfer)) return;
    dragDepth.current = Math.max(0, dragDepth.current - 1);
    if (dragDepth.current === 0) setDropDir(null);
  }

  function onDrop(e: DragEvent<HTMLUListElement>) {
    if (!hasFileDrag(e.dataTransfer)) return;
    e.preventDefault();
    dragDepth.current = 0;
    setDropDir(null);
    const payload = readFileDrag(e.dataTransfer);
    const dir = dropTargetOf(e.target);
    endFileDrag();
    if (!payload || dir === null) return;
    if (sameOrigin(payload, origin)) void moveEntry(payload, dir);
    else if (!isLocal && payload.source === "local") void uploadFiles([payload.path], dir);
  }

  async function moveEntry(payload: FileDragPayload, dir: string) {
    try {
      if (isLocal) await fmMove(payload.path, dir);
      else if (sftpId) await sftpRename(sftpId, payload.path, joinPath(dir, payload.name));
      await refreshTree();
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  /** Local files, one after another, into a remote folder. */
  async function uploadFiles(paths: string[], dir: string) {
    if (!sftpId || uploading !== null) return;
    setError(null);
    try {
      for (const p of paths) {
        const name = baseName(p);
        setUploading(name);
        await sftpUpload(sftpId, p, joinPath(dir, name));
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setUploading(null);
    }
    await refreshTree();
  }

  useOsFileDrop({
    onHover: (point) => {
      if (isLocal || !sftpId) return;
      setDropDir(point ? dirAtPoint(listRef.current, point, path) : null);
    },
    onDrop: (paths, point) => {
      if (isLocal || !sftpId) return;
      const dir = dirAtPoint(listRef.current, point, path);
      if (dir !== null) void uploadFiles(paths, dir);
    },
  });

  /* ---- right-click verbs ---- */

  /** Runs a mutation, surfaces its error, and reloads the tree afterwards. */
  async function mutate(action: () => Promise<unknown>) {
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(errorMessage(err));
    }
    await refreshTree();
  }

  async function removeRow(row: Row) {
    const remoteNote = isLocal ? "It goes to the desktop's trash." : row.is_dir ? "Over SFTP only an empty folder can be removed, and there is no undo." : "There is no undo over SFTP.";
    const ok = await confirmAction(`"${row.name}" will be ${isLocal ? "moved to the trash" : "deleted"}. ${remoteNote}`, {
      title: isLocal ? "Move to trash?" : "Delete?",
      confirmLabel: isLocal ? "Move to trash" : "Delete",
      danger: true,
    });
    if (!ok) return;
    await mutate(() => (isLocal ? fmTrash(row.path) : sftpDelete(sftpId as string, row.path)));
  }

  async function pasteInto(dir: string) {
    if (!clipboard || clipboard.sourceKey !== sourceKey) return;
    const { mode, items } = clipboard;
    if (mode === "cut") setClipboard(null);
    await mutate(async () => {
      const failed: string[] = [];
      for (const item of items) {
        try {
          if (mode === "cut") {
            if (isLocal) await fmMove(item.path, dir);
            else await sftpRename(sftpId as string, item.path, joinPath(dir, item.name));
          } else {
            await fmCopy(item.path, dir);
          }
        } catch (err) {
          failed.push(`${item.name}: ${errorMessage(err)}`);
        }
      }
      if (failed.length) throw new Error(failed.join("\n"));
    });
  }

  async function handleDialogSubmit(value: string) {
    if (!dialog) return;
    const current = dialog;
    setDialog(null);
    await mutate(async () => {
      switch (current.kind) {
        case "mkdir":
          return isLocal ? fmMkdir(current.dir, value) : sftpMkdir(sftpId as string, joinPath(current.dir, value.trim()));
        case "newfile":
          return isLocal ? fmCreateFile(current.dir, value) : sftpWriteFile(sftpId as string, joinPath(current.dir, value.trim()), "");
        case "rename":
          return isLocal ? fmRename(current.row.path, value) : sftpRename(sftpId as string, current.row.path, joinPath(parentPath(current.row.path), value.trim()));
        case "moveto": {
          const target = value.trim().startsWith("/") ? value.trim() : joinPath(path ?? "/", value.trim());
          return isLocal ? fmMove(current.row.path, target) : sftpRename(sftpId as string, current.row.path, joinPath(target, current.row.name));
        }
      }
    });
  }

  /** Move a row to a folder picked in the native dialog (local) or typed (remote). */
  async function moveRowTo(row: Row) {
    if (!isLocal) {
      setDialog({ kind: "moveto", row });
      return;
    }
    const chosen = await openDialog({ directory: true, multiple: false, title: `Move ${row.name} to…`, defaultPath: path ?? home });
    if (chosen && !Array.isArray(chosen)) await mutate(() => fmMove(row.path, chosen));
  }

  const canPaste = clipboard !== null && clipboard.sourceKey === sourceKey && clipboard.items.length > 0;

  /** The verbs that belong to the folder itself: for empty space, and the tail of a folder row's menu. */
  function folderItems(dir: string, separator: boolean): MenuItem[] {
    return [
      { key: "mkdir", label: "New folder", icon: FolderPlus, separatorBefore: separator, onClick: () => setDialog({ kind: "mkdir", dir }) },
      { key: "newfile", label: "New file", icon: FilePlus, onClick: () => setDialog({ kind: "newfile", dir }) },
      {
        key: "paste",
        label: clipboard && canPaste ? `Paste ${clipboard.items.length === 1 ? clipboard.items[0].name : `${clipboard.items.length} items`}` : "Paste",
        icon: ClipboardPaste,
        disabled: !canPaste,
        onClick: () => void pasteInto(dir),
      },
    ];
  }

  function rowItems(row: Row): MenuItem[] {
    const items: MenuItem[] = [];
    if (row.is_dir) {
      items.push({ key: "open", label: "Open", icon: FolderOpen, onClick: () => navigate(row.path) });
      if (session && !session.exited) {
        items.push({ key: "cd", label: "cd here in the terminal", icon: FolderOpen, onClick: () => onSend(session.id, cdCommand(row.path)) });
      }
    } else {
      items.push({ key: "edit", label: "Open in editor tab", icon: Pencil, onClick: () => onOpenFile(row.path) });
    }
    if (session && !session.exited) {
      items.push({ key: "type", label: "Type path into the terminal", icon: FileText, onClick: () => onSend(session.id, `${shellQuote(row.path)} `) });
    }
    items.push({ key: "copypath", label: "Copy path", icon: Copy, onClick: () => void navigator.clipboard.writeText(row.path) });
    if (isLocal) {
      items.push({ key: "reveal", label: "Reveal in system file manager", icon: HardDrive, onClick: () => void revealItemInDir(row.path).catch((e) => setError(errorMessage(e))) });
    }
    items.push({ key: "cut", label: "Cut", icon: Scissors, separatorBefore: true, onClick: () => setClipboard({ mode: "cut", sourceKey, items: [row] }) });
    if (isLocal) items.push({ key: "copy", label: "Copy", icon: Copy, onClick: () => setClipboard({ mode: "copy", sourceKey, items: [row] }) });
    if (row.is_dir) items.push(...folderItems(row.path, false).filter((i) => i.key === "paste"));
    items.push({ key: "moveto", label: "Move to…", icon: FolderInput, onClick: () => void moveRowTo(row) });
    items.push({ key: "rename", label: "Rename", icon: Pencil, onClick: () => setDialog({ kind: "rename", row }) });
    if (row.is_dir) items.push(...folderItems(row.path, true).filter((i) => i.key !== "paste"));
    items.push({
      key: "delete",
      label: isLocal ? "Move to trash" : "Delete",
      icon: Trash2,
      danger: true,
      separatorBefore: true,
      onClick: () => void removeRow(row),
    });
    return items;
  }

  function openRowMenu(e: MouseEvent, row: Row) {
    e.preventDefault();
    e.stopPropagation();
    setSelected(row.path);
    setMenu({ x: e.clientX, y: e.clientY, items: rowItems(row) });
  }

  function openFolderMenu(e: MouseEvent) {
    if (path === null) return;
    e.preventDefault();
    setSelected(null);
    setMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        ...folderItems(path, false),
        { key: "refresh", label: "Refresh", icon: RefreshCw, separatorBefore: true, onClick: () => void refreshTree() },
        ...(isLocal ? [{ key: "reveal", label: "Reveal in system file manager", icon: HardDrive, onClick: () => void revealItemInDir(path).catch((e) => setError(errorMessage(e))) }] : []),
      ],
    });
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

          {uploading !== null && (
            <div className="term-browser-hint term-browser-hint-boxed term-browser-uploading">
              <Spinner size="sm" /> Uploading {uploading}…
            </div>
          )}

          <ul
            ref={listRef}
            className={`term-browser-list${dropDir !== null && dropDir === path ? " term-browser-drop-active" : ""}`}
            aria-label={`Files in ${path ?? ""}`}
            onDragEnter={onDragEnter}
            onDragOver={onDragOver}
            onDragLeave={onDragLeave}
            onDrop={onDrop}
            onContextMenu={openFolderMenu}
          >
            {loading && entries.length === 0 && (
              <li className="term-browser-loading">
                <Spinner size="sm" />
              </li>
            )}
            {!loading && visible.length === 0 && !error && <li className="term-browser-loading">Empty folder</li>}
            <TreeRows
              rows={visible}
              depth={0}
              expanded={expanded}
              showHidden={showHidden}
              origin={origin}
              dropDir={dropDir}
              onToggle={(p) => void toggleFolder(p)}
              onNavigate={navigate}
              onOpenFile={onOpenFile}
              onContextMenu={openRowMenu}
              selected={selected}
              cutPaths={clipboard?.mode === "cut" && clipboard.sourceKey === sourceKey ? clipboard.items.map((i) => i.path) : []}
            />
          </ul>
        </>
      )}

      {menu && (
        <ContextMenu
          menu={menu}
          onClose={() => {
            setMenu(null);
            setSelected(null);
          }}
        />
      )}
      {dialog && (
        <NameDialog
          title={
            dialog.kind === "mkdir"
              ? "New folder"
              : dialog.kind === "newfile"
                ? "New file"
                : dialog.kind === "rename"
                  ? `Rename ${dialog.row.name}`
                  : `Move ${dialog.row.name} to folder`
          }
          initial={dialog.kind === "rename" ? dialog.row.name : dialog.kind === "moveto" ? (path ?? "/") : ""}
          confirmLabel={dialog.kind === "rename" ? "Rename" : dialog.kind === "moveto" ? "Move" : "Create"}
          onSubmit={(v) => void handleDialogSubmit(v)}
          onClose={() => setDialog(null)}
        />
      )}
    </div>
  );
}

/**
 * One level of the tree. A folder row has a chevron that unfolds its
 * children in place (loaded on first unfold) and a name that navigates
 * into it; a file row opens the editor. Every row can be dragged away, and
 * a folder row is a drop target.
 */
function TreeRows({
  rows,
  depth,
  expanded,
  showHidden,
  origin,
  dropDir,
  onToggle,
  onNavigate,
  onOpenFile,
  onContextMenu,
  selected,
  cutPaths,
}: {
  rows: Row[];
  depth: number;
  expanded: Record<string, Row[] | "loading">;
  showHidden: boolean;
  origin: Pick<FileDragPayload, "source" | "session">;
  /** The folder a drag hovering over the tree would land in. */
  dropDir: string | null;
  onToggle: (path: string) => void;
  onNavigate: (path: string) => void;
  onOpenFile: (path: string) => void;
  onContextMenu: (e: MouseEvent, row: Row) => void;
  /** The row whose menu is open. */
  selected: string | null;
  /** Rows waiting to be pasted after a Cut, drawn faded. */
  cutPaths: string[];
}) {
  return (
    <>
      {rows.map((e) => {
        const children = expanded[e.path];
        const unfolded = children !== undefined;
        return (
          <li key={e.path}>
            <div
              className={[
                "term-browser-row",
                e.is_dir && "term-browser-row-dir",
                e.is_dir && dropDir === e.path && "term-browser-row-drop",
                selected === e.path && "term-browser-row-selected",
                cutPaths.includes(e.path) && "term-browser-row-cut",
                `term-browser-depth-${Math.min(depth, 8)}`,
              ]
                .filter(Boolean)
                .join(" ")}
              title={e.path}
              draggable
              onContextMenu={(ev) => onContextMenu(ev, e)}
              data-drop-dir={e.is_dir ? e.path : undefined}
              onDragStart={(ev) => startFileDrag(ev.dataTransfer, { ...origin, path: e.path, name: e.name, is_dir: e.is_dir })}
              onDragEnd={endFileDrag}
            >
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
                    origin={origin}
                    dropDir={dropDir}
                    onToggle={onToggle}
                    onNavigate={onNavigate}
                    onOpenFile={onOpenFile}
                    onContextMenu={onContextMenu}
                    selected={selected}
                    cutPaths={cutPaths}
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
