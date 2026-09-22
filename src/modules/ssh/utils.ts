import type { SftpEntry, SshHost } from "./api";

export interface Crumb {
  label: string;
  path: string;
}

/**
 * "/var/log/nginx" -> [/, var, log, nginx], each with the absolute path it
 * navigates to. The root is always present so there is somewhere to click
 * back to from a top-level directory.
 */
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

export function joinPath(dir: string, name: string): string {
  if (dir === "" || dir === "/") return `/${name}`;
  return `${dir.replace(/\/+$/, "")}/${name}`;
}

export function baseName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? "";
}

const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** 1536 -> "1.5 KB"; whole bytes stay integers. */
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

export type EntrySortKey = "name" | "size" | "modified";

/**
 * Directories first regardless of sort direction — that's how every file
 * manager works, and mixing them in by size would scatter folders (which have
 * no meaningful size) through the list.
 */
export function sortEntries(entries: SftpEntry[], key: EntrySortKey, dir: "asc" | "desc"): SftpEntry[] {
  const sign = dir === "asc" ? 1 : -1;
  return [...entries].sort((a, b) => {
    if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
    let cmp = 0;
    if (key === "name") cmp = a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
    else if (key === "size") cmp = a.size - b.size;
    else cmp = a.modified - b.modified;
    if (cmp === 0) cmp = a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
    return cmp * sign;
  });
}

/** "deploy@example.com:2222" — the port only when it isn't the default. */
export function hostLabel(host: Pick<SshHost, "username" | "host" | "port">): string {
  const port = host.port === 22 ? "" : `:${host.port}`;
  return `${host.username}@${host.host}${port}`;
}

export const UNKNOWN_HOST_PREFIX = "UNKNOWN_HOST:";
export const PASSWORD_REQUIRED = "PASSWORD_REQUIRED";

/** The fingerprint from an `UNKNOWN_HOST:<fp>` error, or null for any other error. */
export function unknownHostFingerprint(message: string): string | null {
  const text = message.trim();
  if (!text.startsWith(UNKNOWN_HOST_PREFIX)) return null;
  return text.slice(UNKNOWN_HOST_PREFIX.length).trim();
}

export function isPasswordRequired(message: string): boolean {
  return message.trim() === PASSWORD_REQUIRED;
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Text a host form can be rejected with before a round trip to Rust. */
export function validateHostForm(input: {
  name: string;
  host: string;
  port: string;
  username: string;
  auth: string;
  keyPath: string;
}): string | null {
  if (!input.name.trim()) return "Name is required.";
  if (!input.host.trim()) return "Host is required.";
  if (/\s/.test(input.host.trim())) return "Host cannot contain spaces.";
  const port = Number(input.port);
  if (!Number.isInteger(port) || port < 1 || port > 65535) return "Port must be between 1 and 65535.";
  if (!input.username.trim()) return "Username is required.";
  if (input.auth === "key" && !input.keyPath.trim()) return "Choose a private key file for key authentication.";
  return null;
}

/** A short, unique-enough title for a terminal tab. */
export function terminalTitle(command: "local" | { ssh: string }, hosts: SshHost[], ordinal: number): string {
  if (command === "local") return ordinal > 1 ? `Local ${ordinal}` : "Local";
  const host = hosts.find((h) => h.id === command.ssh);
  const base = host ? host.name : "ssh";
  return ordinal > 1 ? `${base} (${ordinal})` : base;
}
