import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Risk = "safe" | "review" | "danger";

export interface Finding {
  /** Stable within a scan; clean commands take these back. */
  id: string;
  tab: string;
  group: string;
  label: string;
  reason: string;
  method: string;
  paths: string[];
  bytes: number;
  risk: Risk;
  /** Pre-ticked in the table. */
  suggested: boolean;
  /** Set when it cannot be cleaned right now. */
  blocked_by: string | null;
  /** The user must type this exactly before it is cleaned. */
  needs_confirm_text: string | null;
}

export interface CleanOutcome {
  id: string;
  ok: boolean;
  message: string;
  bytes_freed: number;
}

export interface CleanerSettings {
  scan_roots: string[];
  project_roots: string[];
  exclusions: string[];
  large_file_min_bytes: number;
  project_idle_days: number;
}

export interface HistoryEntry {
  id: number;
  /** RFC3339. */
  at: string;
  tab: string;
  label: string;
  bytes_freed: number;
  ok: boolean;
  message: string;
}

export interface ScanProgress {
  scan_id: string;
  tab: string;
  entries: number;
  bytes: number;
  current: string;
}

export interface DockerScan {
  available: boolean;
  message: string | null;
  findings: Finding[];
}

export type LargeFileKind =
  | "video"
  | "disk-image"
  | "archive"
  | "db-dump"
  | "log"
  | "core-dump"
  | "other";

export interface LargeFile {
  path: string;
  bytes: number;
  /** Unix seconds. */
  modified: number;
  /** Unix seconds. */
  accessed: number;
  kind: LargeFileKind;
}

export interface FolderEntry {
  path: string;
  name: string;
  bytes: number;
  is_dir: boolean;
  entries: number;
}

export interface ProjectJunk {
  project_path: string;
  project_name: string;
  artifact_path: string;
  artifact_kind: string;
  bytes: number;
  /** Unix seconds. */
  last_activity: number | null;
  idle_days: number | null;
  suggested: boolean;
}

export interface TreeNode {
  name: string;
  path: string;
  bytes: number;
  files: number;
  is_dir: boolean;
  children: TreeNode[];
}

export interface ExtStat {
  ext: string;
  kind: string;
  bytes: number;
  files: number;
}

export interface StorageTree {
  root: TreeNode;
  extensions: ExtStat[];
  total_bytes: number;
  files: number;
  truncated: boolean;
}

export const getSettings = () => invoke<CleanerSettings>("cleaner_settings_get");

export const setSettings = (settings: CleanerSettings) =>
  invoke<CleanerSettings>("cleaner_settings_set", { settings });

export const listHistory = (limit?: number) =>
  invoke<HistoryEntry[]>("cleaner_history_list", { limit: limit ?? null });

export const clearHistory = () => invoke<void>("cleaner_history_clear");

export const cancelScan = (scanId: string) => invoke<void>("cleaner_cancel_scan", { scanId });

export const scanCaches = (scanId: string) => invoke<Finding[]>("cleaner_scan_caches", { scanId });

export const cleanCaches = (ids: string[]) => invoke<CleanOutcome[]>("cleaner_clean_caches", { ids });

export const scanSystem = (scanId: string) => invoke<Finding[]>("cleaner_scan_system", { scanId });

export const cleanSystem = (ids: string[]) => invoke<CleanOutcome[]>("cleaner_clean_system", { ids });

export const scanDocker = (scanId: string) => invoke<DockerScan>("cleaner_scan_docker", { scanId });

export const cleanDocker = (ids: string[], confirmed: string[]) =>
  invoke<CleanOutcome[]>("cleaner_clean_docker", { ids, confirmed });

export const scanLargeFiles = (scanId: string, minBytes: number, olderThanDays: number | null) =>
  invoke<LargeFile[]>("cleaner_scan_large_files", { scanId, minBytes, olderThanDays });

export const folderSizes = (scanId: string, path: string) =>
  invoke<FolderEntry[]>("cleaner_folder_sizes", { scanId, path });

export const removeFiles = (paths: string[], permanent: boolean) =>
  invoke<CleanOutcome[]>("cleaner_remove_files", { paths, permanent });

export const scanProjects = (scanId: string) => invoke<ProjectJunk[]>("cleaner_scan_projects", { scanId });

export const cleanProjects = (artifactPaths: string[]) =>
  invoke<CleanOutcome[]>("cleaner_clean_projects", { artifactPaths });

export const scanTree = (scanId: string, root: string) =>
  invoke<StorageTree>("cleaner_scan_tree", { scanId, root });

export const onScanProgress = (handler: (progress: ScanProgress) => void) =>
  listen<ScanProgress>("cleaner://progress", (e) => handler(e.payload));
