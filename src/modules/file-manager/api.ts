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

export type PlaceKind = "home" | "desktop" | "documents" | "downloads" | "pictures" | "music" | "videos" | "root";

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
export const fmList = (path: string) => invoke<FileEntry[]>("fm_list", { path });
export const fmStat = (path: string) => invoke<FileEntry>("fm_stat", { path });
export const fmMkdir = (dir: string, name: string) => invoke<string>("fm_mkdir", { dir, name });
export const fmCreateFile = (dir: string, name: string) => invoke<string>("fm_create_file", { dir, name });
export const fmRename = (path: string, name: string) => invoke<string>("fm_rename", { path, name });
export const fmTrash = (path: string) => invoke<string>("fm_trash", { path });
export const fmReadFile = (path: string) => invoke<string>("fm_read_file", { path });
export const fmWriteFile = (path: string, contents: string) => invoke<string>("fm_write_file", { path, contents });
export const fmOpen = (path: string) => invoke<void>("fm_open", { path });
