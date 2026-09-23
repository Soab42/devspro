import type {
  Cell,
  Filter,
  RelationSpec,
  ResultSet,
  TableStructure,
} from "../api";
import { RELATION_PREFIX } from "../api";

/**
 * Turning a table's foreign keys into something the grid can draw and click.
 *
 * Two directions, and they behave differently:
 *
 *   * **Outgoing** — this row points at one other row. The cell holding the
 *     key becomes a link to it.
 *   * **Incoming** — other rows point at this one, any number of them. The
 *     grid gets an extra column per relation showing how many, which opens
 *     that table filtered to them.
 *
 * Both navigations are the same act: open a table with a filter already
 * applied. That keeps them one code path, and means the filter bar shows
 * exactly why you are seeing these rows.
 */

/** Where a single cell points. */
export interface OutgoingLink {
  /** The column in this table that holds the key. */
  column: string;
  table: string;
  /** The column in that table it matches. */
  referencesColumn: string;
}

/**
 * Only single-column foreign keys become clickable cells: a composite key is
 * spread across several cells, and no one of them is the link.
 */
export function outgoingLinks(
  structure: TableStructure | null,
): Map<string, OutgoingLink> {
  const links = new Map<string, OutgoingLink>();
  for (const fk of structure?.foreign_keys ?? []) {
    if (fk.columns.length !== 1 || fk.references_columns.length !== 1) continue;
    links.set(fk.columns[0], {
      column: fk.columns[0],
      table: fk.references_table,
      referencesColumn: fk.references_columns[0],
    });
  }
  return links;
}

/**
 * The reverse relations to show as columns, deduplicated by target table and
 * key — two constraints between the same pair of tables on the same columns
 * would otherwise draw two identical columns.
 */
export function relationColumns(
  structure: TableStructure | null,
): RelationSpec[] {
  const seen = new Set<string>();
  const out: RelationSpec[] = [];
  for (const relation of structure?.referenced_by ?? []) {
    if (relation.columns.length === 0) continue;
    const key = `${relation.table}:${relation.columns.join(",")}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({
      table: relation.table,
      columns: relation.columns,
      references_columns: relation.references_columns,
    });
  }
  return out;
}

/**
 * The key a reverse relation takes in the column list.
 *
 * Namespaced so it can sit in the same ordering and hiding state as the
 * table's own columns without a relation to `users` colliding with a column
 * named `users`. The key is internal; `relationLabel` is what people read.
 */
export const RELATION_COLUMN_PREFIX = "rel:";

export const relationKey = (relation: RelationSpec): string =>
  `${RELATION_COLUMN_PREFIX}${relation.table}:${relation.columns.join(",")}`;

export const isRelationKey = (key: string): boolean =>
  key.startsWith(RELATION_COLUMN_PREFIX);

/** What a relation column is called in the header and the column manager. */
export const relationLabel = (relation: RelationSpec): string => relation.table;

/** Relation and its position, by column key — the position picks its count. */
export function relationsByKey(
  relations: RelationSpec[],
): Map<string, { relation: RelationSpec; index: number }> {
  return new Map(
    relations.map((relation, index) => [
      relationKey(relation),
      { relation, index },
    ]),
  );
}

/** The index of a `__rel_N` column, or null for an ordinary one. */
export function relationIndexOf(columnName: string): number | null {
  if (!columnName.startsWith(RELATION_PREFIX)) return null;
  const index = Number(columnName.slice(RELATION_PREFIX.length));
  return Number.isInteger(index) && index >= 0 ? index : null;
}

/**
 * Splits a result set into the table's own columns and the relation counts the
 * backend appended. Everything downstream — editing, the column manager,
 * export — works on the data columns alone.
 */
export function splitResult(rs: ResultSet): {
  dataColumns: ResultSet["columns"];
  relationIndexes: number[];
} {
  const dataColumns: ResultSet["columns"] = [];
  const relationIndexes: number[] = [];
  rs.columns.forEach((column, index) => {
    if (relationIndexOf(column.name) === null) dataColumns.push(column);
    else relationIndexes.push(index);
  });
  return { dataColumns, relationIndexes };
}

/**
 * The filter that opens the row an outgoing link points at.
 *
 * A null key is not a link — nothing to open — so this returns null rather
 * than a filter that would match every NULL row in the target table.
 */
export function linkFilter(link: OutgoingLink, value: Cell): Filter[] | null {
  if (value === null) return null;
  return [{ column: link.referencesColumn, op: "eq", value }];
}

/**
 * The filter that opens a relation's child rows. Every key column has to have
 * a value, or the result would be wider than the relation actually is.
 */
export function relationFilter(
  relation: RelationSpec,
  row: Record<string, Cell>,
): Filter[] | null {
  const filters: Filter[] = [];
  for (let i = 0; i < relation.columns.length; i++) {
    const value = row[relation.references_columns[i]];
    if (value === null || value === undefined) return null;
    filters.push({ column: relation.columns[i], op: "eq", value });
  }
  return filters.length > 0 ? filters : null;
}

/** Where a click on a link or a relation chip should take you. */
export interface Navigation {
  table: string;
  filters: Filter[];
}
