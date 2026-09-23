import { KeyboardEvent, useCallback, useEffect, useMemo, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useSearchParams } from "react-router-dom";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import { usePopoutHandoff } from "../../../app/popoutState";
import {
  ActionGroup,
  Button,
  Callout,
  Column,
  CopyButton,
  DataTable,
  EmptyState,
  Icon,
  IconButton,
  Menu,
  MenuItem,
  Mono,
  Page,
  SearchInput,
  Section,
  SegmentedControl,
  SideList,
  SortState,
  SplitView,
  statusOf,
  useResource,
} from "../../../ui";
import {
  ArrowUp,
  ChevronLeft,
  ChevronRight,
  Download,
  Eye,
  EyeOff,
  File,
  FilePlus,
  FileText,
  Film,
  Folder,
  FolderPlus,
  HardDrive,
  House,
  Image,
  LayoutGrid,
  List,
  LucideIcon,
  Monitor,
  Music,
  Pencil,
  RefreshCw,
  SquareArrowOutUpRight,
  Trash2,
} from "../../../ui/icons";
import { FileEntry, PlaceKind, fmCreateFile, fmList, fmMkdir, fmOpen, fmPlaces, fmRename, fmTrash } from "../api";
import FileEditorModal from "../components/FileEditorModal";
import NameDialog from "../components/NameDialog";
import {
  EntrySortKey,
  History,
  canGoBack,
  canGoForward,
  errorMessage,
  fileKind,
  filterEntries,
  formatModified,
  formatSize,
  historyPush,
  historyStep,
  isEditable,
  parentPath,
  sortEntries,
  splitBreadcrumb,
  tildePath,
} from "../utils";
import "./FileManagerPage.css";

const PLACE_ICON: Record<PlaceKind, LucideIcon> = {
  home: House,
  desktop: Monitor,
  documents: FileText,
  downloads: Download,
  pictures: Image,
  music: Music,
  videos: Film,
  root: HardDrive,
};

type View = "list" | "grid";
type Dialog =
  | { kind: "mkdir" }
  | { kind: "newfile" }
  | { kind: "rename"; entry: FileEntry }
  | { kind: "edit"; path: string };

const EMPTY_HISTORY: History = { entries: [], index: -1 };

/**
 * A Finder-style explorer: places down the side, back/forward and a path bar
 * on top, the listing in the middle and a status line underneath.
 *
 * Everything here is one directory deep on purpose — no drag and drop, no
 * multi-select, no copy/move between folders. Those are the parts of a file
 * manager that take months to get right, and the system one is a click away
 * via "Reveal". What this page is for is the ten-second job: find the project,
 * open the config file, rename the folder, bin the stale build directory.
 */
export default function FileManagerPage() {
  const module = useModule("file-manager");
  const confirmAction = useConfirm();
  const places = useResource(fmPlaces);
  // `?path=` opens on that folder instead of home: what a popped-out window
  // gets so it continues where the main one was.
  const [params] = useSearchParams();
  const startPath = params.get("path");

  const [history, setHistory] = useState<History>(EMPTY_HISTORY);
  const path = history.entries[history.index] ?? null;
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [sort, setSort] = useState<SortState>({ key: "name", dir: "asc" });
  const [query, setQuery] = useState("");
  const [showHidden, setShowHidden] = useState(false);
  const [view, setView] = useState<View>("list");
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);

  const home = places.data?.home ?? "";

  const navigate = useCallback((target: string) => {
    setHistory((h) => historyPush(h, target));
    setQuery("");
    setSelected(null);
  }, []);

  // Start at home once the places arrive, or at the folder the route asks for.
  useEffect(() => {
    if (places.data && path === null) navigate(startPath || places.data.home);
  }, [places.data, path, navigate, startPath]);

  usePopoutHandoff(() => (path ? { params: { path } } : null));

  const load = useCallback(async (target: string) => {
    setLoading(true);
    setError(null);
    try {
      setEntries(await fmList(target));
    } catch (e) {
      setError(errorMessage(e));
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (path !== null) void load(path);
  }, [path, load]);

  const visible = useMemo(
    () => sortEntries(filterEntries(entries, query, showHidden), sort.key as EntrySortKey, sort.dir),
    [entries, query, showHidden, sort],
  );
  const selectedEntry = visible.find((e) => e.path === selected) ?? null;

  function open(entry: FileEntry) {
    if (entry.is_dir) navigate(entry.path);
    else if (isEditable(entry)) setDialog({ kind: "edit", path: entry.path });
    else void run(entry.path, () => fmOpen(entry.path));
  }

  async function run(key: string, action: () => Promise<unknown>, reloadAfter = false) {
    setBusy(key);
    setError(null);
    setNotice(null);
    try {
      const result = await action();
      if (typeof result === "string") setNotice(result);
      if (reloadAfter && path !== null) await load(path);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  async function handleTrash(entry: FileEntry) {
    const ok = await confirmAction(
      `"${entry.name}" will be moved to the trash. You can restore it from your desktop's trash.`,
      { title: entry.is_dir ? "Move folder to trash?" : "Move file to trash?", confirmLabel: "Move to trash", danger: true },
    );
    if (!ok) return;
    setSelected(null);
    await run(entry.path, () => fmTrash(entry.path), true);
  }

  async function handleNameSubmit(value: string) {
    if (path === null || !dialog || dialog.kind === "edit") return;
    const current = dialog;
    setDialog(null);
    await run(
      "name",
      async () => {
        if (current.kind === "mkdir") return fmMkdir(path, value);
        if (current.kind === "newfile") return fmCreateFile(path, value);
        return fmRename(current.entry.path, value);
      },
      true,
    );
  }

  function handleKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    const target = e.target as HTMLElement;
    if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;
    if (e.key === "Backspace" && path && path !== "/") {
      e.preventDefault();
      navigate(parentPath(path));
    } else if (e.key === "Enter" && selectedEntry) {
      e.preventDefault();
      open(selectedEntry);
    } else if (e.key === "Escape") {
      setSelected(null);
    }
  }

  function rowActions(entry: FileEntry): MenuItem[] {
    const items: MenuItem[] = [];
    if (entry.is_dir) {
      items.push({ key: "open", label: "Open", icon: Folder, onClick: () => open(entry) });
    } else {
      if (isEditable(entry)) {
        items.push({ key: "edit", label: "Edit", icon: Pencil, onClick: () => setDialog({ kind: "edit", path: entry.path }) });
      }
      items.push({
        key: "open",
        label: "Open with default app",
        icon: SquareArrowOutUpRight,
        onClick: () => void run(entry.path, () => fmOpen(entry.path)),
      });
    }
    items.push({
      key: "reveal",
      label: "Reveal in system file manager",
      icon: HardDrive,
      onClick: () => void run(entry.path, () => revealItemInDir(entry.path)),
    });
    items.push({
      key: "rename",
      label: "Rename",
      icon: Pencil,
      separatorBefore: true,
      onClick: () => setDialog({ kind: "rename", entry }),
    });
    items.push({
      key: "trash",
      label: "Move to trash",
      icon: Trash2,
      danger: true,
      onClick: () => void handleTrash(entry),
    });
    return items;
  }

  // Rows arrive pre-sorted with folders first; the constant sortValue keeps
  // the headers clickable without letting the table re-sort on its own.
  const columns: Column<FileEntry>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      sortValue: () => 0,
      render: (e) => (
        <span className="fm-name">
          <Icon icon={e.is_dir ? Folder : File} size="sm" />
          <span className={e.is_hidden ? "fm-name-text u-muted" : "fm-name-text"}>{e.name}</span>
          {e.is_symlink && <span className="u-muted">→</span>}
        </span>
      ),
    },
    {
      key: "modified",
      header: "Date modified",
      priority: 2,
      sortValue: () => 0,
      render: (e) => formatModified(e.modified),
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
      key: "kind",
      header: "Kind",
      priority: 3,
      sortValue: () => 0,
      render: (e) => <span className="u-secondary">{fileKind(e)}</span>,
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (e) => (
        <ActionGroup>
          <Menu items={rowActions(e)} size="sm" />
        </ActionGroup>
      ),
    },
  ];

  const crumbs = splitBreadcrumb(path ?? "/");
  const placeItems = (places.data?.places ?? []).map((p) => ({
    key: p.path,
    label: p.label,
    icon: <Icon icon={PLACE_ICON[p.kind] ?? Folder} size="sm" />,
    active: path === p.path,
    onSelect: () => navigate(p.path),
  }));

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(places)}
      actions={
        <>
          <Button variant="secondary" size="sm" icon={FolderPlus} disabled={path === null} onClick={() => setDialog({ kind: "mkdir" })}>
            New folder
          </Button>
          <Button variant="secondary" size="sm" icon={FilePlus} disabled={path === null} onClick={() => setDialog({ kind: "newfile" })}>
            New file
          </Button>
        </>
      }
    >
      <Section padding="none" className="fm">
        <SplitView asideLabel="Places" aside={<SideList title="Favourites" items={placeItems} />}>
          <div className="fm-main" onKeyDown={handleKeyDown}>
            <div className="fm-toolbar">
              <ActionGroup>
                <IconButton icon={ChevronLeft} label="Back" disabled={!canGoBack(history)} onClick={() => setHistory((h) => historyStep(h, -1))} />
                <IconButton icon={ChevronRight} label="Forward" disabled={!canGoForward(history)} onClick={() => setHistory((h) => historyStep(h, 1))} />
                <IconButton icon={ArrowUp} label="Enclosing folder" disabled={!path || path === "/"} onClick={() => path && navigate(parentPath(path))} />
              </ActionGroup>
              <nav className="fm-crumbs" aria-label="Current folder">
                {crumbs.map((crumb, i) => (
                  <span key={crumb.path} className="fm-crumb">
                    {i > 0 && <span className="fm-crumb-sep">/</span>}
                    <Button variant="link" size="sm" onClick={() => navigate(crumb.path)}>
                      {crumb.label}
                    </Button>
                  </span>
                ))}
              </nav>
              <div className="fm-toolbar-end">
                <SearchInput value={query} onChange={setQuery} placeholder="Filter" width={160} aria-label="Filter this folder" />
                <SegmentedControl
                  ariaLabel="View"
                  size="sm"
                  iconOnly
                  value={view}
                  onChange={setView}
                  options={[
                    { value: "list", label: "List", icon: List },
                    { value: "grid", label: "Icons", icon: LayoutGrid },
                  ]}
                />
                <IconButton
                  icon={showHidden ? EyeOff : Eye}
                  label={showHidden ? "Hide hidden files" : "Show hidden files"}
                  onClick={() => setShowHidden((v) => !v)}
                />
                <IconButton icon={RefreshCw} label="Refresh" loading={loading} onClick={() => path && load(path)} />
              </div>
            </div>

            {error && (
              <Callout tone="danger" onDismiss={() => setError(null)}>
                {error}
              </Callout>
            )}
            {notice && (
              <Callout tone="success" onDismiss={() => setNotice(null)}>
                {notice}
              </Callout>
            )}

            <div className="fm-body">
              {view === "list" ? (
                <DataTable
                  columns={columns}
                  rows={visible}
                  rowKey={(e) => e.path}
                  sort={sort}
                  onSortChange={setSort}
                  loading={loading && entries.length === 0}
                  onRowClick={(e) => setSelected(e.path)}
                  onRowDoubleClick={open}
                  rowClassName={(e) => (e.path === selected ? "fm-row-selected" : undefined)}
                  density="compact"
                  stickyHeader
                  caption={`Contents of ${path ?? ""}`}
                  empty={{
                    icon: Folder,
                    title: query ? "Nothing matches" : "Empty folder",
                    description: query ? `No names in this folder contain "${query}".` : undefined,
                  }}
                />
              ) : visible.length === 0 ? (
                <EmptyState icon={Folder} title={query ? "Nothing matches" : "Empty folder"} size="md" />
              ) : (
                <ul className="fm-grid" aria-label={`Contents of ${path ?? ""}`}>
                  {visible.map((e) => (
                    <li key={e.path}>
                      <button
                        type="button"
                        className={e.path === selected ? "fm-tile fm-tile-selected" : "fm-tile"}
                        aria-pressed={e.path === selected}
                        onClick={() => setSelected(e.path)}
                        onDoubleClick={() => open(e)}
                        title={e.name}
                      >
                        <Icon icon={e.is_dir ? Folder : File} size="lg" className={e.is_dir ? "fm-tile-icon fm-tile-icon-dir" : "fm-tile-icon"} />
                        <span className={e.is_hidden ? "fm-tile-name u-muted" : "fm-tile-name"}>{e.name}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>

            <div className="fm-status">
              <span className="u-secondary u-nowrap">
                {visible.length} {visible.length === 1 ? "item" : "items"}
                {selectedEntry && ` · ${selectedEntry.name}${selectedEntry.is_dir ? "" : `, ${formatSize(selectedEntry.size)}`}`}
              </span>
              <span className="fm-status-path">
                <Mono truncate title={selectedEntry?.path ?? path ?? ""}>
                  {tildePath(selectedEntry?.path ?? path ?? "", home)}
                </Mono>
                <CopyButton value={selectedEntry?.path ?? path ?? ""} label="Copy path" size="sm" />
              </span>
            </div>
          </div>
        </SplitView>
      </Section>

      {dialog && dialog.kind !== "edit" && (
        <NameDialog
          title={dialog.kind === "mkdir" ? "New folder" : dialog.kind === "newfile" ? "New file" : `Rename ${dialog.entry.name}`}
          initial={dialog.kind === "rename" ? dialog.entry.name : ""}
          confirmLabel={dialog.kind === "rename" ? "Rename" : "Create"}
          onSubmit={handleNameSubmit}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.kind === "edit" && (
        <FileEditorModal
          path={dialog.path}
          onClose={() => setDialog(null)}
          onSaved={(message) => {
            setNotice(message);
            if (path) void load(path);
          }}
        />
      )}
      {busy && <span className="u-sr-only">Working on {busy}</span>}
    </Page>
  );
}
