import type { FileEntry } from "./api";

export interface Crumb {
  label: string;
  path: string;
}

/*
 * Paths come from the backend in the OS's own spelling: `/home/me/x` on
 * Linux, `C:\Users\me\x` on Windows, where every drive is a root of its own.
 * The helpers below tell the two apart by the path's shape rather than by
 * asking which OS this is, so a Unix path is handled exactly as it always was.
 */

/** `C:\…`, `C:/…`, a bare `C:`, or a UNC `\\server\share\…`. */
export function isWindowsPath(path: string): boolean {
  return /^[A-Za-z]:([\\/]|$)/.test(path) || path.startsWith("\\\\");
}

/** The separator a path is written with. */
export function pathSep(path: string): "/" | "\\" {
  return isWindowsPath(path) ? "\\" : "/";
}

/** The root a path hangs from: `/`, or its drive (`C:\`) or share (`\\server\share\`). */
export function pathRoot(path: string): string {
  if (!isWindowsPath(path)) return "/";
  const unc = /^\\\\[^\\/]+[\\/][^\\/]+/.exec(path);
  if (unc) return `${unc[0].replace(/\//g, "\\")}\\`;
  return `${path.slice(0, 2).toUpperCase()}\\`;
}

export function isRootPath(path: string): boolean {
  return path === pathRoot(path) || (isWindowsPath(path) && path.replace(/[\\/]+$/, "") === pathRoot(path).slice(0, -1));
}

/** `dir` and `name` with exactly one separator between them. */
export function joinPath(dir: string, name: string): string {
  const sep = pathSep(dir);
  return dir.endsWith("/") || dir.endsWith("\\") ? `${dir}${name}` : `${dir}${sep}${name}`;
}

/** Whether `path` is `root` itself or anywhere beneath it. */
export function isUnder(path: string, root: string): boolean {
  if (path === root) return true;
  return path.startsWith(root.endsWith("/") || root.endsWith("\\") ? root : `${root}${pathSep(root)}`);
}

/** The path's segments below its root. */
function segments(path: string): string[] {
  const rest = path.slice(pathRoot(path).length - (isWindowsPath(path) ? 1 : 0));
  return rest.split(isWindowsPath(path) ? /[\\/]/ : "/").filter(Boolean);
}

/**
 * "/var/log/nginx" -> [/, var, log, nginx], each with its absolute path;
 * "C:\Users\me" -> [C:, Users, me].
 */
export function splitBreadcrumb(path: string): Crumb[] {
  const root = pathRoot(path);
  const crumbs: Crumb[] = [{ label: root === "/" ? "/" : root.slice(0, -1), path: root }];
  let acc = root;
  for (const part of segments(path)) {
    acc = joinPath(acc, part);
    crumbs.push({ label: part, path: acc });
  }
  return crumbs;
}

export function parentPath(path: string): string {
  if (isWindowsPath(path)) {
    const root = pathRoot(path);
    const trimmed = path.replace(/[\\/]+$/, "");
    const idx = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
    return idx < root.length ? root : trimmed.slice(0, idx);
  }
  const trimmed = path.replace(/\/+$/, "");
  const idx = trimmed.lastIndexOf("/");
  if (idx <= 0) return "/";
  return trimmed.slice(0, idx);
}

export function baseName(path: string): string {
  if (isWindowsPath(path)) {
    const parts = segments(path);
    return parts[parts.length - 1] ?? "";
  }
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? "";
}

/** "~/Projects/x" when the path is under `home`, otherwise unchanged. */
export function tildePath(path: string, home: string): string {
  if (!home) return path;
  if (path === home) return "~";
  return isUnder(path, home) ? `~${path.slice(home.length)}` : path;
}

/**
 * The Permissions row: the octal mode and its `rwx` spelling on Unix. Windows
 * has no mode bits, only a read-only flag, which is all the mode reports there.
 */
export function permissionsLabel(path: string, permissions: string, symbolic?: string): string {
  if (isWindowsPath(path)) return parseInt(permissions || "0", 8) & 0o200 ? "Read and write" : "Read-only";
  return `0${permissions}${symbolic ? ` (${symbolic})` : ""}`;
}

/** The Owner row: `user:group`, or just the owner where there is no group (Windows). */
export function ownerLabel(owner: string, group: string): string {
  return group ? `${owner}:${group}` : owner;
}

const UNITS = ["B", "KB", "MB", "GB", "TB"];

export function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 ? 0 : value >= 100 ? 0 : 1;
  return `${value.toFixed(digits)} ${UNITS[unit]}`;
}

export function formatModified(unixSecs: number): string {
  if (!unixSecs) return "—";
  const d = new Date(unixSecs * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export type EntrySortKey = "name" | "size" | "modified" | "kind";

/** Directories first regardless of direction, then the chosen column. */
export function sortEntries(entries: FileEntry[], key: EntrySortKey, dir: "asc" | "desc"): FileEntry[] {
  const sign = dir === "asc" ? 1 : -1;
  const byName = (a: FileEntry, b: FileEntry) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  return [...entries].sort((a, b) => {
    if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
    let cmp = 0;
    if (key === "name") cmp = byName(a, b);
    else if (key === "size") cmp = a.size - b.size;
    else if (key === "modified") cmp = a.modified - b.modified;
    else cmp = fileKind(a).localeCompare(fileKind(b));
    if (cmp === 0) cmp = byName(a, b);
    return cmp * sign;
  });
}

/** Hidden entries and a case-insensitive substring filter, like Finder's search field. */
export function filterEntries(entries: FileEntry[], query: string, showHidden: boolean): FileEntry[] {
  const q = query.trim().toLowerCase();
  return entries.filter((e) => (showHidden || !e.is_hidden) && (!q || e.name.toLowerCase().includes(q)));
}

export function extensionOf(name: string): string {
  const idx = name.lastIndexOf(".");
  if (idx <= 0 || idx === name.length - 1) return "";
  return name.slice(idx + 1).toLowerCase();
}

const KIND_BY_EXT: Record<string, string> = {
  ts: "TypeScript",
  tsx: "TypeScript",
  js: "JavaScript",
  jsx: "JavaScript",
  mjs: "JavaScript",
  cjs: "JavaScript",
  json: "JSON",
  yml: "YAML",
  yaml: "YAML",
  toml: "TOML",
  md: "Markdown",
  txt: "Text",
  log: "Log",
  env: "Env file",
  rs: "Rust",
  php: "PHP",
  py: "Python",
  go: "Go",
  rb: "Ruby",
  sh: "Shell script",
  bash: "Shell script",
  zsh: "Shell script",
  html: "HTML",
  css: "CSS",
  scss: "SCSS",
  svg: "SVG image",
  png: "PNG image",
  jpg: "JPEG image",
  jpeg: "JPEG image",
  gif: "GIF image",
  webp: "WebP image",
  mp3: "Audio",
  wav: "Audio",
  flac: "Audio",
  mp4: "Video",
  mkv: "Video",
  webm: "Video",
  pdf: "PDF",
  zip: "Archive",
  tar: "Archive",
  gz: "Archive",
  xz: "Archive",
  "7z": "Archive",
  sql: "SQL",
  lock: "Lock file",
  conf: "Config",
  ini: "Config",
};

/** A Finder-style "Kind" column value. */
export function fileKind(entry: Pick<FileEntry, "name" | "is_dir" | "is_symlink">): string {
  if (entry.is_dir) return entry.is_symlink ? "Folder (link)" : "Folder";
  if (entry.name === ".env" || entry.name.startsWith(".env.")) return "Env file";
  if (entry.name === "Dockerfile") return "Dockerfile";
  if (entry.name === "Makefile") return "Makefile";
  const ext = extensionOf(entry.name);
  const kind = ext ? (KIND_BY_EXT[ext] ?? `${ext.toUpperCase()} file`) : "Document";
  return entry.is_symlink ? `${kind} (link)` : kind;
}

const BINARY_KINDS = new Set(["PNG image", "JPEG image", "GIF image", "WebP image", "Audio", "Video", "PDF", "Archive"]);

/** What a file tab shows: the editor, a picture, a player, or just the facts. */
export type ViewerKind = "text" | "image" | "video" | "audio" | "pdf" | "other";

/* What the webview itself can draw or play. TIFF and AVI are left out:
   WebKit shows neither, so they are "other" and open in the desktop's app. */
const IMAGE_VIEW = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "avif"]);
const VIDEO_VIEW = new Set(["mp4", "m4v", "webm", "mkv", "mov", "ogv"]);
const AUDIO_VIEW = new Set(["mp3", "wav", "flac", "ogg", "oga", "opus", "m4a", "aac"]);
/** Formats that are not text, so the editor has nothing to show for them. */
const BINARY_EXTS = new Set([
  "zip", "tar", "gz", "tgz", "xz", "bz2", "zst", "7z", "rar", "iso", "img", "dmg", "deb", "rpm", "apk", "appimage",
  "exe", "dll", "so", "o", "a", "bin", "class", "jar", "war", "pyc", "wasm", "node",
  "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "epub",
  "ttf", "otf", "woff", "woff2", "eot", "sqlite", "sqlite3", "db", "psd", "xcf", "tif", "tiff", "heic",
  "avi", "wmv", "flv", "mpg", "mpeg", "3gp", "wma", "mid", "midi",
]);

/**
 * How a file opens in a tab, or null when it does not: folders, and Google
 * Docs on a Drive mount, which only exist as a web page.
 */
export function viewerKind(entry: Pick<FileEntry, "name" | "is_dir" | "is_symlink"> & { link?: string }): ViewerKind | null {
  if (entry.is_dir || entry.link) return null;
  const ext = extensionOf(entry.name);
  if (IMAGE_VIEW.has(ext)) return "image";
  if (VIDEO_VIEW.has(ext)) return "video";
  if (AUDIO_VIEW.has(ext)) return "audio";
  if (ext === "pdf") return "pdf";
  if (BINARY_EXTS.has(ext) || BINARY_KINDS.has(fileKind(entry).replace(" (link)", ""))) return "other";
  return "text";
}

/** Whether the in-app editor is a sensible way to open this entry. */
export function isEditable(entry: Pick<FileEntry, "name" | "is_dir" | "is_symlink"> & { link?: string }): boolean {
  return viewerKind(entry) === "text";
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * Finder-style back/forward: a linear history with a cursor. Navigating
 * somewhere new truncates anything ahead of the cursor, like a browser.
 */
export interface History {
  entries: string[];
  index: number;
}

export function historyPush(h: History, path: string): History {
  if (h.entries[h.index] === path) return h;
  const entries = [...h.entries.slice(0, h.index + 1), path];
  return { entries, index: entries.length - 1 };
}

export function historyStep(h: History, delta: -1 | 1): History {
  const index = h.index + delta;
  if (index < 0 || index >= h.entries.length) return h;
  return { ...h, index };
}

export const canGoBack = (h: History) => h.index > 0;
export const canGoForward = (h: History) => h.index < h.entries.length - 1;

/**
 * The directories a column view shows for `path`: the nearest place that
 * contains it (so the home column sits first rather than seven columns of
 * `/`, `/home`, ...), then every folder down to `path` itself. A path under no
 * place starts at its root: `/`, or its drive on Windows.
 */
export function columnChain(path: string, placePaths: string[]): string[] {
  const root =
    placePaths.filter((p) => isUnder(path, p)).sort((a, b) => b.length - a.length)[0] ?? pathRoot(path);
  const chain = [root];
  if (path === root) return chain;
  const rest = path.slice(root.length).split(isWindowsPath(path) ? /[\\/]/ : "/").filter(Boolean);
  let acc = root;
  for (const part of rest) {
    acc = joinPath(acc, part);
    chain.push(acc);
  }
  return chain;
}

/**
 * Finder-style click selection over the rows currently on screen. Plain click
 * selects one; ctrl toggles; shift extends from the anchor (the last plain or
 * ctrl click) to the clicked row. Returns the new ordered selection and anchor.
 */
export function clickSelect(
  rows: string[],
  selected: string[],
  anchor: string | null,
  clicked: string,
  mods: { ctrl?: boolean; shift?: boolean },
): { selected: string[]; anchor: string | null } {
  if (mods.shift && anchor !== null && rows.includes(anchor)) {
    const a = rows.indexOf(anchor);
    const b = rows.indexOf(clicked);
    if (b < 0) return { selected, anchor };
    const [from, to] = a < b ? [a, b] : [b, a];
    const range = rows.slice(from, to + 1);
    const base = mods.ctrl ? selected.filter((p) => !range.includes(p)) : [];
    return { selected: [...base, ...range], anchor };
  }
  if (mods.ctrl) {
    return selected.includes(clicked)
      ? { selected: selected.filter((p) => p !== clicked), anchor: clicked }
      : { selected: [...selected, clicked], anchor: clicked };
  }
  return { selected: [clicked], anchor: clicked };
}

/** A list-view row: an entry at a depth, folded open or not. */
export interface TreeRow {
  entry: FileEntry;
  depth: number;
  expanded: boolean;
  /** The folder is open but its listing has not arrived yet. */
  pending: boolean;
}

/**
 * Flattens a folder listing with its unfolded subfolders into the rows a
 * table draws, depth-first, the way Finder's list view does. `children` is
 * whatever listings have been fetched so far; an expanded folder without one
 * yet is marked pending so the caller knows to fetch it.
 */
export function flattenTree(
  entries: FileEntry[],
  expanded: ReadonlySet<string>,
  children: Record<string, FileEntry[] | undefined>,
  depth = 0,
): TreeRow[] {
  const out: TreeRow[] = [];
  for (const entry of entries) {
    const open = entry.is_dir && expanded.has(entry.path);
    const kids = open ? children[entry.path] : undefined;
    out.push({ entry, depth, expanded: open, pending: open && kids === undefined });
    if (open && kids) out.push(...flattenTree(kids, expanded, children, depth + 1));
  }
  return out;
}

/** Drop `paths` on `dir`: the ones that actually move, and why the rest do not. */
export function planMove(paths: string[], dir: string): { moves: string[]; skipped: string[] } {
  const moves: string[] = [];
  const skipped: string[] = [];
  for (const p of paths) {
    const inside = isUnder(dir, p);
    if (parentPath(p) === dir || inside) skipped.push(p);
    else moves.push(p);
  }
  return { moves, skipped };
}

/** Icon view sizes, Windows-style; the slider covers everything in between. */
export const THUMB_PRESETS = [
  { label: "Small icons", size: 48 },
  { label: "Medium icons", size: 96 },
  { label: "Large icons", size: 160 },
  { label: "Extra large icons", size: 256 },
] as const;
export const THUMB_MIN = 32;
export const THUMB_MAX = 256;
export const THUMB_DEFAULT = 96;

/** A requested icon size, kept in range and on an 8px step. */
export function clampThumb(size: number): number {
  if (!Number.isFinite(size)) return THUMB_DEFAULT;
  return Math.round(Math.min(THUMB_MAX, Math.max(THUMB_MIN, size)) / 8) * 8;
}

/**
 * The pixel size to render a thumbnail at for an icon shown `display` wide:
 * the next of a few fixed sizes, so dragging the slider re-renders only when
 * it crosses one, and a HiDPI screen gets sharp pictures.
 */
export function thumbRenderSize(display: number, pixelRatio = 1): number {
  const want = display * Math.max(1, pixelRatio);
  return [128, 256, 512].find((b) => b >= want) ?? 512;
}

const RELATIVE_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 3600],
  ["month", 30 * 24 * 3600],
  ["week", 7 * 24 * 3600],
  ["day", 24 * 3600],
  ["hour", 3600],
  ["minute", 60],
];

/** "3 days ago", the way GitLens dates a line. */
export function relativeTime(unixSecs: number, nowMs = Date.now()): string {
  const ago = Math.round(nowMs / 1000 - unixSecs);
  if (ago < 45) return "just now";
  const fmt = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  for (const [unit, secs] of RELATIVE_UNITS) {
    if (ago >= secs) return fmt.format(-Math.floor(ago / secs), unit);
  }
  return fmt.format(-1, "minute");
}
