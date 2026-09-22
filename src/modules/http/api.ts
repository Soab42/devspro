import { invoke } from "@tauri-apps/api/core";

/** `[key, value]` — the Rust side is `Vec<(String, String)>`. */
export type HeaderPair = [string, string];

export interface HttpResponse {
  status: number;
  status_text: string;
  headers: HeaderPair[];
  body: string;
  /** Body was cut at the backend's cap (2 MB); `size_bytes` is the real length. */
  truncated: boolean;
  elapsed_ms: number;
  size_bytes: number;
}

export interface HttpRequestRecord {
  id: string;
  name: string | null;
  method: string;
  url: string;
  headers: HeaderPair[];
  body: string | null;
  created_at: string;
}

export interface SendArgs {
  method: string;
  url: string;
  headers: HeaderPair[];
  body: string | null;
  followRedirects: boolean;
  timeoutMs: number;
}

export const sendRequest = (args: SendArgs) => invoke<HttpResponse>("http_send", { ...args });

export const listHistory = (limit = 100) =>
  invoke<HttpRequestRecord[]>("http_history_list", { limit });

export const saveHistory = (args: Omit<SendArgs, "followRedirects" | "timeoutMs">) =>
  invoke<HttpRequestRecord>("http_history_save", { name: null, ...args });

export const deleteHistory = (id: string) => invoke<void>("http_history_delete", { id });

export const clearHistory = () => invoke<void>("http_history_clear");
