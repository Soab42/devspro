import { DragEvent, KeyboardEvent, MouseEvent, PointerEvent as ReactPointerEvent, ReactNode, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Icon, SortState, Spinner } from "../../../ui";
import { ChevronRight } from "../../../ui/icons";
import { FileEntry, fmList } from "../api";
import { entryIcon } from "../entryIcon";
import type { GitMarkFor } from "../gitStatus";
import GitBadge from "./GitBadge";
import { EntrySortKey, columnChain, errorMessage, filterEntries, sortEntries } from "../utils";

export interface ColumnViewProps {
  /** The deepest open directory — what the page's history points at. */
  path: string;
  /** The page's fresh listing of `path`; the last directory column shows it. */
  entries: FileEntry[];
  loading: boolean;
  placePaths: string[];
  /** Paths of the selected entries inside `path`. */
  selected: string[];
  query: string;
  showHidden: boolean;
  sort: SortState;
  /** Bumped whenever the page reloads after a change, so cached ancestors refetch. */
  reloadToken: number;
  /** Open `dir` (pushing history) and select `entry` inside it, honouring ctrl/shift. */
  onSelect: (dir: string, entry: string | null, mods: SelectMods) => void;
  onOpen: (entry: FileEntry) => void;
  onQuickLook: (entry: FileEntry) => void;
  /** Right-click on an entry, or on a column's empty space (`entry` null). */
  onContextMenu: (e: MouseEvent, dir: string, entry: FileEntry | null) => void;
  dnd: DragHandlers;
  /** What the rightmost pane shows when a file is selected. */
  preview: ReactNode;
  /** Git letters and folder dots for the rows. */
  gitMark?: GitMarkFor;
}

export interface SelectMods {
  ctrl?: boolean;
  shift?: boolean;
}

/** The page owns what is being dragged; views only mark where it can land. */
export interface DragHandlers {
  onDragStart: (e: DragEvent, entry: FileEntry) => void;
  onDragEnd: () => void;
  /** True when the drag can land in `dir` — used to light the target. */
  canDrop: (dir: string) => boolean;
  onDrop: (dir: string) => void;
}

const DEFAULT_WIDTH = 260;
const MIN_WIDTH = 160;
const ROW_HEIGHT = 26;
const OVERSCAN = 8;

/**
 * Miller columns: every directory from the nearest place down to `path` is a
 * column, the item leading to the next column stays highlighted, and a
 * selected file turns the rightmost pane into its preview card.
 *
 * The page keeps owning the deepest listing and the history; this component
 * only remembers the listings it has already seen (`cache`) so stepping back
 * up a chain never refetches, and fetches ancestors it has not seen when the
 * page starts deep (a popped-out window's `?path=`).
 */
export default function ColumnView({
  path,
  entries,
  loading,
  placePaths,
  selected,
  query,
  showHidden,
  sort,
  reloadToken,
  onSelect,
  onOpen,
  onQuickLook,
  onContextMenu,
  dnd,
  preview,
  gitMark,
}: ColumnViewProps) {
  const chain = useMemo(() => columnChain(path, placePaths), [path, placePaths]);
  const last = chain.length - 1;

  const [cache, setCache] = useState<Record<string, FileEntry[]>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const inflight = useRef(new Set<string>());
  const [widths, setWidths] = useState<Record<number, number>>({});
  const [focusCol, setFocusCol] = useState(last);

  const scrollerRef = useRef<HTMLDivElement>(null);
  const columnRefs = useRef<(HTMLDivElement | null)[]>([]);

  // The page's listing of `path` is always the freshest, so it feeds the cache
  // rather than the other way round; a refresh or rename lands here too.
  useEffect(() => {
    if (loading) return;
    setCache((c) => (c[path] === entries ? c : { ...c, [path]: entries }));
  }, [path, entries, loading]);

  // A rename, move or refresh may have changed any column, not just the
  // deepest, so forget everything and let the effects below refill.
  useEffect(() => {
    if (reloadToken > 0) setCache({});
  }, [reloadToken]);

  // Ancestors the cache has not seen yet: fetched once each.
  useEffect(() => {
    for (const dir of chain.slice(0, last)) {
      if (cache[dir] || inflight.current.has(dir)) continue;
      inflight.current.add(dir);
      fmList(dir)
        .then((list) => setCache((c) => ({ ...c, [dir]: list })))
        .catch((e) => setErrors((err) => ({ ...err, [dir]: errorMessage(e) })))
        .finally(() => inflight.current.delete(dir));
    }
  }, [chain, last, cache]);

  const rowsFor = useCallback(
    (i: number): FileEntry[] | null => {
      const dir = chain[i];
      const source = i === last ? entries : cache[dir];
      if (!source) return null;
      // The filter box only narrows the folder being looked at; ancestors keep
      // every entry, and the one leading onwards stays even when hidden.
      const rows = sortEntries(filterEntries(source, i === last ? query : "", showHidden), sort.key as EntrySortKey, sort.dir);
      if (i < last) {
        const next = chain[i + 1];
        if (!rows.some((e) => e.path === next)) {
          const hidden = source.find((e) => e.path === next);
          if (hidden) return sortEntries([...rows, hidden], sort.key as EntrySortKey, sort.dir);
        }
      }
      return rows;
    },
    [chain, last, entries, cache, query, showHidden, sort],
  );

  const selectedIn = useCallback((i: number): string[] => (i < last ? [chain[i + 1]] : selected), [chain, last, selected]);

  const pick = useCallback(
    (i: number, entry: FileEntry, mods: SelectMods = {}) => {
      setFocusCol(i);
      // Multi-select only makes sense in the deepest column: an ancestor's
      // selection is the folder that is open to its right.
      if (i === last && (mods.ctrl || mods.shift)) onSelect(chain[i], entry.path, mods);
      else if (entry.is_dir) onSelect(entry.path, null, {});
      else onSelect(chain[i], entry.path, {});
    },
    [chain, last, onSelect],
  );

  // A new column (or the preview) opening off the right edge slides into view.
  const paneCount = chain.length + (preview ? 1 : 0);
  useEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    el.scrollTo({ left: el.scrollWidth, behavior: "smooth" });
  }, [paneCount]);

  useEffect(() => {
    setFocusCol((f) => Math.min(f, last));
  }, [last]);

  function focusColumn(i: number) {
    setFocusCol(i);
    columnRefs.current[i]?.focus();
  }

  function handleKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    const col = Math.min(focusCol, last);
    const rows = rowsFor(col) ?? [];
    const current = selectedIn(col);
    const index = rows.findIndex((r) => r.path === current[current.length - 1]);
    const sel = index >= 0 ? rows[index] : null;

    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      if (rows.length === 0) return;
      const delta = e.key === "ArrowDown" ? 1 : -1;
      const next = index < 0 ? (delta > 0 ? 0 : rows.length - 1) : Math.max(0, Math.min(rows.length - 1, index + delta));
      pick(col, rows[next]);
      columnRefs.current[col]?.focus();
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      e.stopPropagation();
      if (!sel?.is_dir || col + 1 > last) return;
      const childRows = rowsFor(col + 1) ?? [];
      if (selectedIn(col + 1).length === 0 && childRows.length > 0) pick(col + 1, childRows[0]);
      focusColumn(col + 1);
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      e.stopPropagation();
      if (col > 0) focusColumn(col - 1);
    } else if (e.key === " ") {
      e.preventDefault();
      e.stopPropagation();
      if (sel) onQuickLook(sel);
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      if (!sel) return;
      if (sel.is_dir) focusColumn(Math.min(col + 1, last));
      else onOpen(sel);
    }
  }

  return (
    <div className="fm-columns" ref={scrollerRef} onKeyDown={handleKeyDown}>
      {chain.map((dir, i) => (
        <ColumnPane
          key={dir}
          ref={(el) => {
            columnRefs.current[i] = el;
          }}
          dir={dir}
          rows={rowsFor(i)}
          error={errors[dir] ?? null}
          loading={i === last && loading && entries.length === 0}
          selected={selectedIn(i)}
          leadsOn={i < last}
          focused={i === Math.min(focusCol, last)}
          width={widths[i] ?? DEFAULT_WIDTH}
          onWidth={(w) => setWidths((ws) => ({ ...ws, [i]: w }))}
          onFocus={() => setFocusCol(i)}
          onPick={(entry, mods) => pick(i, entry, mods)}
          onOpen={onOpen}
          onContextMenu={(e, entry) => {
            setFocusCol(i);
            onContextMenu(e, dir, entry);
          }}
          dnd={dnd}
          gitMark={gitMark}
        />
      ))}
      {preview && <div className="fm-col fm-col-preview">{preview}</div>}
    </div>
  );
}

interface ColumnPaneProps {
  dir: string;
  rows: FileEntry[] | null;
  error: string | null;
  loading: boolean;
  selected: string[];
  /** An ancestor column: its selection is the folder open to the right. */
  leadsOn: boolean;
  focused: boolean;
  width: number;
  onWidth: (w: number) => void;
  onFocus: () => void;
  onPick: (entry: FileEntry, mods: SelectMods) => void;
  onOpen: (entry: FileEntry) => void;
  onContextMenu: (e: MouseEvent, entry: FileEntry | null) => void;
  dnd: DragHandlers;
  gitMark?: GitMarkFor;
  ref: (el: HTMLDivElement | null) => void;
}

/** One directory listing, virtualised so a node_modules column scrolls like any other. */
function ColumnPane({ dir, rows, error, loading, selected, leadsOn, focused, width, onWidth, onFocus, onPick, onOpen, onContextMenu, dnd, gitMark, ref }: ColumnPaneProps) {
  const listRef = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(0);
  // Which drop target under the pointer is lit: the pane itself or one folder.
  const [dropOn, setDropOn] = useState<string | null>(null);

  function dragOver(e: DragEvent, target: string) {
    if (!dnd.canDrop(target)) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = "move";
    if (dropOn !== target) setDropOn(target);
  }

  function drop(e: DragEvent, target: string) {
    e.preventDefault();
    e.stopPropagation();
    setDropOn(null);
    dnd.onDrop(target);
  }

  useLayoutEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const measure = () => setHeight(el.clientHeight);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const count = rows?.length ?? 0;
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const end = Math.min(count, Math.ceil((scrollTop + height) / ROW_HEIGHT) + OVERSCAN);

  // Keep the selected row in view when selection moves by keyboard or the
  // chain changes underneath the column.
  const focusPath = selected[selected.length - 1];
  const selectedIndex = rows?.findIndex((r) => r.path === focusPath) ?? -1;
  useEffect(() => {
    const el = listRef.current;
    if (!el || selectedIndex < 0) return;
    const top = selectedIndex * ROW_HEIGHT;
    if (top < el.scrollTop) el.scrollTop = top;
    else if (top + ROW_HEIGHT > el.scrollTop + el.clientHeight) el.scrollTop = top + ROW_HEIGHT - el.clientHeight;
  }, [selectedIndex, dir]);

  function startResize(e: ReactPointerEvent<HTMLDivElement>) {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = width;
    const handle = e.currentTarget;
    handle.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => onWidth(Math.max(MIN_WIDTH, startWidth + ev.clientX - startX));
    const stop = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", stop);
      handle.removeEventListener("pointercancel", stop);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", stop);
    handle.addEventListener("pointercancel", stop);
  }

  const cls = ["fm-col", focused && "fm-col-focused", dropOn === dir && "fm-drop-target"].filter(Boolean).join(" ");
  return (
    <div className={cls} style={{ width }}>
      <div
        ref={(el) => {
          listRef.current = el;
          ref(el);
        }}
        className="fm-col-list"
        role="listbox"
        aria-label={`Contents of ${dir}`}
        tabIndex={0}
        onFocus={onFocus}
        onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
        onContextMenu={(e) => {
          e.preventDefault();
          onContextMenu(e, null);
        }}
        onDragOver={(e) => dragOver(e, dir)}
        onDragLeave={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node)) setDropOn(null);
        }}
        onDrop={(e) => drop(e, dir)}
      >
        {error ? (
          <p className="fm-col-note u-secondary">{error}</p>
        ) : rows === null || loading ? (
          <p className="fm-col-note">
            <Spinner />
          </p>
        ) : rows.length === 0 ? (
          <p className="fm-col-note u-muted">Empty folder</p>
        ) : (
          <div className="fm-col-spacer" style={{ height: count * ROW_HEIGHT }}>
            <div style={{ transform: `translateY(${start * ROW_HEIGHT}px)` }}>
              {rows.slice(start, end).map((e) => {
                const isSelected = selected.includes(e.path);
                const git = gitMark?.(e.path, e.is_dir) ?? null;
                const rowCls = [
                  "fm-col-item",
                  isSelected && (leadsOn && !focused ? "fm-col-item-ancestor" : "fm-col-item-selected"),
                  e.is_hidden && "fm-col-item-hidden",
                  e.is_dir && dropOn === e.path && "fm-drop-target",
                ]
                  .filter(Boolean)
                  .join(" ");
                return (
                  <div
                    key={e.path}
                    role="option"
                    aria-selected={isSelected}
                    className={rowCls}
                    title={e.name}
                    draggable
                    onClick={(ev) => onPick(e, { ctrl: ev.ctrlKey || ev.metaKey, shift: ev.shiftKey })}
                    onDoubleClick={() => onOpen(e)}
                    onContextMenu={(ev) => {
                      ev.preventDefault();
                      ev.stopPropagation();
                      onContextMenu(ev, e);
                    }}
                    onDragStart={(ev) => dnd.onDragStart(ev, e)}
                    onDragEnd={dnd.onDragEnd}
                    onDragOver={e.is_dir ? (ev) => dragOver(ev, e.path) : undefined}
                    onDrop={e.is_dir ? (ev) => drop(ev, e.path) : undefined}
                  >
                    <Icon icon={entryIcon(e)} size="sm" className={e.is_dir ? "fm-col-icon-dir" : "fm-col-icon"} />
                    <span className={git && !git.inside ? `fm-col-item-name fm-git-name fm-git-${git.code}` : "fm-col-item-name"}>{e.name}</span>
                    {e.is_symlink && <span className="u-muted">→</span>}
                    <GitBadge mark={git} />
                    {e.is_dir && <Icon icon={ChevronRight} size="xs" className="fm-col-item-chevron" />}
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </div>
      <div className="fm-col-resizer" role="separator" aria-orientation="vertical" aria-label="Resize column" onPointerDown={startResize} />
    </div>
  );
}
