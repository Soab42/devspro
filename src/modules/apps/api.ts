import { invoke } from "@tauri-apps/api/core";

export interface AppStatus {
  id: string;
  name: string;
  category: string;
  description: string;
  method: "apt" | "npm" | "git";
  homepage: string | null;
  installed: boolean;
  can_auto_install: boolean;
  available_versions: string[];
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
