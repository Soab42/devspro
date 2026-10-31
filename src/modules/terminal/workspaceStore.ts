import { setBackgroundSessions } from "../../app/backgroundSessions";
import { EMPTY_WORKSPACE, workspaceReducer, type WorkspaceAction, type WorkspaceState } from "./workspace";

/**
 * A terminal workspace's state outside React, so it outlives the page that
 * shows it. The Terminal page's workspace is kept under one key: leaving the
 * page leaves its shells running (see `persist` on the Terminal component),
 * and coming back shows the same windows, panes and screens. Exits and
 * folder changes that happen while the page is away still land here.
 *
 * Kept in memory only: it is gone when DevsPro closes, as are the shells.
 */
export interface WorkspaceStore {
  get: () => WorkspaceState;
  dispatch: (action: WorkspaceAction) => void;
  subscribe: (listener: () => void) => () => void;
  /** host id -> the follow-along browser's SFTP session for it. */
  sftp: Record<string, string>;
  /** Sessions that already had the follow hook typed into them. */
  hooked: Set<string>;
  /** Sessions whose folder is read in the background instead, with no hook. */
  tracked: Set<string>;
}

export function createWorkspaceStore(initial: WorkspaceState = EMPTY_WORKSPACE, onChange?: (state: WorkspaceState) => void): WorkspaceStore {
  let state = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => state,
    dispatch: (action) => {
      const next = workspaceReducer(state, action);
      if (next === state) return;
      state = next;
      onChange?.(state);
      for (const listener of listeners) listener();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    sftp: {},
    hooked: new Set(),
    tracked: new Set(),
  };
}

/** Shells still running in a workspace. */
export function runningSessions(state: WorkspaceState): number {
  return Object.values(state.sessions).filter((s) => !s.exited).length;
}

const kept = new Map<string, WorkspaceStore>();

/** The workspace kept under `key`, created empty the first time. Reports its running shells as `moduleId`'s badge. */
export function keptWorkspace(key: string, moduleId: string): WorkspaceStore {
  let store = kept.get(key);
  if (!store) {
    store = createWorkspaceStore(EMPTY_WORKSPACE, (state) => setBackgroundSessions(moduleId, runningSessions(state)));
    kept.set(key, store);
  }
  return store;
}
