import type { TreeNode } from "../../api";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Placed<T> {
  item: T;
  rect: Rect;
}

/** Worst aspect ratio (>= 1) of a row of areas laid along a side of length `side`. */
function worst(areas: number[], sum: number, side: number): number {
  const max = Math.max(...areas);
  const min = Math.min(...areas);
  const s2 = side * side;
  return Math.max((s2 * max) / (sum * sum), (sum * sum) / (s2 * min));
}

/**
 * Squarified treemap (Bruls, Huizing, van Wijk). Each item gets a rectangle
 * whose area is its share of `bounds`; rectangles tile `bounds` without
 * overlap. Items with no bytes are skipped. Largest first in the result.
 */
export function squarify<T extends { bytes: number }>(items: T[], bounds: Rect): Placed<T>[] {
  const live = items.filter((i) => i.bytes > 0).sort((a, b) => b.bytes - a.bytes);
  const total = live.reduce((s, i) => s + i.bytes, 0);
  if (live.length === 0 || bounds.w <= 0 || bounds.h <= 0 || total <= 0) return [];

  const scale = (bounds.w * bounds.h) / total;
  const out: Placed<T>[] = [];
  let { x, y, w, h } = bounds;
  let i = 0;

  while (i < live.length) {
    const side = Math.min(w, h);
    const row: T[] = [];
    const areas: number[] = [];
    let sum = 0;
    // Grow the row while the worst aspect ratio does not get worse.
    while (i < live.length) {
      const a = live[i].bytes * scale;
      if (row.length > 0 && worst([...areas, a], sum + a, side) > worst(areas, sum, side)) break;
      row.push(live[i]);
      areas.push(a);
      sum += a;
      i += 1;
    }
    const last = i >= live.length;
    if (w >= h) {
      // Row is a column on the left, filling the height.
      const colW = last ? w : sum / h;
      let cy = y;
      row.forEach((item, k) => {
        const ch = last && k === row.length - 1 ? y + h - cy : areas[k] / colW;
        out.push({ item, rect: { x, y: cy, w: colW, h: ch } });
        cy += ch;
      });
      x += colW;
      w -= colW;
    } else {
      const rowH = last ? h : sum / w;
      let cx = x;
      row.forEach((item, k) => {
        const cw = last && k === row.length - 1 ? x + w - cx : areas[k] / rowH;
        out.push({ item, rect: { x: cx, y, w: cw, h: rowH } });
        cx += cw;
      });
      y += rowH;
      h -= rowH;
    }
  }
  return out;
}

export interface Cell {
  node: TreeNode;
  rect: Rect;
  /** 0 for the children of the zoomed folder. */
  depth: number;
  /** A folder drawn as a frame around its own children. */
  frame: boolean;
}

export interface LayoutOptions {
  /** Levels of folders drawn open. */
  maxDepth: number;
  /** Height of the label strip above a framed folder's children. */
  header: number;
  /** Padding inside a frame. */
  pad: number;
  /** A folder is only opened when its rectangle is at least this wide and tall. */
  minOpen: number;
}

export const DEFAULT_LAYOUT: LayoutOptions = { maxDepth: 3, header: 14, pad: 2, minOpen: 48 };

/**
 * Lays out the children of `root` inside `bounds`, opening folders into
 * frames up to `maxDepth` levels. Parents come before their children.
 */
export function layoutTree(root: TreeNode, bounds: Rect, options: LayoutOptions = DEFAULT_LAYOUT): Cell[] {
  const cells: Cell[] = [];
  const place = (node: TreeNode, rect: Rect, depth: number) => {
    // Lay out against what the children add up to, so the frame stays full.
    const kids = node.children.filter((c) => c.bytes > 0);
    for (const p of squarify(kids, rect)) {
      const { item, rect: r } = p;
      const open =
        item.is_dir &&
        item.children.some((c) => c.bytes > 0) &&
        depth + 1 < options.maxDepth &&
        r.w >= options.minOpen &&
        r.h >= options.minOpen;
      cells.push({ node: item, rect: r, depth, frame: open });
      if (open) {
        const inner: Rect = {
          x: r.x + options.pad,
          y: r.y + options.header,
          w: r.w - options.pad * 2,
          h: r.h - options.header - options.pad,
        };
        place(item, inner, depth + 1);
      }
    }
  };
  place(root, bounds, 0);
  return cells;
}

/** Fixed categorical palette, one colour per file kind; readable on light and dark. */
export const KIND_COLORS: Record<string, string> = {
  video: "#c0504d",
  image: "#b86e00",
  audio: "#9b6bc4",
  archive: "#23857a",
  "disk-image": "#4472c4",
  document: "#4f8a38",
  code: "#d4659a",
  "db-dump": "#8c7a3a",
  log: "#7a8896",
  other: "#8a8f98",
};

/** Folders get their own neutral so the colours stay about file kinds. */
export const FOLDER_COLOR = "#5b6b7f";

export const KIND_LABELS: Record<string, string> = {
  video: "Video",
  image: "Image",
  audio: "Audio",
  archive: "Archive",
  "disk-image": "Disk image",
  document: "Document",
  code: "Code",
  "db-dump": "Database",
  log: "Log",
  other: "Other",
};

export function colorOfKind(kind: string): string {
  return KIND_COLORS[kind] ?? KIND_COLORS.other;
}

/** Same grouping as the backend's extension table, for colouring one file by its name. */
const EXT_KIND: Record<string, string> = {};
for (const [kind, list] of Object.entries({
  video: "mp4 mkv avi mov webm wmv flv m4v mpg mpeg",
  image: "png jpg jpeg gif bmp webp svg tif tiff heic raw cr2 nef ico psd",
  audio: "mp3 flac wav ogg m4a aac opus wma",
  archive: "zip tar gz tgz bz2 xz zst 7z rar deb rpm jar",
  "disk-image": "iso img vmdk qcow2 vdi vhd vhdx",
  document: "pdf doc docx xls xlsx ppt pptx odt ods txt md epub rtf csv",
  code: "rs ts tsx js jsx mjs py go java c h cpp hpp cs rb php sh html css json yaml yml toml vue kt swift",
  "db-dump": "sql dump bak sqlite sqlite3 db",
  log: "log",
})) {
  for (const ext of list.split(" ")) EXT_KIND[ext] = kind;
}

export function extensionOf(name: string): string {
  const i = name.lastIndexOf(".");
  return i > 0 && i < name.length - 1 ? name.slice(i + 1).toLowerCase() : "(none)";
}

export function kindOfName(name: string): string {
  return EXT_KIND[extensionOf(name)] ?? "other";
}
