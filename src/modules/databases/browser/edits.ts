import type {
  Cell,
  CellPairs,
  PendingEdit,
  ResultSet,
  TableStructure,
} from "../api";
import { NULL_MARKER } from "../utils";

/**
 * The grid's staged-edit model.
 *
 * Changes are held against the row as it was *read* and only sent when Save is
 * pressed, so a mistyped cell costs nothing and one Save is one transaction.
 * Kept as pure functions over plain data rather than inside the component, so
 * the rule that actually matters — which statement a given set of edits turns
 * into — can be tested without rendering anything.
 */
export interface EditableRow {
  /** Stable across re-renders and re-sorts; rows have no identity of their own. */
  id: string;
  /** Values as read, by column name. `null` for a row added in the grid. */
  original: Record<string, Cell> | null;
  /** Only the cells actually changed, by column name. */
  changes: Record<string, Cell>;
  deleted: boolean;
}

/**
 * The backend reports SQL NULL as a marker string, which a text value of
 * literally "NULL" is indistinguishable from — a limitation of driving a CLI
 * client, and the same one the read-only grid has always had.
 *
 * It is safe for editing in both directions that matter: an UPDATE only writes
 * the cells that were explicitly changed, and a primary key misread this way
 * produces `IS NULL`, which matches no row rather than the wrong one.
 */
const toCell = (raw: string): Cell => (raw === NULL_MARKER ? null : raw);

export function rowsFromResult(rs: ResultSet): EditableRow[] {
  return rs.rows.map((cells, index) => {
    const original: Record<string, Cell> = {};
    rs.columns.forEach((column, i) => {
      original[column.name] = toCell(cells[i] ?? "");
    });
    return { id: `r${index}`, original, changes: {}, deleted: false };
  });
}

let newRowSeq = 0;
export function blankRow(): EditableRow {
  newRowSeq += 1;
  return { id: `new${newRowSeq}`, original: null, changes: {}, deleted: false };
}

export const isNewRow = (row: EditableRow): boolean => row.original === null;

/** What a cell shows now: the edit if there is one, else what was read. */
export function cellValue(row: EditableRow, column: string): Cell {
  if (column in row.changes) return row.changes[column];
  return row.original ? (row.original[column] ?? null) : null;
}

export const isCellChanged = (row: EditableRow, column: string): boolean =>
  column in row.changes;

export function isRowChanged(row: EditableRow): boolean {
  return (
    row.deleted || isNewRow(row) || Object.keys(row.changes).length > 0
  );
}

export const changeCount = (rows: EditableRow[]): number =>
  rows.filter(isRowChanged).length;

/**
 * Records a cell edit, dropping it again if the value is put back to what was
 * read — otherwise "change it and change it back" would leave a no-op UPDATE
 * staged and the Save button lit.
 */
export function setCell(
  row: EditableRow,
  column: string,
  value: Cell,
): EditableRow {
  const changes = { ...row.changes };
  const readValue = row.original ? (row.original[column] ?? null) : undefined;
  if (row.original !== null && value === readValue) delete changes[column];
  else changes[column] = value;
  return { ...row, changes };
}

/** Toggling delete on a row added in the grid discards it outright. */
export function toggleDeleted(row: EditableRow): EditableRow | null {
  if (isNewRow(row)) return null;
  return { ...row, deleted: !row.deleted };
}

export function revertRow(row: EditableRow): EditableRow | null {
  if (isNewRow(row)) return null;
  return { ...row, changes: {}, deleted: false };
}

const pairs = (values: Record<string, Cell>, columns: string[]): CellPairs =>
  columns
    .filter((c) => c in values)
    .map((c) => [c, values[c]] as [string, Cell]);

/**
 * Turns the staged rows into the statements the backend will run, in the order
 * a person would expect them applied: inserts, then updates, then deletes.
 *
 * A row that is both new and deleted never reaches here — deleting a new row
 * removes it from the grid instead.
 */
export function pendingEdits(
  rows: EditableRow[],
  columns: string[],
): PendingEdit[] {
  const inserts: PendingEdit[] = [];
  const updates: PendingEdit[] = [];
  const deletes: PendingEdit[] = [];

  for (const row of rows) {
    if (isNewRow(row)) {
      const values = pairs(row.changes, columns);
      if (values.length > 0) inserts.push({ kind: "insert", values });
      continue;
    }
    const read = pairs(row.original ?? {}, columns);
    if (row.deleted) {
      deletes.push({ kind: "delete", row: read });
      continue;
    }
    const set = pairs(row.changes, columns);
    if (set.length > 0) updates.push({ kind: "update", row: read, set });
  }
  return [...inserts, ...updates, ...deletes];
}

/**
 * Whether the grid can be edited at all. A view has no rows of its own to
 * update, and a table with no primary key has no way to name one row.
 */
export function editability(
  structure: TableStructure | null,
  objectKind: "table" | "view",
): { editable: boolean; reason: string | null } {
  if (objectKind === "view")
    return { editable: false, reason: "Views can't be edited here." };
  if (!structure) return { editable: false, reason: null };
  if (!structure.columns.some((c) => c.primary_key))
    return {
      editable: false,
      reason:
        "This table has no primary key, so there's no way to identify a single row to change.",
    };
  return { editable: true, reason: null };
}
