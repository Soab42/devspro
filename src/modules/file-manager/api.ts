import { invoke } from "@tauri-apps/api/core";

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
