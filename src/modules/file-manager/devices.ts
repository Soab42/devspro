import { useCallback, useEffect, useRef, useState } from "react";
import { Device, DeviceKind, fmDevices } from "./api";
import { Crumb, splitBreadcrumb, tildePath } from "./utils";

/** Drive accounts and network shares get their own sidebar group, under the disks and phones. */
export const REMOTE_KINDS: ReadonlySet<DeviceKind> = new Set(["cloud", "network"]);

/* Plugging in a phone or a stick should show up without a refresh. One
   listing is a single `gio` call, so a few seconds apart is cheap. */
const POLL_MS = 4000;

/** The devices the desktop can see, kept current while the page is open. */
export function useDevices() {
  const [list, setList] = useState<Device[]>([]);
  // Only the newest listing lands: one taken right after a mount must not be
  // overwritten by a poll that started before it.
  const latest = useRef(0);
  const pending = useRef(0);

  const refresh = useCallback(async (): Promise<Device[] | null> => {
    const mine = ++latest.current;
    pending.current += 1;
    try {
      const next = await fmDevices();
      if (mine === latest.current) setList((prev) => (JSON.stringify(prev) === JSON.stringify(next) ? prev : next));
      return next;
    } catch {
      return null;
    } finally {
      pending.current -= 1;
    }
  }, []);

  useEffect(() => {
    void refresh();
    // A slow mount can hold gio up; never stack polls behind it.
    const poll = () => {
      if (document.visibilityState === "visible" && pending.current === 0) void refresh();
    };
    const timer = window.setInterval(poll, POLL_MS);
    window.addEventListener("focus", poll);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", poll);
    };
  }, [refresh]);

  return { list, refresh };
}

/** The mounted device `path` is on, if any — the deepest when one sits inside another. */
export function deviceAt(path: string, devices: Device[]): Device | null {
  let best: Device | null = null;
  for (const d of devices) {
    if (!d.path) continue;
    const inside = path === d.path || path.startsWith(`${d.path}/`);
    if (inside && (!best?.path || d.path.length > best.path.length)) best = d;
  }
  return best;
}

/**
 * The path bar's crumbs. On a device they start at the device, under its
 * name, rather than at `/run/user/1000/gvfs/mtp:host=…`; anywhere a folder
 * has a title of its own (a Drive folder, named by ID on disk) the title is
 * shown instead of the path segment.
 */
export function breadcrumb(path: string, devices: Device[], titles: Readonly<Record<string, string>>): Crumb[] {
  const device = deviceAt(path, devices);
  if (!device?.path) return splitBreadcrumb(path).map((c) => ({ ...c, label: titles[c.path] ?? c.label }));
  const crumbs: Crumb[] = [{ label: device.label, path: device.path }];
  let acc = device.path;
  for (const part of path.slice(device.path.length).split("/").filter(Boolean)) {
    acc += `/${part}`;
    crumbs.push({ label: titles[acc] ?? part, path: acc });
  }
  return crumbs;
}

/** A path as the status line and file tabs show it: from the device's name on a device, `~`-relative elsewhere. */
export function friendlyPath(path: string, devices: Device[], titles: Readonly<Record<string, string>>, home: string): string {
  if (!deviceAt(path, devices)) return tildePath(path, home);
  return breadcrumb(path, devices, titles)
    .map((c) => c.label)
    .join("/");
}
