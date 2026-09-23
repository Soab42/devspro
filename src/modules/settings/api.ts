import { invoke } from "@tauri-apps/api/core";

export const getSetting = (key: string) =>
  invoke<string | null>("settings_get", { key });
export const setSetting = (key: string, value: string) =>
  invoke<void>("settings_set", { key, value });

/** The command "Open in editor" runs, with the file path appended. */
export const EXTERNAL_EDITOR_KEY = "external_editor";
export const DEFAULT_EXTERNAL_EDITOR = "code";

export const getExternalEditor = async (): Promise<string> =>
  (await getSetting(EXTERNAL_EDITOR_KEY).catch(() => null))?.trim() ||
  DEFAULT_EXTERNAL_EDITOR;

export const autostartSetEnabled = (enabled: boolean) =>
  invoke<void>("autostart_set_enabled", { enabled });
export const autostartIsEnabled = () => invoke<boolean>("autostart_is_enabled");

/* ---- Backup & restore ---- */

export type BackupSection = "sites" | "workers" | "settings" | "ca";
export type BackupMode = "merge" | "replace";

export interface BackupSectionCount {
  section: BackupSection;
  count: number;
}

export interface BackupInspection {
  exported_at: string;
  version: number;
  sections: BackupSectionCount[];
}

export interface BackupImportSummary {
  imported: BackupSectionCount[];
  ca_backups: string[];
  needs_apply: boolean;
  note: string;
}

export const backupExport = (path: string) =>
  invoke<BackupInspection>("backup_export", { path });
export const backupInspect = (path: string) =>
  invoke<BackupInspection>("backup_inspect", { path });
export const backupImport = (
  path: string,
  sections: BackupSection[],
  mode: BackupMode,
) => invoke<BackupImportSummary>("backup_import", { path, sections, mode });
