import { invoke } from "@tauri-apps/api/core";

export const readEnvFile = (path: string) => invoke<string>("env_diff_read_file", { path });

/** Overwrites a file that already exists; never creates one. */
export const writeEnvFile = (path: string, contents: string) =>
  invoke<string>("env_diff_write_file", { path, contents });
