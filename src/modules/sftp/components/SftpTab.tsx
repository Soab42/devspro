import {
  DragEvent,
  KeyboardEvent,
  PointerEvent,
  ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Checkbox,
  Column,
  DataTable,
  EmptyState,
  Field,
  Form,
  FormRow,
  Icon,
  IconButton,
  Menu,
  MenuItem,
  Meter,
  Modal,
  Mono,
  Section,
  SegmentedControl,
  Select,
  SortState,
  Stack,
  TextInput,
  Toolbar,
} from "../../../ui";
import {
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Bookmark,
  ChevronsDownUp,
  ChevronsLeftRight,
  ChevronsRightLeft,
  ChevronsUpDown,
  CircleCheck,
  CircleX,
  Columns2,
  Download,
  Eye,
  EyeOff,
  File,
  FileArchive,
  Folder,
  FolderPlus,
  HardDrive,
  House,
  LucideIcon,
  Pencil,
  RefreshCw,
  Scissors,
  SquareArrowOutUpRight,
  Rows2,
  Server,
  Star,
  StarOff,
  Trash2,
  Unplug,
  Upload,
  X,
} from "../../../ui/icons";
import {
  FileEntry,
  fmList,
  fmMkdir,
  fmMove,
  fmPlaces,
  fmReadFile,
  fmRename,
  fmTrash,
  fmOpenInEditor,
  fmWriteFile,
} from "../../file-manager/api";
import { getExternalEditor } from "../../settings/api";
import {
  SftpEntry,
  SshHost,
  onSftpProgress,
  sftpDelete,
  sftpDownload,
  sftpDownloadZip,
  sftpHome,
  sftpList,
  sftpMkdir,
  sftpReadFile,
  sftpRename,
  sftpUpload,
  sftpOpenInEditor,
  sftpWriteFile,
} from "../../ssh/api";
import {
  FileDragPayload,
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
} from "../../ssh/dnd";
import {
  EntrySortKey,
  SortableEntry,
  baseName,
  errorMessage,
  formatModified,
  formatSize,
  hostLabel,
  joinPath,
  parentPath,
  sortEntries,
  splitBreadcrumb,
} from "../../ssh/utils";
import { clickSelect } from "../../file-manager/utils";
import { countLabel, failureSummary, settleEach, zipName } from "../bulk";
import { SftpFavouritesStore, normalisePath, useSftpFavourites } from "../favourites";
import EditFileModal from "./EditFileModal";
import "./SftpTab.css";

export type PaneLayout = "side" | "stack";
const LAYOUT_KEY = "sftp.layout";

export interface SftpSessionInfo {
  id: string;
  host: SshHost;
}

export function readPaneLayout(): PaneLayout {
  return localStorage.getItem(LAYOUT_KEY) === "stack" ? "stack" : "side";
}

export function storePaneLayout(layout: PaneLayout) {
  localStorage.setItem(LAYOUT_KEY, layout);
}

/**
 * The session and layout controls, rendered by the page as its actions so
 * they sit in the page header — or, popped out, in the window's top bar.
 */
export function SftpControls({
  sessions,
  activeId,
  hosts,
  connecting,
  layout,
  onLayoutChange,
  onActivate,
  onConnect,
  onDisconnect,
}: {
  sessions: SftpSessionInfo[];
  activeId: string | null;
  hosts: SshHost[];
  connecting: boolean;
  layout: PaneLayout;
  onLayoutChange: (layout: PaneLayout) => void;
  onActivate: (id: string) => void;
  onConnect: (host: SshHost) => void;
  onDisconnect: (id: string) => void;
}) {
  const active = sessions.find((s) => s.id === activeId) ?? null;
  return (
    <>
      {sessions.length > 0 && (
        <Select
          aria-label="Active SFTP session"
          size="sm"
          value={active?.id ?? ""}
          onChange={onActivate}
          options={sessions.map((s) => ({
            value: s.id,
            label: `${s.host.name} (${hostLabel(s.host)})`,
          }))}
        />
      )}
      <SegmentedControl
        ariaLabel="Pane layout"
        iconOnly
        value={layout}
        onChange={onLayoutChange}
        options={[
          { value: "side", label: "Side by side", icon: Columns2 },
          { value: "stack", label: "Stacked", icon: Rows2 },
        ]}
      />
      <Select
        aria-label="Connect to host"
        size="sm"
        value=""
        placeholder={connecting ? "Connecting…" : "Connect to host…"}
        disabled={connecting || hosts.length === 0}
        onChange={(id) => {
          const host = hosts.find((h) => h.id === id);
          if (host) onConnect(host);
        }}
        options={hosts.map((h) => ({
          value: h.id,
          label: `${h.name} (${hostLabel(h)})`,
        }))}
      />
      {active && (
        <Button
          variant="secondary"
          size="sm"
          icon={Unplug}
          onClick={() => onDisconnect(active.id)}
        >
          Disconnect
        </Button>
      )}
    </>
  );
}

export default function SftpTab({
  sessions,
  activeId,
  hosts,
  layout,
}: {
  sessions: SftpSessionInfo[];
  activeId: string | null;
  hosts: SshHost[];
  layout: PaneLayout;
}) {
  const active = sessions.find((s) => s.id === activeId) ?? null;
  // Held here rather than in the browser, which remounts with every session
  // switch: one load per visit, and no switch racing a save still in flight.
  const favourites = useSftpFavourites();

  return (
    <Stack gap={5} className="sftp-tab">
      {active ? (
        <DualPaneBrowser
          key={active.id}
          session={active}
          layout={layout}
          favourites={favourites}
        />
      ) : (
        <Section padding="none">
          <EmptyState
            icon={Folder}
            size="md"
            title="Not connected"
            description={
              hosts.length === 0
                ? "Save a host in the Hosts tab first, then connect to browse its files."
                : "Pick a host above to open an SFTP session."
            }
          />
        </Section>
      )}
    </Stack>
  );
}

/* ------------------------------------------------------------------------ */

/** One pane's directory state: where it is, what's there, and how it got it. */
function useDirectory<T>(
  list: (path: string) => Promise<T[]>,
  initial: () => Promise<string>,
) {
  const [path, setPath] = useState<string | null>(null);
  const [entries, setEntries] = useState<T[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  // The folder on screen right now. A listing is slow enough to outlive a
  // click into another folder, and a transfer can finish long after the user
  // has moved on; either one, reading `path` from when it started, would put
  // that folder's files under this one's name.
  const shown = useRef<string | null>(null);

  const load = useCallback(
    async (target: string) => {
      setLoading(true);
      setError(null);
      try {
        const listed = await list(target);
        if (shown.current === target) setEntries(listed);
      } catch (e) {
        if (shown.current === target) setError(errorMessage(e));
      } finally {
        if (shown.current === target) setLoading(false);
      }
    },
    [list],
  );

  useEffect(() => {
    let cancelled = false;
    initial()
      .catch(() => "/")
      .then((start) => {
        if (!cancelled) setPath(start || "/");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    shown.current = path;
    if (path !== null) void load(path);
  }, [path, load]);

  // Whatever folder is showing when it is called, not when it was handed out:
  // a queued transfer holds on to this until it finishes.
  const refresh = useCallback(() => {
    if (shown.current !== null) void load(shown.current);
  }, [load]);

  return { path, setPath, entries, loading, error, setError, refresh };
}

/**
 * Local on the left, remote on the right. Each row carries an arrow that
 * copies it straight into the other pane's current folder — no file dialog,
 * no picking a destination twice. Rows can also be dragged: across to the
 * other pane (or onto one of its folders) to copy, onto a folder in their
 * own pane to move, and files from the desktop drop onto the remote pane
 * to upload. Remote rows can also be selected several at a time and copied,
 * moved, zipped or deleted together.
 */
function DualPaneBrowser({
  session,
  layout,
  favourites,
}: {
  session: SftpSessionInfo;
  layout: PaneLayout;
  favourites: SftpFavouritesStore;
}) {
  const confirmAction = useConfirm();
  const [folded, setFolded] = useState<{ local: boolean; remote: boolean }>({
    local: false,
    remote: false,
  });
  const toggleFold = (side: "local" | "remote") =>
    setFolded((f) => ({ ...f, [side]: !f[side] }));

  const remoteList = useCallback(
    (p: string) => sftpList(session.id, p),
    [session.id],
  );
  const remoteHome = useCallback(() => sftpHome(session.id), [session.id]);
  const remote = useDirectory<SftpEntry>(remoteList, remoteHome);

  const [showHidden, setShowHidden] = useState(false);
  const [localHome, setLocalHome] = useState<string>("/");
  const localStart = useCallback(async () => {
    const places = await fmPlaces();
    setLocalHome(places.home);
    return places.home;
  }, []);
  const local = useDirectory<FileEntry>(fmList, localStart);

  const transfers = useTransferQueue();
  const split = usePaneSplit(layout);
  const [editing, setEditing] = useState<{
    side: "local" | "remote";
    path: string;
    /** Go straight to the external editor rather than the in-app one. */
    external?: boolean;
  } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  function uploadEntry(
    entry: Pick<PaneEntry, "name" | "path">,
    dir: string | null = remote.path,
  ) {
    if (dir === null) return;
    const target = joinPath(dir, entry.name);
    transfers.enqueue(
      { name: entry.name, direction: "upload", source: entry.path },
      (id) => sftpUpload(session.id, entry.path, target, id),
      remote.refresh,
    );
  }

  function downloadEntry(
    entry: Pick<PaneEntry, "name" | "path">,
    dir: string | null = local.path,
  ) {
    if (dir === null) return;
    const target = joinPath(dir, entry.name);
    transfers.enqueue(
      { name: entry.name, direction: "download", source: entry.path },
      (id) => sftpDownload(session.id, entry.path, target, id),
      local.refresh,
    );
  }

  /**
   * A row dropped into a local folder: a remote row downloads there, a local
   * row moves there. Resolves true when the pane should refresh now — a
   * transfer refreshes on its own when it finishes.
   */
  async function dropOnLocal(payload: FileDragPayload, dir: string) {
    if (payload.source === "remote") {
      if (payload.session === session.id) downloadEntry(payload, dir);
      return false;
    }
    if (!canMoveInto(payload, dir)) return false;
    await fmMove(payload.path, dir);
    return true;
  }

  async function dropOnRemote(payload: FileDragPayload, dir: string) {
    if (payload.source === "local") {
      uploadEntry(payload, dir);
      return false;
    }
    if (payload.session !== session.id || !canMoveInto(payload, dir)) return false;
    await sftpRename(session.id, payload.path, joinPath(dir, payload.name));
    return true;
  }

  /** Files dragged in from the desktop, dropped on the remote pane. */
  function uploadFiles(paths: string[], dir: string) {
    for (const p of paths) uploadEntry({ name: baseName(p), path: p }, dir);
  }

  const localEntries = useMemo(
    () =>
      showHidden ? local.entries : local.entries.filter((e) => !e.is_hidden),
    [local.entries, showHidden],
  );

  // Favourite folders, for this host only.
  const host = session.host.name;
  const hostId = session.host.id;
  const favouritePaths = favourites.byHost[hostId] ?? [];
  const starred =
    remote.path !== null && favouritePaths.includes(normalisePath(remote.path));
  const [editingFavourites, setEditingFavourites] = useState(false);

  function toggleFavourite() {
    if (remote.path === null) return;
    const change = starred
      ? favourites.remove(hostId, remote.path)
      : favourites.add(hostId, remote.path);
    change.catch((e) =>
      remote.setError(`Couldn't save favourites: ${errorMessage(e)}`),
    );
  }

  const favouriteMenu: MenuItem[] =
    favouritePaths.length === 0
      ? [
          {
            key: "none",
            label: "No favourites yet. Star a folder to add it.",
            disabled: true,
            onClick: () => undefined,
          },
        ]
      : [
          ...favouritePaths.map((p) => ({
            key: p,
            label: p,
            icon: Star,
            onClick: () => remote.setPath(p),
          })),
          {
            key: "edit",
            label: "Edit favourites…",
            icon: Pencil,
            separatorBefore: true,
            onClick: () => setEditingFavourites(true),
          },
        ];

  /**
   * Each entry downloads, then is deleted on the server once its own copy has
   * landed, so a failed download never costs the original. Folders are copied
   * but kept: `sftp_delete` only removes empty ones, on purpose, so the user
   * is told so before anything starts.
   */
  async function moveToLocal(entries: SftpEntry[]) {
    const dir = local.path;
    if (dir === null) return false;
    const folders = entries.filter((e) => e.is_dir).length;
    if (folders > 0) {
      const ok = await confirmAction(
        `Files are deleted on ${host} as soon as each one is copied. ${
          folders === 1 ? "The folder" : `The ${folders} folders`
        } will be copied but kept on ${host}, since folders are only removed here when empty.`,
        {
          title: `Move ${countLabel(entries.length)} to ${dir}?`,
          confirmLabel: "Move",
        },
      );
      if (!ok) return false;
    }
    for (const entry of entries) {
      const target = joinPath(dir, entry.name);
      transfers.enqueue(
        { name: entry.name, direction: "download", source: entry.path },
        async (id) => {
          await sftpDownload(session.id, entry.path, target, id);
          local.refresh();
          if (entry.is_dir) return;
          try {
            await sftpDelete(session.id, entry.path);
          } catch (e) {
            throw new Error(`Copied, but not deleted on ${host}: ${errorMessage(e)}`);
          }
        },
        remote.refresh,
      );
    }
    return true;
  }

  async function downloadZip(entries: SftpEntry[]) {
    const target = await save({
      title: "Save as zip",
      defaultPath: joinPath(local.path ?? localHome, zipName(entries, remote.path, host)),
      filters: [{ name: "Zip archive", extensions: ["zip"] }],
    });
    if (!target) return false;
    transfers.enqueue(
      { name: baseName(target), direction: "download", source: target },
      async (id) => {
        const { skipped } = await sftpDownloadZip(
          session.id,
          entries.map((e) => e.path),
          target,
          id,
        );
        local.refresh();
        // The archive is saved either way; what's missing from it shows on
        // the transfer's row.
        if (skipped.length > 0)
          throw new Error(
            `Saved without ${countLabel(skipped.length)}: ${skipped.join("; ")}`,
          );
      },
      () => undefined,
    );
    return true;
  }

  async function deleteRemote(entries: SftpEntry[]) {
    const ok = await confirmAction(
      `${countLabel(entries.length)} will be permanently deleted on ${host}.${
        entries.some((e) => e.is_dir)
          ? " Folders are removed only if they are empty."
          : ""
      }`,
      {
        title: `Delete ${countLabel(entries.length)}?`,
        confirmLabel: "Delete",
        danger: true,
      },
    );
    if (!ok) return false;
    const failures = await settleEach(entries, (e) =>
      sftpDelete(session.id, e.path),
    );
    // Refresh first: a reload clears the pane's error, and the failures
    // should outlast it.
    remote.refresh();
    if (failures.length > 0)
      remote.setError(failureSummary("delete", failures, entries.length));
    const deleted = entries.length - failures.length;
    if (deleted > 0) setNotice(`Deleted ${countLabel(deleted)} on ${host}.`);
    return true;
  }

  const remoteBulk: BulkAction<SftpEntry>[] = [
    {
      key: "copy",
      label: "Copy to local",
      icon: ArrowLeft,
      disabled: local.path === null,
      run: async (entries) => {
        for (const e of entries) downloadEntry(e);
        return true;
      },
    },
    {
      key: "move",
      label: "Move to local",
      icon: Scissors,
      disabled: local.path === null,
      run: moveToLocal,
    },
    { key: "zip", label: "Download as zip", icon: FileArchive, run: downloadZip },
    { key: "delete", label: "Delete", icon: Trash2, danger: true, run: deleteRemote },
  ];

  return (
    <>
      {notice && (
        <Callout tone="success" onDismiss={() => setNotice(null)}>
          {notice}
        </Callout>
      )}
      <div
        className={[
          "sftp-panes",
          `sftp-panes-${layout}`,
          folded.local && "sftp-panes-fold-local",
          folded.remote && "sftp-panes-fold-remote",
        ]
          .filter(Boolean)
          .join(" ")}
        ref={split.container}
        style={{ ["--sftp-split" as string]: `${split.ratio * 100}%` }}
      >
        <FilePane
          title="Local"
          icon={HardDrive}
          layout={layout}
          folded={folded.local}
          onToggleFold={() => toggleFold("local")}
          caption={`Local files in ${local.path ?? "/"}`}
          path={local.path}
          entries={localEntries}
          loading={local.loading}
          error={local.error}
          onDismissError={() => local.setError(null)}
          onNavigate={local.setPath}
          onRefresh={local.refresh}
          pending={transfers.pending}
          origin={{ source: "local" }}
          onDrop={dropOnLocal}
          onEdit={(entry) => setEditing({ side: "local", path: entry.path })}
          onOpenExternal={(entry) =>
            setEditing({ side: "local", path: entry.path, external: true })
          }
          transfer={{
            icon: ArrowRight,
            label: `Upload to ${session.host.name}`,
            disabled: remote.path === null,
            onClick: uploadEntry,
          }}
          extraTools={
            <>
              <IconButton
                icon={House}
                label="Home folder"
                onClick={() => local.setPath(localHome)}
              />
              <IconButton
                icon={showHidden ? EyeOff : Eye}
                label={showHidden ? "Hide hidden files" : "Show hidden files"}
                onClick={() => setShowHidden((v) => !v)}
              />
            </>
          }
          onMkdir={async (name) => {
            if (local.path !== null) await fmMkdir(local.path, name);
          }}
          onRename={async (entry, name) => {
            await fmRename(entry.path, name);
          }}
          onDelete={async (entry) => {
            const ok = await confirmAction(
              `"${entry.name}" will be moved to the trash.`,
              {
                title: "Move to trash?",
                confirmLabel: "Trash",
                danger: true,
              },
            );
            if (!ok) return false;
            await fmTrash(entry.path);
            return true;
          }}
          deleteLabel="Move to trash"
        />
        <div
          className="sftp-divider"
          role="separator"
          aria-orientation={layout === "side" ? "vertical" : "horizontal"}
          aria-label="Resize panes"
          aria-valuenow={Math.round(split.ratio * 100)}
          tabIndex={0}
          onPointerDown={split.onPointerDown}
          onKeyDown={split.onKeyDown}
          onDoubleClick={split.reset}
        />
        <FilePane
          title={session.host.name}
          icon={Server}
          layout={layout}
          folded={folded.remote}
          onToggleFold={() => toggleFold("remote")}
          caption={`Files in ${remote.path ?? "/"} on ${session.host.name}`}
          path={remote.path}
          entries={remote.entries}
          loading={remote.loading}
          error={remote.error}
          onDismissError={() => remote.setError(null)}
          onNavigate={remote.setPath}
          onRefresh={remote.refresh}
          pending={transfers.pending}
          origin={{ source: "remote", session: session.id }}
          onDrop={dropOnRemote}
          onDropFiles={uploadFiles}
          onEdit={(entry) => setEditing({ side: "remote", path: entry.path })}
          onOpenExternal={(entry) =>
            setEditing({ side: "remote", path: entry.path, external: true })
          }
          transfer={{
            icon: ArrowLeft,
            label: "Download to local folder",
            disabled: local.path === null,
            onClick: downloadEntry,
          }}
          bulkActions={remoteBulk}
          extraTools={
            <>
              <IconButton
                icon={starred ? StarOff : Star}
                label={
                  starred
                    ? "Remove this folder from favourites"
                    : "Add this folder to favourites"
                }
                disabled={remote.path === null}
                onClick={toggleFavourite}
              />
              <Menu
                icon={Bookmark}
                label="Favourite folders"
                size="md"
                items={favouriteMenu}
              />
            </>
          }
          onMkdir={async (name) => {
            if (remote.path !== null)
              await sftpMkdir(session.id, joinPath(remote.path, name));
          }}
          onRename={async (entry, name) => {
            if (remote.path !== null)
              await sftpRename(
                session.id,
                entry.path,
                joinPath(remote.path, name),
              );
          }}
          onDelete={async (entry) => {
            const ok = await confirmAction(
              entry.is_dir
                ? `The directory "${entry.name}" will be removed. It must be empty.`
                : `"${entry.name}" will be permanently deleted on ${session.host.name}.`,
              {
                title: entry.is_dir ? "Remove directory?" : "Delete file?",
                confirmLabel: "Delete",
                danger: true,
              },
            );
            if (!ok) return false;
            await sftpDelete(session.id, entry.path);
            return true;
          }}
          deleteLabel="Delete"
        />
      </div>

      <TransferBar
        transfers={transfers.items}
        onClear={transfers.clearFinished}
      />

      {editingFavourites && (
        <FavouritesDialog
          host={host}
          paths={favouritePaths}
          onOpen={(p) => {
            remote.setPath(p);
            setEditingFavourites(false);
          }}
          onRemove={(p) =>
            favourites
              .remove(hostId, p)
              .catch((e) =>
                remote.setError(`Couldn't save favourites: ${errorMessage(e)}`),
              )
          }
          onClose={() => setEditingFavourites(false)}
        />
      )}

      {editing && (
        <EditFileModal
          path={editing.path}
          where={editing.side === "local" ? "Local" : session.host.name}
          read={() =>
            editing.side === "local"
              ? fmReadFile(editing.path)
              : sftpReadFile(session.id, editing.path)
          }
          write={(contents) =>
            editing.side === "local"
              ? fmWriteFile(editing.path, contents)
              : sftpWriteFile(session.id, editing.path, contents)
          }
          openExternal={async () => {
            const command = await getExternalEditor();
            if (editing.side === "local") {
              await fmOpenInEditor(editing.path, command);
              return editing.path;
            }
            return sftpOpenInEditor(session.id, editing.path, command);
          }}
          onExternalSaved={async (localPath) => {
            if (editing.side === "local") {
              local.refresh();
              return;
            }
            await sftpUpload(session.id, localPath, editing.path);
            remote.refresh();
          }}
          startExternal={editing.external}
          onClose={() => setEditing(null)}
          onSaved={(message) => {
            setNotice(message);
            if (editing.side === "local") local.refresh();
            else remote.refresh();
          }}
        />
      )}
    </>
  );
}

/* ------------------------------------------------------------------------ */

const SPLIT_KEY = "sftp.split";
const SPLIT_MIN = 0.2;
const SPLIT_MAX = 0.8;

/**
 * The draggable divider between the panes. The ratio is the left pane's share
 * of the width, handed to the grid as a CSS variable and remembered in
 * localStorage so it survives a restart. Drag with the pointer, nudge with the
 * arrow keys, double-click to go back to half and half.
 */
function usePaneSplit(layout: PaneLayout) {
  const container = useRef<HTMLDivElement>(null);
  const [ratio, setRatio] = useState(() => {
    const saved = Number(localStorage.getItem(SPLIT_KEY));
    return saved >= SPLIT_MIN && saved <= SPLIT_MAX ? saved : 0.5;
  });

  const commit = useCallback((next: number) => {
    const clamped = Math.min(SPLIT_MAX, Math.max(SPLIT_MIN, next));
    setRatio(clamped);
    localStorage.setItem(SPLIT_KEY, String(clamped));
  }, []);

  const onPointerDown = useCallback(
    (e: PointerEvent<HTMLDivElement>) => {
      const box = container.current?.getBoundingClientRect();
      if (!box) return;
      e.preventDefault();
      const divider = e.currentTarget;
      divider.setPointerCapture(e.pointerId);
      divider.classList.add("sftp-divider-dragging");
      const move = (ev: globalThis.PointerEvent) =>
        commit(
          layout === "side"
            ? (ev.clientX - box.left) / box.width
            : (ev.clientY - box.top) / box.height,
        );
      const up = () => {
        divider.classList.remove("sftp-divider-dragging");
        divider.removeEventListener("pointermove", move);
        divider.removeEventListener("pointerup", up);
        divider.removeEventListener("pointercancel", up);
      };
      divider.addEventListener("pointermove", move);
      divider.addEventListener("pointerup", up);
      divider.addEventListener("pointercancel", up);
    },
    [commit, layout],
  );

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLDivElement>) => {
      const back = layout === "side" ? "ArrowLeft" : "ArrowUp";
      const forward = layout === "side" ? "ArrowRight" : "ArrowDown";
      if (e.key === back) commit(ratio - 0.05);
      else if (e.key === forward) commit(ratio + 0.05);
      else if (e.key === "Home") commit(0.5);
      else return;
      e.preventDefault();
    },
    [commit, ratio, layout],
  );

  const reset = useCallback(() => commit(0.5), [commit]);

  return { container, ratio, onPointerDown, onKeyDown, reset };
}

type TransferStatus = "queued" | "running" | "done" | "error";

interface Transfer {
  id: string;
  name: string;
  direction: "upload" | "download";
  /** Path of the row it started from, so that row can show a spinner. */
  source: string;
  done: number;
  total: number;
  status: TransferStatus;
  error?: string;
}

interface TransferJob {
  id: string;
  run: (id: string) => Promise<unknown>;
  onDone: () => void;
}

/**
 * Transfers run one at a time — the Rust side holds the session for the whole
 * copy, so two at once would only queue there instead — but any number can be
 * clicked in; they line up in the bar and each reports its own progress.
 */
function useTransferQueue() {
  const [items, setItems] = useState<Transfer[]>([]);
  const queue = useRef<TransferJob[]>([]);
  const running = useRef(false);

  useEffect(() => {
    const unlisten = onSftpProgress((p) => {
      setItems((prev) =>
        prev.map((t) =>
          t.id === p.id ? { ...t, done: p.done, total: p.total } : t,
        ),
      );
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  const patch = useCallback((id: string, changes: Partial<Transfer>) => {
    setItems((prev) =>
      prev.map((t) => (t.id === id ? { ...t, ...changes } : t)),
    );
  }, []);

  const pump = useCallback(async () => {
    if (running.current) return;
    running.current = true;
    while (queue.current.length > 0) {
      const job = queue.current.shift()!;
      patch(job.id, { status: "running" });
      try {
        await job.run(job.id);
        patch(job.id, { status: "done" });
        job.onDone();
      } catch (e) {
        patch(job.id, { status: "error", error: errorMessage(e) });
      }
    }
    running.current = false;
  }, [patch]);

  const enqueue = useCallback(
    (
      info: Pick<Transfer, "name" | "direction" | "source">,
      run: (id: string) => Promise<unknown>,
      onDone: () => void,
    ) => {
      const id = crypto.randomUUID();
      setItems((prev) => [
        ...prev,
        { ...info, id, done: 0, total: 0, status: "queued" },
      ]);
      queue.current.push({ id, run, onDone });
      void pump();
    },
    [pump],
  );

  const clearFinished = useCallback(() => {
    setItems((prev) =>
      prev.filter((t) => t.status === "queued" || t.status === "running"),
    );
  }, []);

  const pending = useMemo(() => {
    const map = new Map<string, TransferStatus>();
    for (const t of items) {
      if (t.status === "queued" || t.status === "running")
        map.set(t.source, t.status);
    }
    return map;
  }, [items]);

  return { items, enqueue, clearFinished, pending };
}

function TransferBar({
  transfers,
  onClear,
}: {
  transfers: Transfer[];
  onClear: () => void;
}) {
  if (transfers.length === 0) return null;
  const active = transfers.filter(
    (t) => t.status === "queued" || t.status === "running",
  ).length;
  const finished = transfers.length - active;
  return (
    <Section
      padding="none"
      className="sftp-transfers"
      title="Transfers"
      headingLevel={3}
      meta={
        active > 0 ? (
          <Badge tone="accent">{active} active</Badge>
        ) : (
          <Badge>{finished} finished</Badge>
        )
      }
      actions={
        finished > 0 && (
          <Button variant="secondary" size="sm" onClick={onClear}>
            Clear finished
          </Button>
        )
      }
    >
      <ul className="sftp-transfer-list">
        {transfers.map((t) => (
          <TransferRow key={t.id} transfer={t} />
        ))}
      </ul>
    </Section>
  );
}

function TransferRow({ transfer: t }: { transfer: Transfer }) {
  const percent =
    t.total > 0 ? (t.done / t.total) * 100 : t.status === "done" ? 100 : 0;
  let status: ReactNode;
  if (t.status === "queued") status = <span className="u-muted">Waiting</span>;
  else if (t.status === "running")
    status = (
      <span className="u-tnum">{`${formatSize(t.done)} / ${formatSize(t.total)}`}</span>
    );
  else if (t.status === "done")
    status = <span className="u-tnum">{formatSize(t.done)}</span>;
  else status = <span className="sftp-transfer-error">{t.error}</span>;

  const icon =
    t.status === "done"
      ? CircleCheck
      : t.status === "error"
        ? CircleX
        : t.direction === "upload"
          ? Upload
          : Download;
  return (
    <li className={`sftp-transfer sftp-transfer-${t.status}`}>
      <span className="sftp-transfer-icon">
        <Icon icon={icon} size="sm" />
      </span>
      <Mono truncate>{t.name}</Mono>
      <span className="sftp-transfer-meter">
        <Meter
          variant="compact"
          percent={percent}
          warningAt={101}
          criticalAt={101}
        />
      </span>
      <span className="sftp-transfer-status">{status}</span>
    </li>
  );
}

/* ------------------------------------------------------------------------ */

interface PaneEntry extends SortableEntry {
  path: string;
  permissions: string;
}

interface TransferAction<T> {
  icon: LucideIcon;
  label: string;
  disabled: boolean;
  onClick: (entry: T) => void;
}

/** Something to do with every selected row at once. */
interface BulkAction<T> {
  key: string;
  label: string;
  icon: LucideIcon;
  danger?: boolean;
  disabled?: boolean;
  /** Resolves false when the user backed out, so the selection stays. */
  run: (entries: T[]) => Promise<boolean>;
}

function FilePane<T extends PaneEntry>({
  title,
  icon,
  layout,
  folded,
  onToggleFold,
  caption,
  path,
  entries,
  loading,
  error,
  onDismissError,
  onNavigate,
  onRefresh,
  pending,
  origin,
  onDrop,
  onDropFiles,
  onEdit,
  onOpenExternal,
  transfer,
  bulkActions,
  extraTools,
  onMkdir,
  onRename,
  onDelete,
  deleteLabel,
}: {
  title: string;
  icon: LucideIcon;
  layout: PaneLayout;
  /** Folded down to a rail so the other pane can have the room. */
  folded: boolean;
  onToggleFold: () => void;
  caption: string;
  path: string | null;
  entries: T[];
  loading: boolean;
  error: string | null;
  onDismissError: () => void;
  onNavigate: (path: string) => void;
  onRefresh: () => void;
  /** Rows (by path) with a transfer queued or running. */
  pending: Map<string, TransferStatus>;
  /** Which filesystem this pane shows, stamped on every drag that starts here. */
  origin: Pick<FileDragPayload, "source" | "session">;
  /**
   * A dragged row landed in `dir` — the pane's folder, or a folder row. Resolves
   * true when the listing changed and the pane should refresh.
   */
  onDrop: (payload: FileDragPayload, dir: string) => Promise<boolean>;
  /** Files dragged in from the desktop landed in `dir`. Absent: not a target. */
  onDropFiles?: (paths: string[], dir: string) => void;
  onEdit: (entry: T) => void;
  onOpenExternal: (entry: T) => void;
  transfer: TransferAction<T>;
  /**
   * Makes rows selectable — checkboxes, ctrl/shift-click, Ctrl+A — and shows
   * these over the table while anything is selected. Absent: one row at a time.
   */
  bulkActions?: BulkAction<T>[];
  extraTools?: ReactNode;
  onMkdir: (name: string) => Promise<void>;
  onRename: (entry: T, name: string) => Promise<void>;
  /** Resolves false when the user cancelled, so the pane doesn't refresh. */
  onDelete: (entry: T) => Promise<boolean>;
  deleteLabel: string;
}) {
  const [sort, setSort] = useState<SortState>({ key: "name", dir: "asc" });
  const [busy, setBusy] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [nameDialog, setNameDialog] = useState<
    { kind: "mkdir" } | { kind: "rename"; entry: T } | null
  >(null);

  // Drag and drop. `dropDir` is where a drag hovering over the pane would
  // land: the pane's own folder (the whole pane lights up) or a folder row
  // (that row does). Enter/leave are counted because a drag crossing the
  // table's cells fires a leave for every element it passes.
  const dropZone = useRef<HTMLDivElement>(null);
  const dragDepth = useRef(0);
  const [dropDir, setDropDir] = useState<string | null>(null);

  /** Where a drag over `target` would land, or null when a drop would do nothing. */
  function dropTargetOf(target: EventTarget | null): string | null {
    const dir = dropDirOf(target, path);
    if (dir === null) return null;
    const drag = currentFileDrag();
    if (drag && sameOrigin(drag, origin) && !canMoveInto(drag, dir)) return null;
    return dir;
  }

  function onDragEnter(e: DragEvent<HTMLDivElement>) {
    if (!hasFileDrag(e.dataTransfer)) return;
    dragDepth.current += 1;
    onDragOver(e);
  }

  function onDragOver(e: DragEvent<HTMLDivElement>) {
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

  function onDragLeave(e: DragEvent<HTMLDivElement>) {
    if (!hasFileDrag(e.dataTransfer)) return;
    dragDepth.current = Math.max(0, dragDepth.current - 1);
    if (dragDepth.current === 0) setDropDir(null);
  }

  function onDropRow(e: DragEvent<HTMLDivElement>) {
    if (!hasFileDrag(e.dataTransfer)) return;
    e.preventDefault();
    dragDepth.current = 0;
    setDropDir(null);
    const payload = readFileDrag(e.dataTransfer);
    const dir = dropTargetOf(e.target);
    endFileDrag();
    if (!payload || dir === null) return;
    void withBusy(() => onDrop(payload, dir));
  }

  useOsFileDrop({
    onHover: (point) => {
      if (!onDropFiles) return;
      setDropDir(point ? dirAtPoint(dropZone.current, point, path) : null);
    },
    onDrop: (paths, point) => {
      if (!onDropFiles) return;
      const dir = dirAtPoint(dropZone.current, point, path);
      if (dir !== null) onDropFiles(paths, dir);
    },
  });

  const sorted = useMemo(
    () => sortEntries(entries, sort.key as EntrySortKey, sort.dir),
    [entries, sort],
  );

  // Multi-select, by path, the file manager's way: click picks one, ctrl
  // toggles, shift extends from the anchor. A selection belongs to one folder.
  const selectable = bulkActions !== undefined;
  const [selected, setSelected] = useState<string[]>([]);
  const [anchor, setAnchor] = useState<string | null>(null);
  useEffect(() => {
    setSelected([]);
    setAnchor(null);
  }, [path]);
  const rowOrder = useMemo(() => sorted.map((e) => e.path), [sorted]);
  // Rows a refresh took away drop out here rather than lingering.
  const chosen = useMemo(() => {
    const set = new Set(selected);
    return sorted.filter((e) => set.has(e.path));
  }, [sorted, selected]);
  const chosenPaths = useMemo(() => new Set(chosen.map((e) => e.path)), [chosen]);
  const allChosen = sorted.length > 0 && chosen.length === sorted.length;

  function clearSelection() {
    setSelected([]);
    setAnchor(null);
  }

  function selectRow(clicked: string, mods: { ctrl: boolean; shift: boolean }) {
    const next = clickSelect(rowOrder, selected, anchor, clicked, mods);
    setSelected(next.selected);
    setAnchor(next.anchor);
  }

  function onSelectionKey(e: KeyboardEvent<HTMLDivElement>) {
    if ((e.key === "a" || e.key === "A") && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      setSelected(rowOrder);
    } else if (e.key === "Escape" && chosen.length > 0) {
      e.preventDefault();
      clearSelection();
    }
  }

  async function runBulk(action: BulkAction<T>) {
    setBusy(true);
    setLocalError(null);
    try {
      if (await action.run(chosen)) clearSelection();
    } catch (e) {
      setLocalError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  function openDir(entry: T) {
    if (entry.is_dir) onNavigate(entry.path);
  }

  function activate(entry: T) {
    if (entry.is_dir) onNavigate(entry.path);
    else onEdit(entry);
  }

  async function withBusy(action: () => Promise<boolean | void>) {
    setBusy(true);
    setLocalError(null);
    try {
      const result = await action();
      if (result !== false) onRefresh();
    } catch (e) {
      setLocalError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleNameSubmit(value: string) {
    if (path === null || !nameDialog) return;
    const name = value.trim();
    const dialog = nameDialog;
    setNameDialog(null);
    if (!name || name.includes("/")) {
      setLocalError("Enter a name without slashes.");
      return;
    }
    await withBusy(async () => {
      if (dialog.kind === "mkdir") await onMkdir(name);
      else if (name !== dialog.entry.name) await onRename(dialog.entry, name);
    });
  }

  // Rows arrive pre-sorted (directories first, then by the chosen column);
  // the constant sortValue keeps the header clickable without letting the
  // table re-sort and scatter folders through the files.
  const columns: Column<T>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      sortValue: () => 0,
      render: (e) => (
        <Stack direction="row" gap={2} align="center">
          <Icon icon={e.is_dir ? Folder : File} size="sm" />
          {e.is_dir ? (
            <Button variant="link" onClick={() => openDir(e)}>
              {e.name}
            </Button>
          ) : (
            <span>{e.name}</span>
          )}
        </Stack>
      ),
    },
    {
      key: "size",
      header: "Size",
      priority: 2,
      align: "end",
      sortValue: () => 0,
      render: (e) => (e.is_dir ? "—" : formatSize(e.size)),
    },
    {
      key: "modified",
      header: "Modified",
      priority: 2,
      sortValue: () => 0,
      render: (e) => formatModified(e.modified),
    },
    {
      key: "perm",
      header: "Mode",
      priority: 3,
      mono: true,
      render: (e) => <Mono>{e.permissions}</Mono>,
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (e) => (
        <ActionGroup>
          <IconButton
            icon={transfer.icon}
            label={transfer.label}
            loading={pending.get(e.path) === "running"}
            disabled={transfer.disabled || pending.has(e.path)}
            onClick={() => transfer.onClick(e)}
          />
          <Menu
            items={[
              ...(e.is_dir
                ? []
                : [
                    {
                      key: "edit",
                      label: "Edit",
                      icon: Pencil,
                      onClick: () => onEdit(e),
                    },
                    {
                      key: "external",
                      label: "Open in editor",
                      icon: SquareArrowOutUpRight,
                      onClick: () => onOpenExternal(e),
                    },
                  ]),
              {
                key: "rename",
                label: "Rename",
                icon: Pencil,
                onClick: () => setNameDialog({ kind: "rename", entry: e }),
              },
              {
                key: "delete",
                label: deleteLabel,
                icon: Trash2,
                danger: true,
                separatorBefore: true,
                onClick: () => void withBusy(() => onDelete(e)),
              },
            ]}
          />
        </ActionGroup>
      ),
    },
  ];

  const selectColumn: Column<T> = {
    key: "select",
    header: (
      <Checkbox
        label="Select all"
        hideLabel
        checked={allChosen}
        indeterminate={chosen.length > 0}
        disabled={sorted.length === 0}
        onChange={(on) => (on ? setSelected(rowOrder) : clearSelection())}
      />
    ),
    headerLabel: "Select",
    priority: 1,
    width: "1px",
    render: (e) => (
      <Checkbox
        label={`Select ${e.name}`}
        hideLabel
        checked={chosenPaths.has(e.path)}
        onChange={(on) => {
          setSelected((s) => [
            ...s.filter((p) => p !== e.path),
            ...(on ? [e.path] : []),
          ]);
          setAnchor(e.path);
        }}
      />
    ),
  };

  const crumbs = splitBreadcrumb(path ?? "/");
  const shownError = error ?? localError;

  const unfoldIcon = layout === "side" ? ChevronsLeftRight : ChevronsUpDown;
  const foldIcon = layout === "side" ? ChevronsRightLeft : ChevronsDownUp;

  if (folded) {
    return (
      <Section padding="none" className="sftp-pane sftp-pane-rail">
        <div className="sftp-rail">
          <IconButton
            icon={unfoldIcon}
            label={`Show ${title}`}
            onClick={onToggleFold}
          />
          <span className="sftp-pane-title">
            <Icon icon={icon} size="sm" />
            {title}
          </span>
        </div>
      </Section>
    );
  }

  return (
    <Section
      padding="none"
      fill
      className="sftp-pane"
      toolbar={
        <Toolbar
          end={
            <>
              {extraTools}
              <IconButton
                icon={ArrowUp}
                label="Up one level"
                disabled={path === null || path === "/"}
                onClick={() => path !== null && onNavigate(parentPath(path))}
              />
              <IconButton
                icon={RefreshCw}
                label="Refresh"
                loading={loading}
                onClick={onRefresh}
              />
              <IconButton
                icon={FolderPlus}
                label="New folder"
                disabled={path === null || busy}
                onClick={() => setNameDialog({ kind: "mkdir" })}
              />
              <IconButton
                icon={foldIcon}
                label={`Hide ${title}`}
                onClick={onToggleFold}
              />
            </>
          }
        >
          <span className="sftp-pane-title">
            <Icon icon={icon} size="sm" />
            {title}
          </span>
          <div className="sftp-crumbs">
            {crumbs.map((crumb, i) => (
              <span key={crumb.path} className="sftp-crumb">
                {i > 0 && <span className="sftp-crumb-sep">/</span>}
                <Button
                  variant="link"
                  size="sm"
                  onClick={() => onNavigate(crumb.path)}
                >
                  {crumb.label}
                </Button>
              </span>
            ))}
          </div>
        </Toolbar>
      }
    >
      {shownError && (
        <Callout
          tone="danger"
          onDismiss={() => {
            onDismissError();
            setLocalError(null);
          }}
        >
          {shownError}
        </Callout>
      )}
      {bulkActions && chosen.length > 0 && (
        <Toolbar
          className="sftp-selection"
          end={
            <>
              {bulkActions.map((action) => (
                <Button
                  key={action.key}
                  size="sm"
                  variant={action.danger ? "danger" : "secondary"}
                  icon={action.icon}
                  disabled={busy || action.disabled}
                  onClick={() => void runBulk(action)}
                >
                  {action.label}
                </Button>
              ))}
              <IconButton
                icon={X}
                label="Clear selection"
                size="sm"
                onClick={clearSelection}
              />
            </>
          }
        >
          <span className="sftp-selection-count">
            {countLabel(chosen.length)} selected
          </span>
        </Toolbar>
      )}
      <div
        ref={dropZone}
        className={[
          "sftp-drop",
          dropDir !== null && dropDir === path && "sftp-drop-active",
        ]
          .filter(Boolean)
          .join(" ")}
        /* Focusable so Ctrl+A and Escape reach it once a row is clicked. */
        tabIndex={selectable ? -1 : undefined}
        onKeyDown={selectable ? onSelectionKey : undefined}
        onDragEnter={onDragEnter}
        onDragOver={onDragOver}
        onDragLeave={onDragLeave}
        onDrop={onDropRow}
      >
        <DataTable
          columns={selectable ? [selectColumn, ...columns] : columns}
          rows={sorted}
          rowKey={(e) => e.path}
          rowProps={(e) => ({
            draggable: true,
            "data-drop-dir": e.is_dir ? e.path : undefined,
            onDragStart: (ev) =>
              startFileDrag(ev.dataTransfer, {
                ...origin,
                path: e.path,
                name: e.name,
                is_dir: e.is_dir,
              }),
            onDragEnd: endFileDrag,
            onClick: (ev) => {
              // The row's own buttons and checkbox do their own thing.
              const own = (ev.target as HTMLElement).closest("button, input, label, a");
              if (!selectable || own) return;
              selectRow(e.path, {
                ctrl: ev.ctrlKey || ev.metaKey,
                shift: ev.shiftKey,
              });
            },
            // Shift-click extends the selection, not a text highlight.
            onMouseDown: (ev) => {
              if (selectable && ev.shiftKey) ev.preventDefault();
            },
          })}
          rowClassName={(e) =>
            [
              e.is_dir && dropDir === e.path && "sftp-drop-row",
              chosenPaths.has(e.path) && "sftp-row-selected",
            ]
              .filter(Boolean)
              .join(" ") || undefined
          }
          sort={sort}
          onSortChange={setSort}
          loading={loading && entries.length === 0}
          onRowDoubleClick={activate}
          density="compact"
          fillHeight
          caption={caption}
          empty={{ icon: Folder, title: "Empty directory" }}
        />
      </div>

      {nameDialog && (
        <NameDialog
          title={
            nameDialog.kind === "mkdir"
              ? "New folder"
              : `Rename ${nameDialog.entry.name}`
          }
          initial={nameDialog.kind === "mkdir" ? "" : nameDialog.entry.name}
          confirmLabel={nameDialog.kind === "mkdir" ? "Create" : "Rename"}
          onSubmit={handleNameSubmit}
          onClose={() => setNameDialog(null)}
        />
      )}
    </Section>
  );
}

/**
 * The host's favourite folders, each one to open or remove. The menu in the
 * pane opens them too, but a menu item has room for one action, and a folder
 * deleted on the server is one you want to remove without opening first.
 */
function FavouritesDialog({
  host,
  paths,
  onOpen,
  onRemove,
  onClose,
}: {
  host: string;
  paths: string[];
  onOpen: (path: string) => void;
  onRemove: (path: string) => void;
  onClose: () => void;
}) {
  const columns: Column<string>[] = [
    {
      key: "path",
      header: "Folder",
      priority: 1,
      render: (p) => (
        <Button variant="link" onClick={() => onOpen(p)}>
          <Mono>{p}</Mono>
        </Button>
      ),
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (p) => (
        <IconButton
          icon={StarOff}
          label={`Remove ${p} from favourites`}
          onClick={() => onRemove(p)}
        />
      ),
    },
  ];
  return (
    <Modal
      size="md"
      title={`Favourite folders on ${host}`}
      icon={Bookmark}
      onClose={onClose}
    >
      <DataTable
        columns={columns}
        rows={paths}
        rowKey={(p) => p}
        density="compact"
        minWidth={0}
        caption={`Favourite folders on ${host}`}
        empty={{
          icon: Star,
          title: "No favourites",
          description: "Star a folder in the path bar to add it here.",
        }}
      />
    </Modal>
  );
}

function NameDialog({
  title,
  initial,
  confirmLabel,
  onSubmit,
  onClose,
}: {
  title: string;
  initial: string;
  confirmLabel: string;
  onSubmit: (value: string) => void;
  onClose: () => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <Modal size="sm" title={title} onClose={onClose}>
      <Form onSubmit={() => onSubmit(value)}>
        <Stack gap={5}>
          <Field label="Name" htmlFor="sftp-name">
            <TextInput
              id="sftp-name"
              mono
              value={value}
              onChange={(e) => setValue(e.target.value)}
              autoFocus
            />
          </Field>
          <FormRow className="form-actions">
            <Button variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={!value.trim()}>
              {confirmLabel}
            </Button>
          </FormRow>
        </Stack>
      </Form>
    </Modal>
  );
}
