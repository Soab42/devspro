import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** How this copy was installed; decides which package an update needs. */
export type InstallKind = "deb" | "rpm" | "nsis";

export interface UpdateCheck {
  current_version: string;
  latest_version: string;
  /** The feed's version is newer than this one. */
  available: boolean;
  /** ...and there is a package for how this copy was installed. */
  installable: boolean;
  /** CHANGELOG.md's section for that version: "### Group" over "- " bullets. */
  notes: string;
  pub_date: string | null;
  checked_at: string;
}

export interface UpdaterStatus {
  current_version: string;
  install_kind: InstallKind | null;
  /** Why this copy can't install an update itself; null when it can. */
  unsupported: string | null;
  auto_check: boolean;
  last_check: UpdateCheck | null;
  downloading: boolean;
  /** The version downloaded and verified, ready to install. */
  ready: string | null;
  feed_url: string;
  release_notes_url: string;
}

export interface DownloadProgress {
  downloaded: number;
  total: number | null;
}

/** Read by the Rust side before each background check; anything but "false" is on. */
export const AUTO_CHECK_KEY = "updates_auto_check";
export const AVAILABLE_EVENT = "updater://available";
export const PROGRESS_EVENT = "updater://progress";
/** Where the topbar pill sends you. */
export const UPDATES_SETTINGS_PATH = "/settings?section=updates";

export const updaterStatus = () => invoke<UpdaterStatus>("updater_status");
export const checkForUpdates = () => invoke<UpdateCheck>("updater_check");
/** Resolves with the version now ready to install. */
export const downloadUpdate = () => invoke<string>("updater_download");
/** Prompts for elevation, installs, then the app restarts — it doesn't resolve on success. */
export const installUpdate = () => invoke<void>("updater_install");

export const onUpdateAvailable = (fn: (check: UpdateCheck) => void) =>
  listen<UpdateCheck>(AVAILABLE_EVENT, (e) => fn(e.payload));
export const onDownloadProgress = (fn: (progress: DownloadProgress) => void) =>
  listen<DownloadProgress>(PROGRESS_EVENT, (e) => fn(e.payload));

export interface NotesGroup {
  /** The "### " heading; null for bullets before any heading. */
  title: string | null;
  items: string[];
}

/**
 * Release notes as `scripts/release.mjs` copies them out of CHANGELOG.md.
 * Anything that isn't a heading or a bullet continues the bullet above it,
 * so a wrapped line stays with its item.
 */
export function parseNotes(notes: string): NotesGroup[] {
  const groups: NotesGroup[] = [];
  let current: NotesGroup | null = null;
  for (const raw of notes.split("\n")) {
    const line = raw.trim();
    if (!line) continue;
    if (line.startsWith("#")) {
      current = { title: line.replace(/^#+\s*/, ""), items: [] };
      groups.push(current);
      continue;
    }
    if (!current) {
      current = { title: null, items: [] };
      groups.push(current);
    }
    if (line.startsWith("- ") || line.startsWith("* ")) {
      current.items.push(line.slice(2).trim());
    } else if (current.items.length > 0) {
      current.items[current.items.length - 1] += ` ${line}`;
    } else {
      current.items.push(line);
    }
  }
  return groups.filter((g) => g.items.length > 0);
}

/** "12.3 MB of 40.1 MB", or just the first half when the size isn't known. */
export function formatProgress({ downloaded, total }: DownloadProgress): string {
  const mb = (bytes: number) => `${(bytes / 1_000_000).toFixed(1)} MB`;
  return total ? `${mb(downloaded)} of ${mb(total)}` : mb(downloaded);
}
