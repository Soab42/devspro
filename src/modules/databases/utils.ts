export function generatePassword(length = 20): string {
  const charset =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_!@#%^*+=";
  const bytes = new Uint32Array(length);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => charset[b % charset.length]).join("");
}

/** SQL NULL as the backend spells it in a result grid. */
export const NULL_MARKER = "NULL";

export const PAGE_SIZE = 50;

/**
 * Page label for the row grid, e.g. "1–50 of 1,234". `total` is null when the
 * count is unknown (an ad-hoc query), so it falls back to what was fetched.
 */
export function pageLabel(
  offset: number,
  fetched: number,
  total: number | null,
): string {
  if (fetched === 0 && (total === null || total === 0)) return "No rows";
  const first = offset + 1;
  const last = offset + fetched;
  const range = `${first.toLocaleString()}–${last.toLocaleString()}`;
  return total === null ? range : `${range} of ${total.toLocaleString()}`;
}

/** Whether a Next page exists — unknown totals allow paging while pages come back full. */
export function hasNextPage(
  offset: number,
  fetched: number,
  total: number | null,
  pageSize = PAGE_SIZE,
): boolean {
  if (total !== null) return offset + fetched < total;
  return fetched === pageSize;
}

/**
 * Column sort state -> the `order_by` string the backend validates
 * (`<column> asc|desc`). The key is the column name itself.
 */
export function orderByFrom(
  sort: { key: string; dir: "asc" | "desc" } | undefined,
): string | undefined {
  if (!sort) return undefined;
  return `${sort.key} ${sort.dir}`;
}

/**
 * Sort value for a cell so the client-side sort of the current page agrees
 * with the server-side ORDER BY: numbers compare numerically, NULL sorts first.
 */
export function cellSortValue(cell: string): string | number {
  if (cell === NULL_MARKER) return "";
  const n = Number(cell);
  return cell.trim() !== "" && Number.isFinite(n) ? n : cell;
}

/** Only MySQL/Postgres containers have a tabular browser. */
export function isBrowsable(image: string): boolean {
  return (
    image.startsWith("mysql") ||
    image.startsWith("mariadb") ||
    image.startsWith("postgres")
  );
}

/** Default file name for a dump, e.g. `shop_20260922.sql`. */
export function dumpFileName(database: string, now: Date): string {
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${database}_${y}${m}${d}.sql`;
}
