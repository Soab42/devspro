import { CSSProperties, DragEvent, KeyboardEvent, MouseEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useNavigate, useSearchParams } from "react-router-dom";
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
  Camera,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Check,
  Cloud,
  Code,
  Columns3,
  Disc,
  Download,
  Eject,
  ExternalLink,
  Eye,
  EyeOff,
  FilePlus,
  FileText,
  Film,
  Folder,
  FolderPlus,
  HardDrive,
  House,
  Image,
  Info,
  LayoutGrid,
  List,
  LucideIcon,
  Monitor,
  Music,
  Network,
  Pencil,
  RefreshCw,
  SlidersHorizontal,
  Smartphone,
  SquareArrowOutUpRight,
  Star,
  StarOff,
  Terminal,
  Trash2,
  Usb,
} from "../../../ui/icons";
import { getExternalEditor } from "../../settings/api";
import {
  Device,
  DeviceKind,
  EditorApp,
  FileEntry,
  PlaceKind,
  fmCreateFile,
  fmDeviceAction,
  fmDisplayNames,
  fmEditors,
  fmList,
  fmMkdir,
  fmMove,
  fmOpen,
  fmOpenInEditor,
  fmOpenTerminal,
  fmPlaces,
  fmRename,
  fmTrash,
} from "../api";
import ColumnView, { DragHandlers, SelectMods } from "../components/ColumnView";
import ContextMenu, { ContextMenuState } from "../components/ContextMenu";
import Thumbnail from "../components/Thumbnail";
import { entryIcon } from "../entryIcon";
import { useFavourites } from "../favourites";
import { REMOTE_KINDS, breadcrumb, deviceAt, friendlyPath, useDevices } from "../devices";
import {
  closeTab,
  closeTabsUnder,
  cycleTab,
  drafts,
  moveTab,
  openTab,
  retargetDrafts,
  retargetTabs,
  setDirty,
  useOpenFiles,
} from "../openFiles";
import FilePreview from "../components/FilePreview";
import FileTab from "../components/FileTab";
import FileTabStrip from "../components/FileTabStrip";
import NameDialog from "../components/NameDialog";
import PropertiesModal from "../components/PropertiesModal";
import {
  EntrySortKey,
  History,
  TreeRow,
  baseName,
  canGoBack,
  canGoForward,
  clickSelect,
  errorMessage,
  fileKind,
  filterEntries,
  flattenTree,
  formatModified,
  formatSize,
  historyPush,
  historyStep,
  parentPath,
  planMove,
  sortEntries,
  THUMB_DEFAULT,
  THUMB_MAX,
  THUMB_MIN,
  THUMB_PRESETS,
  clampThumb,
  thumbRenderSize,
  tildePath,
  viewerKind,
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

const DEVICE_ICON: Record<DeviceKind, LucideIcon> = {
  usb: Usb,
  drive: HardDrive,
  phone: Smartphone,
  camera: Camera,
  disc: Disc,
  cloud: Cloud,
  network: Network,
};

type View = "list" | "grid" | "columns";
type Dialog =
  | { kind: "mkdir" }
  | { kind: "newfile" }
  | { kind: "rename"; entry: FileEntry }
  | { kind: "props"; entries: FileEntry[] };

const EMPTY_HISTORY: History = { entries: [], index: -1 };

/* The icon view's size is a per-viewer convenience, like the sidebar width. */
const THUMB_KEY = "fm.thumb-size";

function readThumbSize(): number {
  try {
    const raw = window.localStorage.getItem(THUMB_KEY);
    return raw ? clampThumb(Number(raw)) : THUMB_DEFAULT;
  } catch {
    return THUMB_DEFAULT;
  }
}
const TREE_INDENT = 18;

const modsOf = (e: MouseEvent | KeyboardEvent): SelectMods => ({ ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey });

/**
 * A Finder-style explorer: places down the side, back/forward and a path bar
 * on top, the listing in the middle and a status line underneath. Three
 * views of the same folder: a list whose folders fold open in place, an icon
 * grid, and Miller columns with a preview pane. Selection is multi (ctrl and
 * shift), and any selection can be dragged onto a folder, a column or a place.
 *
 * USB drives, phones, Google Drive accounts and network shares are listed
 * under the places, mounted on a click, and browsed like any other folder.
 * Files open as tabs beside the folder view, editor-style: source and text in
 * the editor, pictures, audio, video and PDFs in viewers. Anything else goes
 * to the desktop's own app. No copy, no archives: "Reveal" hands the rest to
 * the system file manager.
 */
export default function FileManagerPage() {
  const module = useModule("file-manager");
  const confirmAction = useConfirm();
  const places = useResource(fmPlaces);
  const editors = useResource(fmEditors);
  const favourites = useFavourites();
  const devices = useDevices();
  const [tabs, updateTabs] = useOpenFiles();
  // Folder and file titles where they differ from the name on disk (Google
  // Drive names everything by ID), for the path bar and the status line.
  const [titles, setTitles] = useState<Record<string, string>>({});
  // The device being mounted or ejected.
  const [deviceBusy, setDeviceBusy] = useState<string | null>(null);
  const routerNavigate = useNavigate();
  // `?path=` opens on that folder instead of home: what a popped-out window
  // gets so it continues where the main one was.
  const [params] = useSearchParams();
  const startPath = params.get("path");

  const [history, setHistory] = useState<History>(EMPTY_HISTORY);
  const path = history.entries[history.index] ?? null;
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [reloadToken, setReloadToken] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [sort, setSort] = useState<SortState>({ key: "name", dir: "asc" });
  const [query, setQuery] = useState("");
  const [showHidden, setShowHidden] = useState(false);
  const [view, setView] = useState<View>("list");
  // Icon view: how big each icon is, and the size thumbnails are rendered at.
  // The render size trails the slider so a drag does not re-render every step.
  const [thumbSize, setThumbSizeState] = useState(readThumbSize);
  const [thumbRender, setThumbRender] = useState(() => thumbRenderSize(readThumbSize(), window.devicePixelRatio));
  const setThumbSize = useCallback((size: number) => {
    const next = clampThumb(size);
    setThumbSizeState(next);
    try {
      window.localStorage.setItem(THUMB_KEY, String(next));
    } catch {
      // Private window or blocked storage: the size just is not remembered.
    }
  }, []);
  useEffect(() => {
    const t = window.setTimeout(() => setThumbRender(thumbRenderSize(thumbSize, window.devicePixelRatio)), 250);
    return () => window.clearTimeout(t);
  }, [thumbSize]);
  // The folder view takes the keyboard: rows are not focusable, so without
  // this a click on one left the focus on the page and no shortcut reached it.
  const folderRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [anchor, setAnchor] = useState<string | null>(null);
  // List view: folders folded open in place, and the listings behind them.
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [children, setChildren] = useState<Record<string, FileEntry[] | undefined>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const dragPaths = useRef<string[]>([]);
  const [dragging, setDragging] = useState(false);
  const [dropOn, setDropOn] = useState<string | null>(null);

  const home = places.data?.home ?? "";
  // Devices count as places for the column view: their columns start at the device.
  const placePaths = useMemo(
    () => [...(places.data?.places ?? []).map((p) => p.path), ...devices.list.flatMap((d) => (d.path ? [d.path] : []))],
    [places.data, devices.list],
  );
  const describe = useCallback((p: string) => friendlyPath(p, devices.list, titles, home), [devices.list, titles, home]);

  const rememberTitles = useCallback((list: FileEntry[]) => {
    setTitles((t) => {
      let next = t;
      for (const e of list) {
        if (e.name === baseName(e.path) || t[e.path] === e.name) continue;
        if (next === t) next = { ...t };
        next[e.path] = e.name;
      }
      return next;
    });
  }, []);

  const navigate = useCallback((target: string, select: string | null = null, mods: SelectMods = {}) => {
    setHistory((h) => historyPush(h, target));
    setQuery((q) => (select === null ? "" : q));
    setExpanded(new Set());
    if (mods.ctrl || mods.shift) return;
    setSelected(select === null ? [] : [select]);
    setAnchor(select);
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
      const list = await fmList(target);
      setEntries(list);
      rememberTitles(list);
      setChildren({});
      setReloadToken((n) => n + 1);
    } catch (e) {
      setError(errorMessage(e));
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, [rememberTitles]);

  useEffect(() => {
    if (path !== null) void load(path);
  }, [path, load]);

  // A Drive folder reached by a favourite or a restored window has never had
  // its parents listed; ask for their titles so the path bar is not a row of IDs.
  useEffect(() => {
    if (path === null || deviceAt(path, devices.list)?.kind !== "cloud") return;
    const missing = breadcrumb(path, devices.list, titles)
      .slice(1)
      .map((c) => c.path)
      .filter((p) => titles[p] === undefined);
    if (missing.length === 0) return;
    let cancelled = false;
    fmDisplayNames(missing)
      .then((names) => !cancelled && setTitles((t) => ({ ...t, ...Object.fromEntries(missing.map((p, i) => [p, names[i]])) })))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [path, devices.list, titles]);

  // Any folder folded open without a listing yet gets one.
  useEffect(() => {
    for (const dir of expanded) {
      if (children[dir] !== undefined) continue;
      fmList(dir)
        .then((list) => {
          rememberTitles(list);
          setChildren((c) => ({ ...c, [dir]: list }));
        })
        .catch((e) => {
          setError(errorMessage(e));
          setExpanded((x) => {
            const next = new Set(x);
            next.delete(dir);
            return next;
          });
        });
    }
  }, [expanded, children, rememberTitles]);

  // Editor-style tab keys, wherever the focus is on the page: Ctrl+Tab and
  // Ctrl+PageDown/PageUp step through the tabs, Ctrl+W closes the file in front.
  const tabKeys = useRef({ tabs, closeFileTab });
  tabKeys.current = { tabs, closeFileTab };
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const { tabs: open, closeFileTab: close } = tabKeys.current;
      if (open.files.length === 0) return;
      if (e.key === "Tab" || e.key === "PageDown" || e.key === "PageUp") {
        e.preventDefault();
        const back = e.key === "PageUp" || (e.key === "Tab" && e.shiftKey);
        updateTabs((s) => cycleTab(s, back ? -1 : 1));
      } else if (e.key.toLowerCase() === "w" && !e.shiftKey && open.active) {
        e.preventDefault();
        void close(open.active);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [updateTabs]);

  // After moving to another folder, or back to the folder view from a file
  // tab, keys go to the listing — unless the user is typing somewhere.
  const folderInFront = tabs.active === null;
  useEffect(() => {
    if (!folderInFront) return;
    const el = document.activeElement as HTMLElement | null;
    const typing = el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
    if (!typing && !folderRef.current?.contains(el)) folderRef.current?.focus({ preventScroll: true });
  }, [path, folderInFront]);

  // Ctrl+wheel over the icon view makes the icons bigger or smaller, as in
  // Windows Explorer. Native and not passive so the app's own zoom stays out.
  const thumbRef = useRef({ view, thumbSize, setThumbSize });
  thumbRef.current = { view, thumbSize, setThumbSize };
  useEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      const { view: v, thumbSize: size, setThumbSize: set } = thumbRef.current;
      if (!(e.ctrlKey || e.metaKey) || v !== "grid") return;
      e.preventDefault();
      e.stopPropagation();
      set(size + (e.deltaY < 0 ? 16 : -16));
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  const sortKey = sort.key as EntrySortKey;
  const visible = useMemo(() => sortEntries(filterEntries(entries, query, showHidden), sortKey, sort.dir), [entries, query, showHidden, sortKey, sort.dir]);
  // Sub-listings follow the hidden toggle and sort order; the filter box only
  // narrows the folder being looked at.
  const childView = useMemo(() => {
    const out: Record<string, FileEntry[] | undefined> = {};
    for (const [dir, list] of Object.entries(children)) if (list) out[dir] = sortEntries(filterEntries(list, "", showHidden), sortKey, sort.dir);
    return out;
  }, [children, showHidden, sortKey, sort.dir]);
  const treeRows = useMemo(() => flattenTree(visible, expanded, childView), [visible, expanded, childView]);

  // Every entry on screen by path, so a selection made in a folded-open
  // subfolder resolves the same way as one in the folder itself.
  const entryByPath = useMemo(() => {
    const map = new Map<string, FileEntry>();
    for (const e of entries) map.set(e.path, e);
    for (const list of Object.values(children)) for (const e of list ?? []) map.set(e.path, e);
    return map;
  }, [entries, children]);
  const selectedEntries = useMemo(() => selected.map((p) => entryByPath.get(p)).filter((e): e is FileEntry => Boolean(e)), [selected, entryByPath]);
  const single = selectedEntries.length === 1 ? selectedEntries[0] : null;
  const rowOrder = useMemo(() => (view === "list" ? treeRows.map((r) => r.entry.path) : visible.map((e) => e.path)), [view, treeRows, visible]);

  function select(clicked: string, mods: SelectMods) {
    const next = clickSelect(rowOrder, selected, anchor, clicked, mods);
    setSelected(next.selected);
    setAnchor(next.anchor);
  }

  function clearSelection() {
    setSelected([]);
    setAnchor(null);
  }

  function toggleExpanded(dir: string, open?: boolean) {
    setExpanded((x) => {
      const next = new Set(x);
      const want = open ?? !next.has(dir);
      if (want) next.add(dir);
      else next.delete(dir);
      return next;
    });
  }

  function open(entry: FileEntry) {
    if (entry.is_dir) navigate(entry.path);
    else openFile(entry);
  }

  /**
   * Enter, double-click, Space and the preview's button: a tab for anything
   * the app can show, the desktop's app for the rest, and the browser for a
   * Google Doc.
   */
  function openFile(entry: FileEntry) {
    if (entry.is_dir) return;
    const kind = viewerKind(entry);
    if (kind === null || kind === "other") void run(entry.path, () => fmOpen(entry.path));
    else updateTabs((s) => openTab(s, { path: entry.path, name: entry.name }));
  }

  async function closeFileTab(target: string) {
    const file = tabs.files.find((f) => f.path === target);
    if (!file) return;
    if (tabs.dirty.includes(target)) {
      const ok = await confirmAction(`Your changes to ${file.name} have not been saved.`, {
        title: "Close without saving?",
        confirmLabel: "Close without saving",
        danger: true,
      });
      if (!ok) return;
    }
    drafts.delete(target);
    updateTabs((s) => closeTab(s, target));
  }

  /** "Show in folder" from a file tab: the folder view, on that file. */
  function showInFolder(target: string) {
    updateTabs((s) => ({ ...s, active: null }));
    navigate(parentPath(target), target);
  }

  /** Closes the tabs on or under `roots`, asking first when any holds unsaved edits. */
  async function closeTabsFor(roots: string[], why: string): Promise<boolean> {
    const under = (p: string) => roots.some((r) => p === r || p.startsWith(`${r}/`));
    const unsaved = tabs.dirty.filter(under);
    if (unsaved.length) {
      const ok = await confirmAction(`${unsaved.length === 1 ? "An open file has" : `${unsaved.length} open files have`} unsaved changes, which will be lost.`, {
        title: why,
        confirmLabel: "Discard changes",
        danger: true,
      });
      if (!ok) return false;
    }
    for (const p of [...drafts.keys()]) if (under(p)) drafts.delete(p);
    updateTabs((s) => closeTabsUnder(s, roots));
    return true;
  }

  /** A file or folder was renamed or moved: its tabs follow it. */
  function followMove(from: string, to: string, name?: string) {
    retargetDrafts(from, to);
    updateTabs((s) => retargetTabs(s, from, to, name));
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

  async function handleTrash(targets: FileEntry[]) {
    if (targets.length === 0) return;
    const what = targets.length === 1 ? `"${targets[0].name}"` : `${targets.length} items`;
    const ok = await confirmAction(`${what} will be moved to the trash. You can restore it from your desktop's trash.`, {
      title: targets.length === 1 ? (targets[0].is_dir ? "Move folder to trash?" : "Move file to trash?") : "Move items to trash?",
      confirmLabel: "Move to trash",
      danger: true,
    });
    if (!ok) return;
    if (!(await closeTabsFor(targets.map((t) => t.path), "Move to trash?"))) return;
    clearSelection();
    await run("trash", async () => {
      const failed: string[] = [];
      for (const t of targets) {
        try {
          await fmTrash(t.path);
        } catch (e) {
          failed.push(`${t.name}: ${errorMessage(e)}`);
        }
      }
      if (failed.length) throw new Error(failed.join("\n"));
      return targets.length === 1 ? `Moved ${targets[0].name} to the trash` : `Moved ${targets.length} items to the trash`;
    }, true);
  }

  async function moveTo(paths: string[], dir: string) {
    const { moves, skipped } = planMove(paths, dir);
    if (moves.length === 0) return;
    clearSelection();
    await run("move", async () => {
      const failed: string[] = [];
      for (const p of moves) {
        try {
          followMove(p, await fmMove(p, dir));
        } catch (e) {
          failed.push(`${p.split("/").pop()}: ${errorMessage(e)}`);
        }
      }
      if (failed.length) throw new Error(failed.join("\n"));
      const n = moves.length;
      const note = skipped.length ? ` (${skipped.length} already there or skipped)` : "";
      return `Moved ${n === 1 ? moves[0].split("/").pop() : `${n} items`} to ${tildePath(dir, home)}${note}`;
    }, true);
  }

  const dnd: DragHandlers = {
    onDragStart: (e: DragEvent, entry: FileEntry) => {
      const paths = selected.includes(entry.path) ? selected : [entry.path];
      if (!selected.includes(entry.path)) {
        setSelected([entry.path]);
        setAnchor(entry.path);
      }
      dragPaths.current = paths;
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", paths.join("\n"));
      setDragging(true);
    },
    onDragEnd: () => {
      setDragging(false);
      setDropOn(null);
      dragPaths.current = [];
    },
    canDrop: (dir: string) => dragging && planMove(dragPaths.current, dir).moves.length > 0,
    onDrop: (dir: string) => {
      const paths = dragPaths.current;
      dragPaths.current = [];
      setDragging(false);
      setDropOn(null);
      void moveTo(paths, dir);
    },
  };

  /** Drop-target props for a folder row or tile in the list and grid views. */
  function dropProps(dir: string) {
    return {
      onDragOver: (e: DragEvent) => {
        if (!dnd.canDrop(dir)) return;
        e.preventDefault();
        e.stopPropagation();
        e.dataTransfer.dropEffect = "move";
        if (dropOn !== dir) setDropOn(dir);
      },
      onDragLeave: (e: DragEvent) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node)) setDropOn((d) => (d === dir ? null : d));
      },
      onDrop: (e: DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        dnd.onDrop(dir);
      },
    };
  }

  async function handleNameSubmit(value: string) {
    if (path === null || !dialog || dialog.kind === "props") return;
    const current = dialog;
    setDialog(null);
    await run(
      "name",
      async () => {
        if (current.kind === "mkdir") return fmMkdir(path, value);
        if (current.kind === "newfile") return fmCreateFile(path, value);
        const renamed = await fmRename(current.entry.path, value);
        followMove(current.entry.path, renamed, value.trim());
        return renamed;
      },
      true,
    );
  }

  function handleKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    const target = e.target as HTMLElement;
    if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;
    const focusPath = selected[selected.length - 1];
    const index = focusPath ? rowOrder.indexOf(focusPath) : -1;
    if (e.key === "Backspace" && path && path !== "/") {
      e.preventDefault();
      navigate(parentPath(path));
    } else if (e.key === "Enter" && single) {
      e.preventDefault();
      open(single);
    } else if (e.key === "Delete" && selectedEntries.length) {
      e.preventDefault();
      void handleTrash(selectedEntries);
    } else if (e.key === " " && single) {
      e.preventDefault();
      openFile(single);
    } else if ((e.key === "a" || e.key === "A") && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      setSelected(rowOrder);
    } else if (e.key === "Escape") {
      clearSelection();
    } else if ((e.key === "ArrowDown" || e.key === "ArrowUp") && view !== "columns") {
      e.preventDefault();
      if (rowOrder.length === 0) return;
      const delta = e.key === "ArrowDown" ? 1 : -1;
      const next = index < 0 ? (delta > 0 ? 0 : rowOrder.length - 1) : Math.max(0, Math.min(rowOrder.length - 1, index + delta));
      select(rowOrder[next], { shift: e.shiftKey });
    } else if (e.key === "ArrowRight" && view === "list" && single?.is_dir) {
      e.preventDefault();
      toggleExpanded(single.path, true);
    } else if (e.key === "ArrowLeft" && view === "list" && single) {
      e.preventDefault();
      if (single.is_dir && expanded.has(single.path)) toggleExpanded(single.path, false);
      else {
        // Folded already, or a file: jump to the enclosing folder's row.
        const parent = parentPath(single.path);
        if (rowOrder.includes(parent)) select(parent, {});
      }
    }
  }

  /**
   * "Open in …" for a folder (or a file's folder for the terminals): the
   * in-app terminal, the desktop's terminal, then every code editor found on
   * PATH, with the editor configured in Settings when it is not one of them.
   */
  function openInItems(entry: FileEntry): MenuItem[] {
    const dir = entry.is_dir ? entry.path : parentPath(entry.path);
    const installed = editors.data ?? [];
    const items: MenuItem[] = [
      {
        key: "term",
        label: "Open in Terminal",
        icon: Terminal,
        separatorBefore: true,
        onClick: () => routerNavigate(`/terminal?cwd=${encodeURIComponent(dir)}`),
      },
      { key: "systerm", label: "Open in system terminal", icon: SquareArrowOutUpRight, onClick: () => void run(dir, () => fmOpenTerminal(dir)) },
      ...installed.map((app: EditorApp) => ({
        key: `editor-${app.id}`,
        label: `Open in ${app.label}`,
        icon: Code,
        onClick: () => void run(entry.path, () => fmOpenInEditor(entry.path, app.command)),
      })),
      {
        key: "editor-configured",
        label: "Open in configured editor",
        icon: Code,
        onClick: () => void run(entry.path, async () => fmOpenInEditor(entry.path, await getExternalEditor())),
      },
    ];
    return items;
  }

  /** The verbs for the current selection, or for one entry when it is not part of it. */
  function actionsFor(targets: FileEntry[]): MenuItem[] {
    const items: MenuItem[] = [];
    const one = targets.length === 1 ? targets[0] : null;
    if (one) {
      if (one.is_dir) {
        items.push({ key: "open", label: "Open", icon: Folder, onClick: () => open(one) });
        items.push(favouriteItem(one.path));
        if (view === "list") {
          const isOpen = expanded.has(one.path);
          items.push({ key: "fold", label: isOpen ? "Fold" : "Unfold", icon: isOpen ? ChevronDown : ChevronRight, onClick: () => toggleExpanded(one.path) });
        }
      } else if (one.link) {
        items.push({ key: "open", label: "Open in browser", icon: ExternalLink, onClick: () => void run(one.path, () => fmOpen(one.path)) });
      } else {
        const kind = viewerKind(one);
        if (kind !== null && kind !== "other") items.push({ key: "tab", label: "Open", icon: Eye, onClick: () => openFile(one) });
        items.push({ key: "open", label: "Open with default app", icon: SquareArrowOutUpRight, onClick: () => void run(one.path, () => fmOpen(one.path)) });
      }
      items.push({ key: "reveal", label: "Reveal in system file manager", icon: HardDrive, onClick: () => void run(one.path, () => revealItemInDir(one.path)) });
      items.push({ key: "copy", label: "Copy path", icon: FileText, onClick: () => void navigator.clipboard.writeText(one.path) });
      items.push(...openInItems(one));
      items.push({ key: "rename", label: "Rename", icon: Pencil, separatorBefore: true, onClick: () => setDialog({ kind: "rename", entry: one }) });
    } else {
      items.push({
        key: "copy",
        label: `Copy ${targets.length} paths`,
        icon: FileText,
        onClick: () => void navigator.clipboard.writeText(targets.map((t) => t.path).join("\n")),
      });
    }
    items.push({ key: "props", label: "Properties", icon: Info, onClick: () => setDialog({ kind: "props", entries: targets }) });
    items.push({
      key: "trash",
      label: one ? "Move to trash" : `Move ${targets.length} items to trash`,
      icon: Trash2,
      danger: true,
      separatorBefore: true,
      onClick: () => void handleTrash(targets),
    });
    return items;
  }

  function favouriteItem(dir: string): MenuItem {
    return favourites.has(dir)
      ? { key: "fav", label: "Remove from Favourites", icon: StarOff, onClick: () => favourites.remove(dir) }
      : { key: "fav", label: "Add to Favourites", icon: Star, onClick: () => favourites.add(dir) };
  }

  /** Right-click on empty space: the folder's own verbs. */
  function folderActions(dir: string): MenuItem[] {
    const here = dir === path;
    const folderEntry = entryByPath.get(dir);
    const asEntry: FileEntry = folderEntry ?? {
      name: dir.split("/").pop() || "/",
      path: dir,
      is_dir: true,
      is_symlink: false,
      is_hidden: false,
      size: 0,
      modified: 0,
      permissions: "",
    };
    return [
      { key: "mkdir", label: "New folder", icon: FolderPlus, disabled: !here, onClick: () => setDialog({ kind: "mkdir" }) },
      { key: "newfile", label: "New file", icon: FilePlus, disabled: !here, onClick: () => setDialog({ kind: "newfile" }) },
      favouriteItem(dir),
      { key: "all", label: "Select all", separatorBefore: true, disabled: !here, onClick: () => setSelected(rowOrder) },
      { key: "refresh", label: "Refresh", icon: RefreshCw, onClick: () => path && void load(path) },
      { key: "reveal", label: "Reveal in system file manager", icon: HardDrive, onClick: () => void run(dir, () => revealItemInDir(dir)) },
      ...openInItems(asEntry),
      { key: "props", label: "Properties", icon: Info, separatorBefore: true, onClick: () => setDialog({ kind: "props", entries: [asEntry] }) },
    ];
  }

  function openEntryMenu(e: MouseEvent, entry: FileEntry) {
    e.preventDefault();
    e.stopPropagation();
    // Right-clicking outside the selection retargets it, as every file manager does.
    let targets = selectedEntries;
    if (!selected.includes(entry.path)) {
      setSelected([entry.path]);
      setAnchor(entry.path);
      targets = [entry];
    }
    setMenu({ x: e.clientX, y: e.clientY, items: actionsFor(targets) });
  }

  function openFolderMenu(e: MouseEvent, dir: string) {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, items: folderActions(dir) });
  }

  // Rows arrive pre-sorted with folders first; the constant sortValue keeps
  // the headers clickable without letting the table re-sort on its own.
  const columns: Column<TreeRow>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      sortValue: () => 0,
      render: ({ entry: e, depth, expanded: isOpen, pending }) => (
        <span className="fm-name" style={{ paddingLeft: depth * TREE_INDENT }}>
          {e.is_dir ? (
            <button
              type="button"
              className="fm-tree-toggle"
              aria-label={isOpen ? `Fold ${e.name}` : `Unfold ${e.name}`}
              aria-expanded={isOpen}
              onClick={(ev) => {
                ev.stopPropagation();
                toggleExpanded(e.path);
              }}
              onDoubleClick={(ev) => ev.stopPropagation()}
            >
              <Icon icon={isOpen ? ChevronDown : ChevronRight} size="xs" className={pending ? "fm-tree-pending" : undefined} />
            </button>
          ) : (
            <span className="fm-tree-toggle fm-tree-toggle-blank" />
          )}
          <Icon icon={entryIcon(e)} size="sm" className={e.is_dir ? "fm-col-icon-dir" : "fm-col-icon"} />
          <span className={e.is_hidden ? "fm-name-text u-muted" : "fm-name-text"}>{e.name}</span>
          {e.is_symlink && <span className="u-muted">→</span>}
        </span>
      ),
    },
    { key: "modified", header: "Date modified", priority: 2, sortValue: () => 0, render: ({ entry: e }) => formatModified(e.modified) },
    { key: "size", header: "Size", priority: 2, align: "end", sortValue: () => 0, render: ({ entry: e }) => (e.is_dir ? "—" : formatSize(e.size)) },
    { key: "kind", header: "Kind", priority: 3, sortValue: () => 0, render: ({ entry: e }) => <span className="u-secondary">{fileKind(e)}</span> },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: ({ entry: e }) => (
        <ActionGroup>
          <Menu items={actionsFor(selected.includes(e.path) ? selectedEntries : [e])} size="sm" />
        </ActionGroup>
      ),
    },
  ];

  const crumbs = breadcrumb(path ?? "/", devices.list, titles);
  const here = path !== null ? deviceAt(path, devices.list) : null;
  const placeItems = (places.data?.places ?? []).map((p) => ({
    key: p.path,
    label: p.label,
    icon: <Icon icon={PLACE_ICON[p.kind] ?? Folder} size="sm" />,
    active: path === p.path && tabs.active === null,
    onSelect: () => {
      updateTabs((s) => ({ ...s, active: null }));
      navigate(p.path);
    },
    attrs: { ...dropProps(p.path), "data-drop": dropOn === p.path ? "true" : undefined },
  }));
  const favouriteItems = favourites.list.map((f) => ({
    key: `fav:${f.path}`,
    label: f.label,
    icon: <Icon icon={Star} size="sm" />,
    active: path === f.path && tabs.active === null,
    onSelect: () => {
      updateTabs((s) => ({ ...s, active: null }));
      navigate(f.path);
    },
    attrs: {
      ...dropProps(f.path),
      "data-drop": dropOn === f.path ? "true" : undefined,
      onContextMenu: (e: MouseEvent) => {
        e.preventDefault();
        setMenu({
          x: e.clientX,
          y: e.clientY,
          items: [
            { key: "open", label: "Open", icon: Folder, onClick: () => navigate(f.path) },
            { key: "term", label: "Open in Terminal", icon: Terminal, onClick: () => routerNavigate(`/terminal?cwd=${encodeURIComponent(f.path)}`) },
            { key: "remove", label: "Remove from Favourites", icon: StarOff, danger: true, separatorBefore: true, onClick: () => favourites.remove(f.path) },
          ],
        });
      },
    },
  }));
  const inFavourites = path !== null && favourites.has(path);

  /** A click on a device: open it, mounting it first when it is not. */
  async function openDevice(d: Device) {
    if (d.path) {
      updateTabs((s) => ({ ...s, active: null }));
      navigate(d.path);
      return;
    }
    if (!d.mount || deviceBusy) return;
    setDeviceBusy(d.id);
    setError(null);
    try {
      await fmDeviceAction("mount", d.mount);
      const now = (await devices.refresh())?.find((x) => x.id === d.id);
      if (now?.path) {
        updateTabs((s) => ({ ...s, active: null }));
        navigate(now.path);
      } else setNotice(`Mounted ${d.label}`);
    } catch (e) {
      const hint = d.kind === "phone" ? " Unlock the phone and choose File transfer in its USB notification, then try again." : "";
      setError(`Could not open ${d.label}: ${errorMessage(e)}${hint}`);
    } finally {
      setDeviceBusy(null);
    }
  }

  /** Unmount, or for a drive that can be powered down, make it safe to unplug. */
  async function releaseDevice(d: Device, how: "unmount" | "eject") {
    const target = how === "eject" ? d.eject : d.unmount;
    if (!target || !d.path || deviceBusy) return;
    if (!(await closeTabsFor([d.path], `${how === "eject" ? "Eject" : "Unmount"} ${d.label}?`))) return;
    // An open listing keeps the device busy; step off it first.
    if (path !== null && (path === d.path || path.startsWith(`${d.path}/`))) navigate(home);
    setDeviceBusy(d.id);
    setError(null);
    try {
      await fmDeviceAction(how, target);
      setNotice(how === "eject" ? `${d.label} can be unplugged now` : `Unmounted ${d.label}`);
    } catch (e) {
      setError(`Could not ${how} ${d.label}: ${errorMessage(e)}`);
    } finally {
      setDeviceBusy(null);
      void devices.refresh();
    }
  }

  function deviceItem(d: Device) {
    const release: "eject" | "unmount" | null = d.path ? (d.eject ? "eject" : d.unmount ? "unmount" : null) : null;
    return {
      key: `dev:${d.id}`,
      label: d.label,
      icon: <Icon icon={DEVICE_ICON[d.kind] ?? HardDrive} size="sm" />,
      meta: deviceBusy === d.id ? (d.path ? "Ejecting…" : "Mounting…") : undefined,
      muted: !d.path,
      active: here?.id === d.id && tabs.active === null,
      onSelect: () => void openDevice(d),
      action: release
        ? {
            icon: Eject,
            label: release === "eject" ? `Eject ${d.label} so it can be unplugged` : `Unmount ${d.label}`,
            disabled: deviceBusy !== null,
            onClick: () => void releaseDevice(d, release),
          }
        : undefined,
      attrs: {
        ...(d.path ? dropProps(d.path) : {}),
        "data-drop": d.path && dropOn === d.path ? "true" : undefined,
        title: d.path ? undefined : `${d.label} is not mounted. Click to mount it.`,
        onContextMenu: (e: MouseEvent) => {
          e.preventDefault();
          const items: MenuItem[] = [{ key: "open", label: d.path ? "Open" : "Mount and open", icon: Folder, onClick: () => void openDevice(d) }];
          if (d.path) {
            const dir = d.path;
            items.push({ key: "term", label: "Open in Terminal", icon: Terminal, onClick: () => routerNavigate(`/terminal?cwd=${encodeURIComponent(dir)}`) });
            items.push(favouriteItem(dir));
          }
          if (d.path && d.unmount) items.push({ key: "unmount", label: "Unmount", icon: Eject, separatorBefore: true, onClick: () => void releaseDevice(d, "unmount") });
          if (d.path && d.eject) items.push({ key: "eject", label: "Eject / safely remove", icon: Eject, separatorBefore: !d.unmount, onClick: () => void releaseDevice(d, "eject") });
          setMenu({ x: e.clientX, y: e.clientY, items });
        },
      },
    };
  }
  const localDevices = devices.list.filter((d) => !REMOTE_KINDS.has(d.kind)).map(deviceItem);
  const remoteDevices = devices.list.filter((d) => REMOTE_KINDS.has(d.kind)).map(deviceItem);

  const rowClass = (row: TreeRow) =>
    [selected.includes(row.entry.path) && "fm-row-selected", row.entry.is_dir && dropOn === row.entry.path && "fm-drop-target"].filter(Boolean).join(" ") || undefined;

  const selectionLabel =
    selectedEntries.length > 1
      ? `${selectedEntries.length} selected`
      : single
        ? `${single.name}${single.is_dir ? "" : `, ${formatSize(single.size)}`}`
        : "";
  const shownPath = single?.path ?? path ?? "";

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(places)}
      fill
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
      <Section padding="none" fill className="fm">
        <SplitView
          asideLabel="Places"
          resizable
          storageKey="fm.aside-width"
          aside={
            <>
              <SideList title="Favourites" items={favouriteItems} />
              {favouriteItems.length === 0 && <p className="fm-aside-hint u-muted">Right-click a folder and choose Add to Favourites.</p>}
              <SideList title="Places" items={placeItems} />
              {localDevices.length > 0 && <SideList title="Devices" items={localDevices} />}
              {remoteDevices.length > 0 && <SideList title="Cloud & network" items={remoteDevices} />}
            </>
          }
        >
          <div className="fm-main">
            {tabs.files.length > 0 && (
              <FileTabStrip
                files={tabs.files}
                active={tabs.active}
                dirty={tabs.dirty}
                describe={describe}
                folderLabel={crumbs[crumbs.length - 1]?.label ?? "Files"}
                onActivate={(target) => updateTabs((s) => ({ ...s, active: target }))}
                onClose={(target) => void closeFileTab(target)}
                onReorder={(target, before) => updateTabs((s) => moveTab(s, target, before))}
              />
            )}
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
            <div ref={folderRef} tabIndex={-1} className={tabs.active === null ? "fm-folder" : "fm-folder fm-hidden"} onKeyDown={handleKeyDown}>
              <div className="fm-toolbar">
                <ActionGroup>
                  <IconButton icon={ChevronLeft} label="Back" disabled={!canGoBack(history)} onClick={() => setHistory((h) => historyStep(h, -1))} />
                  <IconButton icon={ChevronRight} label="Forward" disabled={!canGoForward(history)} onClick={() => setHistory((h) => historyStep(h, 1))} />
                  <IconButton icon={ArrowUp} label="Enclosing folder" disabled={!path || path === "/"} onClick={() => path && navigate(parentPath(path))} />
                </ActionGroup>
                <nav className="fm-crumbs" aria-label="Current folder">
                  {crumbs.map((crumb, i) => (
                    <span key={crumb.path} className={dropOn === crumb.path ? "fm-crumb fm-drop-target" : "fm-crumb"} {...dropProps(crumb.path)}>
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
                      { value: "columns", label: "Columns", icon: Columns3 },
                    ]}
                  />
                  {view === "grid" && (
                    <span className="fm-thumb-size">
                      <input
                        type="range"
                        className="fm-thumb-slider"
                        min={THUMB_MIN}
                        max={THUMB_MAX}
                        step={8}
                        value={thumbSize}
                        onChange={(e) => setThumbSize(Number(e.target.value))}
                        aria-label="Icon size"
                        title={`Icon size: ${thumbSize}px (Ctrl+wheel over the icons also resizes)`}
                      />
                      <Menu
                        icon={SlidersHorizontal}
                        label="Icon size presets"
                        items={THUMB_PRESETS.map((p) => ({
                          key: `thumb-${p.size}`,
                          label: p.label,
                          icon: thumbSize === p.size ? Check : undefined,
                          onClick: () => setThumbSize(p.size),
                        }))}
                      />
                    </span>
                  )}
                  <IconButton
                    icon={inFavourites ? StarOff : Star}
                    label={inFavourites ? "Remove this folder from Favourites" : "Add this folder to Favourites"}
                    disabled={path === null}
                    onClick={() => path && (inFavourites ? favourites.remove(path) : favourites.add(path))}
                  />
                  <IconButton
                    icon={showHidden ? EyeOff : Eye}
                    label={showHidden ? "Hide hidden files" : "Show hidden files"}
                    onClick={() => setShowHidden((v) => !v)}
                  />
                  <IconButton icon={RefreshCw} label="Refresh" loading={loading} onClick={() => path && load(path)} />
                </div>
              </div>

              <div
                ref={bodyRef}
                data-own-wheel-zoom={view === "grid" ? "" : undefined}
                className={dropOn === path && view !== "columns" ? "fm-body fm-drop-target" : "fm-body"}
                onContextMenu={(e) => path && view !== "columns" && openFolderMenu(e, path)}
                {...(path && view !== "columns" ? dropProps(path) : {})}
              >
                {view === "columns" && path !== null ? (
                  <ColumnView
                    path={path}
                    entries={entries}
                    loading={loading}
                    placePaths={placePaths}
                    selected={selected}
                    query={query}
                    showHidden={showHidden}
                    sort={sort}
                    reloadToken={reloadToken}
                    onSelect={(dir, entry, mods) => {
                      if (dir === path && entry && (mods.ctrl || mods.shift)) select(entry, mods);
                      else navigate(dir, entry, mods);
                    }}
                    onOpen={open}
                    onQuickLook={openFile}
                    onContextMenu={(e, dir, entry) => (entry ? openEntryMenu(e, entry) : openFolderMenu(e, dir))}
                    dnd={dnd}
                    preview={
                      single && !single.is_dir ? (
                        <FilePreview
                          entry={single}
                          shownPath={describe(single.path)}
                          onOpen={openFile}
                          onReveal={(e) => void run(e.path, () => revealItemInDir(e.path))}
                        />
                      ) : null
                    }
                  />
                ) : view === "list" ? (
                  <DataTable
                    columns={columns}
                    rows={treeRows}
                    rowKey={(r) => r.entry.path}
                    sort={sort}
                    onSortChange={setSort}
                    loading={loading && entries.length === 0}
                    onRowClick={(r) => select(r.entry.path, {})}
                    onRowDoubleClick={(r) => open(r.entry)}
                    rowClassName={rowClass}
                    rowProps={(r) => ({
                      draggable: true,
                      // Replaces the table's plain onRowClick (kept above for the
                      // clickable styling) so ctrl and shift reach the selection.
                      onClick: (e) => select(r.entry.path, modsOf(e)),
                      onContextMenu: (e) => openEntryMenu(e, r.entry),
                      onDragStart: (e) => dnd.onDragStart(e, r.entry),
                      onDragEnd: dnd.onDragEnd,
                      ...(r.entry.is_dir ? dropProps(r.entry.path) : {}),
                    })}
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
                  <ul className="fm-grid" aria-label={`Contents of ${path ?? ""}`} style={{ "--fm-thumb": `${thumbSize}px` } as CSSProperties}>
                    {visible.map((e) => (
                      <li key={e.path}>
                        <button
                          type="button"
                          className={[
                            "fm-tile",
                            selected.includes(e.path) && "fm-tile-selected",
                            e.is_dir && dropOn === e.path && "fm-drop-target",
                          ]
                            .filter(Boolean)
                            .join(" ")}
                          aria-pressed={selected.includes(e.path)}
                          draggable
                          onClick={(ev) => select(e.path, modsOf(ev))}
                          onDoubleClick={() => open(e)}
                          onContextMenu={(ev) => openEntryMenu(ev, e)}
                          onDragStart={(ev) => dnd.onDragStart(ev, e)}
                          onDragEnd={dnd.onDragEnd}
                          {...(e.is_dir ? dropProps(e.path) : {})}
                          title={e.name}
                        >
                          <Thumbnail entry={e} size={thumbRender} className="fm-tile-icon" />
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
                  {selectionLabel && ` · ${selectionLabel}`}
                </span>
                <span className="fm-status-path">
                  <Mono truncate title={shownPath}>
                    {describe(shownPath)}
                  </Mono>
                  <CopyButton value={shownPath} label="Copy path" size="sm" />
                </span>
              </div>
            </div>
            {tabs.files.map((f) => (
              <FileTab
                key={f.path}
                file={f}
                active={tabs.active === f.path}
                shownPath={describe(f.path)}
                onDirty={(d) => updateTabs((s) => setDirty(s, f.path, d))}
                onSaved={() => {
                  if (path !== null && parentPath(f.path) === path) void load(path);
                }}
                onReveal={() => showInFolder(f.path)}
                onOpenExternal={() => void run(f.path, () => fmOpen(f.path))}
              />
            ))}
          </div>
        </SplitView>
      </Section>

      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
      {dialog && (dialog.kind === "mkdir" || dialog.kind === "newfile" || dialog.kind === "rename") && (
        <NameDialog
          title={dialog.kind === "mkdir" ? "New folder" : dialog.kind === "newfile" ? "New file" : `Rename ${dialog.entry.name}`}
          initial={dialog.kind === "rename" ? dialog.entry.name : ""}
          confirmLabel={dialog.kind === "rename" ? "Rename" : "Create"}
          onSubmit={handleNameSubmit}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.kind === "props" && <PropertiesModal entries={dialog.entries} home={home} onClose={() => setDialog(null)} />}
      {busy && <span className="u-sr-only">Working on {busy}</span>}
    </Page>
  );
}
