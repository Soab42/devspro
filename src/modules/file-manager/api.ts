import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_symlink: boolean;
  is_hidden: boolean;
  size: number;
  /** Unix seconds; 0 when unknown. */
  modified: number;
  /** Octal mode, e.g. "755". */
  permissions: string;
  /** A Google Docs, Sheets or Slides file on a Drive mount: the web page it opens as. */
  link?: string;
}

export type PlaceKind =
  | "home"
  | "desktop"
  | "documents"
  | "downloads"
  | "pictures"
  | "music"
  | "videos"
  | "root";

export interface Place {
  label: string;
  path: string;
  kind: PlaceKind;
}

export interface Places {
  home: string;
  places: Place[];
}

export const fmPlaces = () => invoke<Places>("fm_places");
export const fmList = (path: string) =>
  invoke<FileEntry[]>("fm_list", { path });
export const fmStat = (path: string) => invoke<FileEntry>("fm_stat", { path });

/** What the column view's preview card shows: the entry plus what a listing does not carry. */
export interface FileDetail {
  entry: FileEntry;
  /** Unix seconds; 0 when the filesystem does not record it. */
  created: number;
  owner: string;
  group: string;
  /** `rw-r--r--`. */
  mode_symbolic: string;
  mime: string;
}

export const fmStatDetail = (path: string) =>
  invoke<FileDetail>("fm_stat_detail", { path });
export const fmMkdir = (dir: string, name: string) =>
  invoke<string>("fm_mkdir", { dir, name });
export const fmCreateFile = (dir: string, name: string) =>
  invoke<string>("fm_create_file", { dir, name });
export const fmRename = (path: string, name: string) =>
  invoke<string>("fm_rename", { path, name });
/** Move a file or folder into `dir`, keeping its name. */
export const fmMove = (path: string, dir: string) =>
  invoke<string>("fm_move", { path, dir });
export const fmCopy = (path: string, dir: string) =>
  invoke<string>("fm_copy", { path, dir });
export const fmTrash = (path: string) => invoke<string>("fm_trash", { path });
export const fmReadFile = (path: string) =>
  invoke<string>("fm_read_file", { path });
export const fmWriteFile = (path: string, contents: string) =>
  invoke<string>("fm_write_file", { path, contents });
export const fmOpen = (path: string) => invoke<void>("fm_open", { path });
export const fmOpenInEditor = (path: string, command: string) =>
  invoke<void>("fm_open_in_editor", { path, command });

export interface DirStats {
  files: number;
  dirs: number;
  bytes: number;
  /** The walk stopped at a cap; the numbers are a floor. */
  truncated: boolean;
}

export const fmDirStats = (path: string) =>
  invoke<DirStats>("fm_dir_stats", { path });

/** A code editor installed on this machine, for the "Open in" menu. */
export interface EditorApp {
  id: string;
  label: string;
  /** Runs with the path appended, like the configured external editor. */
  command: string;
}

export const fmEditors = () => invoke<EditorApp[]>("fm_editors");
export const fmOpenTerminal = (dir: string) =>
  invoke<void>("fm_open_terminal", { dir });

export const fmThumbnail = (path: string, size: number) =>
  invoke<{ data_url: string } | null>("fm_thumbnail", { path, size });

/** How a device is drawn and which sidebar group it sits in. */
export type DeviceKind = "usb" | "drive" | "phone" | "camera" | "disc" | "cloud" | "network";

/** A USB drive, phone, camera, Drive account or share, mounted or not. */
export interface Device {
  /** Stable across mounting and unmounting. */
  id: string;
  label: string;
  kind: DeviceKind;
  /** Where its files are; null until mounted. */
  path: string | null;
  /** Pass to `fmDeviceAction("mount", …)`; null once mounted. */
  mount: string | null;
  unmount: string | null;
  /** Eject the medium or power the drive down for unplugging. */
  eject: string | null;
  /** Bytes free, where the listing knows it (Windows drives). */
  free?: number;
}

export const fmDevices = () => invoke<Device[]>("fm_devices");
export const fmDeviceAction = (action: "mount" | "unmount" | "eject", target: string) =>
  invoke<string>("fm_device_action", { action, target });

/** Folder titles for the path bar; on a Drive mount the path itself is IDs. */
export const fmDisplayNames = (paths: string[]) => invoke<string[]>("fm_display_names", { paths });

let mediaToken: Promise<string> | null = null;

/**
 * A URL the webview can load a file from directly — for `<img>`, `<video>`
 * and the PDF frame — instead of carrying its bytes over IPC. `version`
 * (the modified time) makes a saved-over file load fresh.
 */
export async function fmMediaUrl(path: string, version = 0): Promise<string> {
  mediaToken ??= invoke<string>("fm_media_token").catch((e) => {
    mediaToken = null;
    throw e;
  });
  // The token and the path meet at a slash: a Unix path brings its own, a
  // Windows one (`C:\…`) needs one added.
  const url = convertFileSrc(`${await mediaToken}${path.startsWith("/") ? "" : "/"}${path}`, "fmfile");
  return version ? `${url}?v=${version}` : url;
}

/* ---- git ---------------------------------------------------------------- */

/** VS Code's letters: Modified, Added, Deleted, Renamed, Untracked, Conflict. */
export type GitCode = "M" | "A" | "D" | "R" | "U" | "C";

export interface GitFile {
  /** Absolute. */
  path: string;
  code: GitCode;
  staged: boolean;
}

export interface GitStatus {
  root: string;
  branch: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  files: GitFile[];
}

export interface GitCommit {
  sha: string;
  author: string;
  email: string;
  /** Unix seconds. */
  time: number;
  summary: string;
  uncommitted: boolean;
}

export interface GitBlame {
  commits: GitCommit[];
  /** For each line of the blamed text, its index into `commits`. */
  lines: number[];
}

export interface GitLogEntry {
  sha: string;
  author: string;
  email: string;
  time: number;
  summary: string;
  /** The file's repo-relative path in that commit; it follows renames. */
  path: string;
}

/** Null outside a repository, or on a phone or Drive mount. */
export const fmGitStatus = (dir: string) => invoke<GitStatus | null>("fm_git_status", { dir });
/** The file as staged, which the editor's change bars compare against; null when untracked. */
export const fmGitBase = (path: string) => invoke<string | null>("fm_git_base", { path });
/** Blames `contents` — the unsaved buffer — as the file at `path`. */
export const fmGitBlame = (path: string, contents: string) => invoke<GitBlame>("fm_git_blame", { path, contents });
export const fmGitLog = (path: string) => invoke<GitLogEntry[]>("fm_git_log", { path });
/** The file before and after one commit. */
export const fmGitShow = (path: string, sha: string, rel: string) =>
  invoke<{ before: string; after: string }>("fm_git_show", { path, sha, rel });

/* ---- formatting --------------------------------------------------------- */

export interface Formatted {
  /** Null when no formatter is installed for a language prettier knows; the editor then uses its own copy. */
  contents: string | null;
  tool: string | null;
  /** The project's prettier settings, for that fallback. */
  prettier_config: string | null;
}

/** Formats `contents` as the file at `path`, with the project's own formatter. */
export const fmFormat = (path: string, contents: string) => invoke<Formatted>("fm_format", { path, contents });
