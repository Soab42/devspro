import { useEffect, useRef } from "react";
import { parentPath } from "./utils";

/**
 * Drag and drop of files between the app's own file views, and in from the
 * desktop.
 *
 * In-app drags ride on HTML5 drag events with a private MIME type carrying
 * the file's origin (which filesystem, which SFTP session) and path. The
 * payload is also kept in a module variable for the duration of the drag,
 * because `dataTransfer.getData` is empty during `dragover` and the drop
 * targets want to know, while hovering, whether a drop would do anything.
 *
 * Desktop drags never reach the DOM: Tauri intercepts them and reports them
 * as webview events with a position and a list of paths, so those are hit
 * tested against the page by hand — see `useOsFileDrop` and `dirAtPoint`.
 */
export const FILE_DRAG_TYPE = "application/x-devspro-file";

export interface FileDragPayload {
  /** Which filesystem the path lives on. */
  source: "local" | "remote";
  /** The SFTP session a remote path belongs to. */
  session?: string;
  path: string;
  name: string;
  is_dir: boolean;
}

let current: FileDragPayload | null = null;

/** The in-app drag in progress in this window, if any. */
export function currentFileDrag(): FileDragPayload | null {
  return current;
}

export function startFileDrag(dt: DataTransfer, payload: FileDragPayload) {
  current = payload;
  dt.setData(FILE_DRAG_TYPE, JSON.stringify(payload));
  // So a drop on anything that only understands text (the terminal, an
  // input) still gets the path.
  dt.setData("text/plain", payload.path);
  dt.effectAllowed = "copyMove";
}

export function endFileDrag() {
  current = null;
}

export function hasFileDrag(dt: DataTransfer): boolean {
  return Array.from(dt.types).includes(FILE_DRAG_TYPE);
}

export function readFileDrag(dt: DataTransfer): FileDragPayload | null {
  const raw = dt.getData(FILE_DRAG_TYPE);
  if (!raw) return current;
  try {
    const p = JSON.parse(raw) as Partial<FileDragPayload>;
    if (
      (p.source === "local" || p.source === "remote") &&
      typeof p.path === "string" &&
      typeof p.name === "string"
    ) {
      return {
        source: p.source,
        session: p.session,
        path: p.path,
        name: p.name,
        is_dir: Boolean(p.is_dir),
      };
    }
  } catch {
    /* not ours */
  }
  return current;
}

/** True when the two payload origins are the same filesystem. */
export function sameOrigin(
  a: Pick<FileDragPayload, "source" | "session">,
  b: Pick<FileDragPayload, "source" | "session">,
): boolean {
  return a.source === b.source && (a.source === "local" || a.session === b.session);
}

/**
 * Whether moving `entry` into `dir` on the same filesystem changes anything:
 * not when it is already there, and never a folder into itself or a
 * descendant of itself.
 */
export function canMoveInto(entry: Pick<FileDragPayload, "path" | "is_dir">, dir: string): boolean {
  if (parentPath(entry.path) === dir) return false;
  if (entry.is_dir && (dir === entry.path || dir.startsWith(`${entry.path}/`))) return false;
  return true;
}

/**
 * The folder a drop at `target` lands in: the nearest row marked with
 * `data-drop-dir`, else `fallback` (the view's current folder).
 */
export function dropDirOf(target: EventTarget | null, fallback: string | null): string | null {
  if (!(target instanceof Element)) return fallback;
  return target.closest<HTMLElement>("[data-drop-dir]")?.dataset.dropDir ?? fallback;
}

export interface Point {
  x: number;
  y: number;
}

/**
 * The same, for a point on the page: null when the point is outside
 * `container`, else the folder row under it or `fallback`.
 */
export function dirAtPoint(
  container: HTMLElement | null,
  point: Point,
  fallback: string | null,
): string | null {
  if (!container) return null;
  const el = document.elementFromPoint(point.x, point.y);
  if (!el || !container.contains(el)) return null;
  return dropDirOf(el, fallback);
}

/** The element under a point, matched against a selector inside `container`. */
export function elementAtPoint<E extends HTMLElement>(
  container: HTMLElement | null,
  point: Point,
  selector: string,
): E | null {
  if (!container) return null;
  const el = document.elementFromPoint(point.x, point.y);
  if (!el || !container.contains(el)) return null;
  return el.closest<E>(selector);
}

export interface OsFileDropHandlers {
  /** A desktop drag is over the window at this point; null when it has left. */
  onHover: (point: Point | null) => void;
  onDrop: (paths: string[], point: Point) => void;
}

function inTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

/**
 * Files dragged in from the desktop. Tauri consumes the native drag before
 * the DOM sees it and reports it on the webview instead, in physical
 * pixels; this converts to page coordinates and hands the handlers the
 * point so they can hit test their own area. Does nothing outside Tauri.
 */
export function useOsFileDrop(handlers: OsFileDropHandlers) {
  const ref = useRef(handlers);
  ref.current = handlers;

  useEffect(() => {
    if (!inTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((event) => {
          const p = event.payload;
          if (p.type === "leave") {
            ref.current.onHover(null);
            return;
          }
          const scale = window.devicePixelRatio || 1;
          const point = { x: p.position.x / scale, y: p.position.y / scale };
          if (p.type === "drop") {
            ref.current.onHover(null);
            if (p.paths.length > 0) ref.current.onDrop(p.paths, point);
          } else {
            ref.current.onHover(point);
          }
        }),
      )
      .then((f) => {
        if (cancelled) f();
        else unlisten = f;
      })
      .catch(() => {
        /* no webview API: not running in Tauri */
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
