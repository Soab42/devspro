import { useEffect, useRef } from "react";

/**
 * What the current page hands to the window that pops it out.
 *
 * The top-bar "open in a new window" button should carry on from where the
 * user is — the folder they're in, the terminal they're typing in — not
 * start the module from scratch. Pages that have such state register it
 * here with `usePopoutHandoff`; the shell reads it when the button is
 * pressed. The launcher's Ctrl+Enter deliberately doesn't: that is the
 * "give me a fresh one" gesture.
 */
export interface PopoutHandoff {
  /**
   * Query parameters for the new window's route. The page reads them on
   * load and picks up from there (`?path=` for Files, `?host=` for SFTP…).
   */
  params?: Record<string, string>;
  /**
   * The page does the pop-out itself, for things that move rather than copy:
   * a terminal pane, a browser tab. Resolves false to say there was nothing
   * to move, and the shell opens a plain module window instead.
   */
  open?: () => Promise<boolean>;
}

let current: (() => PopoutHandoff | null) | null = null;

/** Only one page is on screen at a time, so there is one slot. */
export function registerPopoutHandoff(get: () => PopoutHandoff | null): () => void {
  current = get;
  return () => {
    if (current === get) current = null;
  };
}

export function currentPopoutHandoff(): PopoutHandoff | null {
  return current?.() ?? null;
}

/** Register the page's hand-off for as long as it is mounted. `get` sees the latest render's state. */
export function usePopoutHandoff(get: () => PopoutHandoff | null): void {
  const ref = useRef(get);
  ref.current = get;
  useEffect(() => registerPopoutHandoff(() => ref.current()), []);
}
