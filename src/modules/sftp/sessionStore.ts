import { useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import { SESSIONS_ENDED_EVENT, setBackgroundSessions } from "../../app/backgroundSessions";
import { sftpDisconnect, type SshHost } from "../ssh/api";

/**
 * The SFTP page's open connections, outside React so they outlive the page.
 * Leaving the page keeps them connected (the sidebar shows how many); coming
 * back shows them again, each in the folders it was left in. A connection
 * ends when the user disconnects it, or when DevsPro closes.
 */
export interface SftpSessionInfo {
  id: string;
  host: SshHost;
}

interface SftpState {
  sessions: SftpSessionInfo[];
  active: string | null;
}

let state: SftpState = { sessions: [], active: null };
const listeners = new Set<() => void>();

function set(next: SftpState) {
  state = next;
  setBackgroundSessions("sftp", state.sessions.length);
  for (const listener of listeners) listener();
}

export function useSftpSessions(): SftpState {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => state,
  );
}

export function addSftpSession(session: SftpSessionInfo) {
  set({ sessions: [...state.sessions, session], active: session.id });
}

export function removeSftpSession(id: string) {
  const sessions = state.sessions.filter((s) => s.id !== id);
  const active = state.active === id ? (sessions[sessions.length - 1]?.id ?? null) : state.active;
  places.delete(id);
  set({ sessions, active });
}

export function setActiveSftpSession(id: string | null) {
  set({ ...state, active: id });
}

/** session id -> the folder each pane was showing, for coming back to it. */
const places = new Map<string, { local?: string; remote?: string }>();

export function rememberedPlace(sessionId: string, side: "local" | "remote"): string | null {
  return places.get(sessionId)?.[side] ?? null;
}

export function rememberPlace(sessionId: string, side: "local" | "remote", path: string) {
  places.set(sessionId, { ...places.get(sessionId), [side]: path });
}

// The main window was closed (to the tray): its connections end with it.
try {
  void listen(SESSIONS_ENDED_EVENT, () => {
    for (const s of state.sessions) void sftpDisconnect(s.id).catch(() => undefined);
    places.clear();
    set({ sessions: [], active: null });
  }).catch(() => undefined);
} catch {
  /* no backend (plain `vite` dev, tests) */
}

/** Back to no connections and no remembered folders. For tests. */
export function resetSftpSessions() {
  places.clear();
  set({ sessions: [], active: null });
}
