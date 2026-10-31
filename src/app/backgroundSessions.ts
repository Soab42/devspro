import { useSyncExternalStore } from "react";

/**
 * Live sessions per module, for the sidebar's badge: a terminal or an SFTP
 * connection that keeps running after its page is left. A module reports its
 * own count; nothing here starts or stops anything.
 *
 * Every such session ends when DevsPro closes (the window's X, even with
 * "keep running in tray" on), so the badge never stands for something left
 * running behind the user's back.
 */
const counts = new Map<string, number>();
const listeners = new Set<() => void>();

export function setBackgroundSessions(moduleId: string, count: number) {
  if ((counts.get(moduleId) ?? 0) === count) return;
  if (count > 0) counts.set(moduleId, count);
  else counts.delete(moduleId);
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useBackgroundSessions(moduleId: string): number {
  return useSyncExternalStore(subscribe, () => counts.get(moduleId) ?? 0);
}

/** Emitted by the backend when the main window closes and the sessions are ended. */
export const SESSIONS_ENDED_EVENT = "app:sessions-ended";
