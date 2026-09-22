import type { FileEntry } from "./api";

export interface Crumb {
  label: string;
  path: string;
}

/** "/var/log/nginx" -> [/, var, log, nginx], each with its absolute path. */
export function splitBreadcrumb(path: string): Crumb[] {
  const crumbs: Crumb[] = [{ label: "/", path: "/" }];
  let acc = "";
  for (const part of path.split("/").filter(Boolean)) {
    acc += `/${part}`;
    crumbs.push({ label: part, path: acc });
  }
  return crumbs;
}

export function parentPath(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  const idx = trimmed.lastIndexOf("/");
  if (idx <= 0) return "/";
  return trimmed.slice(0, idx);
}

export function baseName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? "";
}

/** "~/Projects/x" when the path is under `home`, otherwise unchanged. */
export function tildePath(path: string, home: string): string {
  if (!home) return path;
  if (path === home) return "~";
  return path.startsWith(`${home}/`) ? `~${path.slice(home.length)}` : path;
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

/** Whether the in-app editor is a sensible way to open this entry. */
export function isEditable(entry: Pick<FileEntry, "name" | "is_dir" | "is_symlink">): boolean {
  if (entry.is_dir) return false;
  return !BINARY_KINDS.has(fileKind(entry).replace(" (link)", ""));
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
