import type { Column, SortState } from "./DataTable";

/**
 * Sort comparison, extracted from the component so it can be unit-tested
 * without rendering. Pure — returns a new array.
 *
 * Columns without a `sortValue` are not sortable; asking to sort by one is a
 * no-op rather than an error, so a stale persisted sort key can't break a page.
 */
export function sortRows<T>(
  rows: T[],
  columns: Column<T>[],
  sort: SortState | undefined,
): T[] {
  if (!sort) return rows;

  const column = columns.find((c) => c.key === sort.key);
  if (!column?.sortValue) return rows;

  const getValue = column.sortValue;
  const direction = sort.dir === "asc" ? 1 : -1;

  return [...rows].sort((a, b) => direction * compare(getValue(a), getValue(b)));
}

function compare(a: string | number, b: string | number): number {
  if (typeof a === "number" && typeof b === "number") {
    // NaN would make the comparator inconsistent and can corrupt the sort;
    // push it to the end instead.
    if (Number.isNaN(a)) return Number.isNaN(b) ? 0 : 1;
    if (Number.isNaN(b)) return -1;
    return a - b;
  }

  // numeric:true so "item2" sorts before "item10", which matters for container
  // and worker names. sensitivity:"base" makes it case- and accent-insensitive.
  return String(a).localeCompare(String(b), undefined, {
    numeric: true,
    sensitivity: "base",
  });
}

/** Clicking the active column flips direction; a new column starts ascending. */
export function nextSort(current: SortState | undefined, key: string): SortState {
  if (current?.key === key) {
    return { key, dir: current.dir === "asc" ? "desc" : "asc" };
  }
  return { key, dir: "asc" };
}
