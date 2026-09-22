import { ReactNode, useMemo, useState } from "react";
import { ChevronDown, ChevronUp, ChevronsUpDown } from "./icons";
import { Icon } from "./Icon";
import { Skeleton } from "./Feedback";
import { EmptyState, EmptyStateProps } from "./Layout";
import { nextSort, sortRows } from "./dataTableUtils";
import "./DataTable.css";

export type ColumnAlign = "start" | "end" | "center";
export type SortDirection = "asc" | "desc";

export interface SortState {
  key: string;
  dir: SortDirection;
}

/**
 * Column visibility priority for narrow windows:
 *
 *   1 — identity and actions. Never hidden. Aim for three or fewer.
 *   2 — state you would scan for. Hidden below ~700px of table width.
 *   3 — detail you would look up. Hidden below ~900px of table width.
 *
 * Actions belong at priority 1: hiding them, or letting them scroll away
 * horizontally, removes the column users reach for most.
 */
export type ColumnPriority = 1 | 2 | 3;

export interface Column<T> {
  key: string;
  header: ReactNode;
  render: (row: T, index: number) => ReactNode;
  /** Providing this makes the column sortable. */
  sortValue?: (row: T) => string | number;
  align?: ColumnAlign;
  width?: string;
  minWidth?: string;
  priority?: ColumnPriority;
  mono?: boolean;
  wrap?: boolean;
  /** Plain-text label for aria when `header` is not a string. */
  headerLabel?: string;
}

export interface DataTableProps<T> {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  /** Controlled sort. Omit and use `defaultSort` to let the table own it. */
  sort?: SortState;
  onSortChange?: (sort: SortState) => void;
  defaultSort?: SortState;
  loading?: boolean;
  skeletonRows?: number;
  /** Same shape as Section's `empty`, so the two are interchangeable. */
  empty?: EmptyStateProps;
  rowClassName?: (row: T) => string | undefined;
  onRowClick?: (row: T) => void;
  /** Open/drill-in gesture, e.g. entering a directory in a file browser. */
  onRowDoubleClick?: (row: T) => void;
  stickyHeader?: boolean;
  density?: "compact" | "normal";
  /** Screen-reader caption describing the table. */
  caption?: string;
  /** px width below which the table scrolls horizontally instead of shrinking. */
  minWidth?: number;
  className?: string;
}

export function DataTable<T>({
  columns,
  rows,
  rowKey,
  sort,
  onSortChange,
  defaultSort,
  loading = false,
  skeletonRows = 5,
  empty,
  rowClassName,
  onRowClick,
  onRowDoubleClick,
  stickyHeader = true,
  density = "normal",
  caption,
  minWidth = 640,
  className,
}: DataTableProps<T>) {
  const [internalSort, setInternalSort] = useState<SortState | undefined>(defaultSort);
  const activeSort = sort ?? internalSort;

  function applySort(key: string) {
    const next = nextSort(activeSort, key);
    if (onSortChange) onSortChange(next);
    if (!sort) setInternalSort(next);
  }

  const sorted = useMemo(
    () => sortRows(rows, columns, activeSort),
    [rows, columns, activeSort],
  );

  const classes = [
    "dt",
    `dt-density-${density}`,
    stickyHeader && "dt-sticky",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className="dt-scroll">
      <table className={classes} style={{ minWidth }}>
        {caption && <caption className="u-sr-only">{caption}</caption>}
        <thead className="dt-head">
          <tr>
            {columns.map((column) => {
              const sortable = Boolean(column.sortValue);
              const isSorted = activeSort?.key === column.key;
              return (
                <th
                  key={column.key}
                  scope="col"
                  className={cellClasses(column, "dt-th", sortable && "dt-sortable")}
                  style={{ width: column.width, minWidth: column.minWidth }}
                  aria-sort={
                    !sortable
                      ? undefined
                      : isSorted
                        ? activeSort.dir === "asc"
                          ? "ascending"
                          : "descending"
                        : "none"
                  }
                >
                  {sortable ? (
                    <button
                      type="button"
                      className="dt-sort-btn"
                      onClick={() => applySort(column.key)}
                    >
                      <span>{column.header}</span>
                      <Icon
                        icon={
                          isSorted
                            ? activeSort.dir === "asc"
                              ? ChevronUp
                              : ChevronDown
                            : ChevronsUpDown
                        }
                        size="xs"
                        className={isSorted ? "dt-sort-icon" : "dt-sort-icon dt-sort-hint"}
                      />
                    </button>
                  ) : (
                    column.header
                  )}
                </th>
              );
            })}
          </tr>
        </thead>

        <tbody>
          {loading &&
            Array.from({ length: skeletonRows }, (_, i) => (
              <tr key={`skeleton-${i}`} className="dt-row">
                {columns.map((column) => (
                  <td key={column.key} className={cellClasses(column, "dt-td")}>
                    <Skeleton height="0.85rem" />
                  </td>
                ))}
              </tr>
            ))}

          {!loading && sorted.length === 0 && (
            <tr>
              <td colSpan={columns.length} className="dt-empty-cell">
                <EmptyState {...(empty ?? { title: "Nothing to show" })} />
              </td>
            </tr>
          )}

          {!loading &&
            sorted.map((row, index) => (
              <tr
                key={rowKey(row)}
                className={[
                  "dt-row",
                  onRowClick && "dt-row-clickable",
                  rowClassName?.(row),
                ]
                  .filter(Boolean)
                  .join(" ")}
                onClick={onRowClick ? () => onRowClick(row) : undefined}
                onDoubleClick={onRowDoubleClick ? () => onRowDoubleClick(row) : undefined}
              >
                {columns.map((column) => (
                  <td key={column.key} className={cellClasses(column, "dt-td")}>
                    {column.render(row, index)}
                  </td>
                ))}
              </tr>
            ))}
        </tbody>
      </table>
    </div>
  );
}

/**
 * The same classes go on the <th> and every <td> of a column, so priority
 * hiding stays in sync with no JS measurement or resize observer.
 */
function cellClasses<T>(column: Column<T>, base: string, extra?: string | false): string {
  return [
    base,
    extra,
    column.priority === 2 && "dt-prio-2",
    column.priority === 3 && "dt-prio-3",
    column.align && column.align !== "start" && `dt-align-${column.align}`,
    column.mono && "dt-cell-mono",
    column.wrap && "dt-cell-wrap",
  ]
    .filter(Boolean)
    .join(" ");
}
