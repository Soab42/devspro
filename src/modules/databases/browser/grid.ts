import type { Cell } from "../api";
import type { EditableRow } from "./edits";
import { cellValue } from "./edits";

/**
 * Grid state that isn't about editing: which columns are shown, which rows are
 * ticked, and the quick filter over what's already loaded.
 *
 * Kept as pure functions so the rules that are easy to get subtly wrong —
 * "hidden columns must not vanish from a saved row", "select-all means the
 * rows you can see" — are testable without a DOM.
 */

/**
 * The table's columns in the order the grid draws them.
 *
 * `order` is allowed to be partial and to name columns that no longer exist:
 * it is a preference, not a schema. Anything it doesn't mention keeps its
 * place from the table, appended after what it does — so a column added while
 * you are looking at the table shows up at the end rather than disappearing.
 */
export function orderedColumns(all: string[], order: string[]): string[] {
  const known = new Set(all);
  const placed = order.filter((c) => known.has(c));
  const seen = new Set(placed);
  return [...placed, ...all.filter((c) => !seen.has(c))];
}

/**
 * Columns the grid draws, in order, minus the hidden ones.
 *
 * `hidden` rather than `visible` is what's stored, for the same reason: a
 * column added to the table while you are looking at it should appear, not be
 * missing because it wasn't in a list written earlier.
 */
export function visibleColumns(
  all: string[],
  hidden: Set<string>,
  order: string[] = [],
): string[] {
  return orderedColumns(all, order).filter((c) => !hidden.has(c));
}

/**
 * Moves a column one place in the *full* order, hidden columns included.
 *
 * Deliberately not "one place among the visible columns": with a column
 * hidden between two others, moving past it would look like nothing happened,
 * and un-hiding it later would put things somewhere you never asked for.
 */
export function moveColumn(
  all: string[],
  order: string[],
  column: string,
  delta: -1 | 1,
): string[] {
  const current = orderedColumns(all, order);
  const from = current.indexOf(column);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= current.length) return current;
  const next = [...current];
  next.splice(to, 0, ...next.splice(from, 1));
  return next;
}

/**
 * Drops `column` on `target`, as a header drag does.
 *
 * Which side it lands on depends on the direction of travel: dragging
 * rightwards puts it *after* the column dropped on, dragging leftwards puts it
 * before. Always inserting before would mean a column dragged to the far right
 * could never actually become the last one.
 */
export function reorderColumn(
  all: string[],
  order: string[],
  column: string,
  target: string,
): string[] {
  const current = orderedColumns(all, order);
  if (column === target) return current;
  const from = current.indexOf(column);
  const to = current.indexOf(target);
  if (from < 0 || to < 0) return current;

  const next = [...current];
  next.splice(from, 1);
  // Recomputed after the removal: an index taken from the original array is
  // one place off whenever the column moved rightwards.
  const anchor = next.indexOf(target);
  next.splice(from < to ? anchor + 1 : anchor, 0, column);
  return next;
}

export function toggleHidden(hidden: Set<string>, column: string): Set<string> {
  const next = new Set(hidden);
  if (next.has(column)) next.delete(column);
  else next.add(column);
  return next;
}

/**
 * "Clear" hides everything but the first column. A grid with no columns at all
 * is a blank rectangle with no way back to a column, so one always stays.
 */
export function clearedColumns(all: string[]): Set<string> {
  return new Set(all.slice(1));
}

/** Whether a row matches the quick filter — any cell containing the text. */
export function rowMatches(
  row: EditableRow,
  columns: string[],
  query: string,
): boolean {
  const q = query.trim().toLowerCase();
  if (q === "") return true;
  return columns.some((column) => {
    const value: Cell = cellValue(row, column);
    if (value === null) return "null".includes(q);
    return value.toLowerCase().includes(q);
  });
}

/**
 * The quick filter narrows the page that is already loaded — it is not the
 * filter bar, which asks the server. Rows being edited stay visible whatever
 * it says, so a change can't be hidden behind a filter and lost on Save.
 */
export function filterRows(
  rows: EditableRow[],
  columns: string[],
  query: string,
  isChanged: (row: EditableRow) => boolean,
): EditableRow[] {
  if (query.trim() === "") return rows;
  return rows.filter((row) => isChanged(row) || rowMatches(row, columns, query));
}

/** Ticking the header box selects what is on screen, not what was filtered out. */
export function toggleAll(
  selected: Set<string>,
  visible: EditableRow[],
): Set<string> {
  const allTicked = visible.length > 0 && visible.every((r) => selected.has(r.id));
  const next = new Set(selected);
  for (const row of visible) {
    if (allTicked) next.delete(row.id);
    else next.add(row.id);
  }
  return next;
}

export function toggleOne(selected: Set<string>, id: string): Set<string> {
  const next = new Set(selected);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  return next;
}

export type HeaderTick = "none" | "some" | "all";

export function headerTick(
  selected: Set<string>,
  visible: EditableRow[],
): HeaderTick {
  if (visible.length === 0) return "none";
  const ticked = visible.filter((r) => selected.has(r.id)).length;
  if (ticked === 0) return "none";
  return ticked === visible.length ? "all" : "some";
}

/** Page sizes the Rows picker offers. */
export const PAGE_SIZES = [25, 50, 100, 250, 500] as const;
