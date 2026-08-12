import { invoke } from "@tauri-apps/api/core";

export const getSetting = (key: string) => invoke<string | null>("settings_get", { key });
export const setSetting = (key: string, value: string) => invoke<void>("settings_set", { key, value });

export const autostartSetEnabled = (enabled: boolean) => invoke<void>("autostart_set_enabled", { enabled });
export const autostartIsEnabled = () => invoke<boolean>("autostart_is_enabled");
