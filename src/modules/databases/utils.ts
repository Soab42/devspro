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

/* ------------------------------------------------------ Connection refs -- */

/**
 * Prefix the backend uses to tell a saved custom connection apart from a
 * service in DevsPro's own compose project. Keeping both behind one opaque
 * string is why every browse command still takes a single `connection`.
 */
export const CUSTOM_PREFIX = "custom:";

export const customRef = (id: string): string => `${CUSTOM_PREFIX}${id}`;

export const isCustomRef = (ref: string): boolean =>
  ref.startsWith(CUSTOM_PREFIX);

/** The saved-connection id inside a ref, or null for a managed service. */
export function customIdOf(ref: string): string | null {
  return isCustomRef(ref) ? ref.slice(CUSTOM_PREFIX.length) : null;
}

/** Default port for an engine, used to prefill the connection form. */
export const DEFAULT_PORT: Record<"mysql" | "postgres", number> = {
  mysql: 3306,
  postgres: 5432,
};

/** Default admin user for an engine, used as the form's placeholder. */
export const DEFAULT_USER: Record<"mysql" | "postgres", string> = {
  mysql: "root",
  postgres: "postgres",
};

/**
 * One line saying where a saved connection actually points — the only thing
 * that distinguishes a remote server from a local one, since both are plain
 * TCP as far as the backend is concerned.
 */
export function connectionTarget(c: {
  transport: "tcp" | "docker";
  host: string;
  port: number;
  container: string;
}): string {
  return c.transport === "docker" ? c.container : `${c.host}:${c.port}`;
}

/**
 * Image-name fragments that mark a container as running one engine or the
 * other, used to narrow the container picker from "every container on this
 * machine" to the handful that could plausibly be a database.
 *
 * Matched anywhere in the image reference so a registry prefix or a tag
 * doesn't defeat it (`ghcr.io/acme/postgres:16-alpine`). This is a heuristic —
 * a database built into a custom image matches nothing here, which is why the
 * picker can still be told to show everything.
 */
const ENGINE_IMAGE_HINTS: Record<"mysql" | "postgres", string[]> = {
  mysql: ["mysql", "mariadb", "percona"],
  postgres: ["postgres", "pgvector", "timescale", "postgis"],
};

export function isEngineImage(
  image: string,
  kind: "mysql" | "postgres",
): boolean {
  const lower = image.toLowerCase();
  return ENGINE_IMAGE_HINTS[kind].some((hint) => lower.includes(hint));
}
