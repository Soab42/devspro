import { invoke } from "@tauri-apps/api/core";

export interface AppStatus {
  id: string;
  name: string;
  category: string;
  description: string;
  /** Display label for how this app installs. Not a closed set: the elevated mechanism is
   *  named after whichever package manager this OS uses (`apt`, `dnf`, `pacman`, `winget`),
   *  alongside `npm`, `git` and `binary`. */
  method: string;
  homepage: string | null;
  installed: boolean;
  can_auto_install: boolean;
  available_versions: string[];
  /** Versions come from `liveVersions` when the picker opens, not from here. */
  live_versions: boolean;
}

export interface InstallOutcome {
  id: string;
  success: boolean;
  message: string;
}

export interface InstallSelection {
  id: string;
  version: string | null;
}

export const listApps = () => invoke<AppStatus[]>("apps_list");
export const installBatch = (selections: InstallSelection[]) =>
  invoke<InstallOutcome[]>("apps_install_batch", { selections });

// One fetch per app per session: the list is a network round trip of a few
// seconds, and it doesn't change while the page is open. A failed fetch is
// forgotten so the next open tries again.
const liveVersionCache = new Map<string, Promise<string[]>>();

/** Every published version of `id`, newest first. */
export function liveVersions(id: string): Promise<string[]> {
  let pending = liveVersionCache.get(id);
  if (!pending) {
    pending = invoke<string[]>("apps_live_versions", { id });
    pending.catch(() => liveVersionCache.delete(id));
    liveVersionCache.set(id, pending);
  }
  return pending;
}
