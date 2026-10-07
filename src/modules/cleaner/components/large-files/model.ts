import type { Finding } from "../../api";
import type { LargeFile, LargeFileKind } from "../../api";

const MB = 1024 * 1024;
const GB = 1024 * MB;

export const SIZE_OPTIONS = [
  { value: String(100 * MB), label: "100 MB" },
  { value: String(500 * MB), label: "500 MB" },
  { value: String(GB), label: "1 GB" },
  { value: String(5 * GB), label: "5 GB" },
];

/** The threshold option closest to the settings value, so the control always shows one selected. */
export function nearestSize(bytes: number): string {
  let best = SIZE_OPTIONS[0].value;
  for (const o of SIZE_OPTIONS) {
    if (Math.abs(Number(o.value) - bytes) < Math.abs(Number(best) - bytes)) best = o.value;
  }
  return best;
}

export const AGE_OPTIONS = [
  { value: "0", label: "Any age" },
  { value: "30", label: "Older than 30 days" },
  { value: "90", label: "Older than 90 days" },
  { value: "180", label: "Older than 6 months" },
  { value: "365", label: "Older than a year" },
];

export const KIND_LABEL: Record<LargeFileKind, string> = {
  video: "Video",
  "disk-image": "Disk image",
  archive: "Archive",
  "db-dump": "Database dump",
  log: "Log",
  "core-dump": "Core dump",
  other: "Other",
};

export const KIND_OPTIONS = [
  { value: "all", label: "All kinds" },
  ...(Object.keys(KIND_LABEL) as LargeFileKind[]).map((k) => ({ value: k, label: KIND_LABEL[k] })),
];

/** What the filters leave of a scan's results, by size, kind and age. */
export function filterFiles(files: LargeFile[], minBytes: number, kind: string, olderThanDays: number, nowSecs: number): LargeFile[] {
  const cutoff = olderThanDays > 0 ? nowSecs - olderThanDays * 86_400 : null;
  return files.filter(
    (f) => f.bytes >= minBytes && (kind === "all" || f.kind === kind) && (cutoff === null || f.modified < cutoff),
  );
}

export const baseName = (path: string) => path.slice(path.lastIndexOf("/") + 1) || path;

export const parentOf = (path: string) => path.slice(0, Math.max(path.lastIndexOf("/"), 1));

/** Seconds since the epoch as a local date, or a dash when unknown. */
export function formatDate(secs: number): string {
  return secs > 0 ? new Date(secs * 1000).toLocaleDateString() : "-";
}

/** The review modal takes findings; a file is one with its own path. */
export function toFinding(file: LargeFile, permanent: boolean): Finding {
  return {
    id: file.path,
    tab: "large-files",
    group: KIND_LABEL[file.kind],
    label: baseName(file.path),
    reason: permanent ? "Deleted for good; it will not be in the Trash." : "Goes to the Trash; restore it from there.",
    method: permanent ? "delete permanently" : "trash",
    paths: [file.path],
    bytes: file.bytes,
    risk: permanent ? "danger" : "review",
    suggested: false,
    blocked_by: null,
    needs_confirm_text: null,
  };
}

export interface Crumb {
  name: string;
  path: string;
}

/** The root, then each folder down to `path`. */
export function crumbsFor(root: string, path: string): Crumb[] {
  const crumbs: Crumb[] = [{ name: root, path: root }];
  const rest = path.startsWith(root) ? path.slice(root.length).split("/").filter(Boolean) : [];
  let at = root;
  for (const part of rest) {
    at = `${at.replace(/\/$/, "")}/${part}`;
    crumbs.push({ name: part, path: at });
  }
  return crumbs;
}
