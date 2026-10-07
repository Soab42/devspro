import { describe, expect, it } from "vitest";
import type { TreeNode } from "../../api";
import { layoutTree, squarify, type Rect } from "./treemap";

const BOUNDS: Rect = { x: 10, y: 20, w: 400, h: 250 };
const item = (bytes: number) => ({ bytes });
const overlap = (a: Rect, b: Rect) =>
  Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x) > 1e-6 &&
  Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y) > 1e-6;

describe("squarify", () => {
  const items = [50, 30, 20, 10, 10, 5, 5, 1].map(item);
  const placed = squarify(items, BOUNDS);

  it("gives every rectangle an area proportional to its bytes", () => {
    const total = items.reduce((s, i) => s + i.bytes, 0);
    for (const p of placed) {
      expect(p.rect.w * p.rect.h).toBeCloseTo((p.item.bytes / total) * BOUNDS.w * BOUNDS.h, 4);
    }
    expect(placed).toHaveLength(items.length);
  });

  it("keeps every rectangle inside the bounds", () => {
    for (const { rect: r } of placed) {
      expect(r.x).toBeGreaterThanOrEqual(BOUNDS.x - 1e-6);
      expect(r.y).toBeGreaterThanOrEqual(BOUNDS.y - 1e-6);
      expect(r.x + r.w).toBeLessThanOrEqual(BOUNDS.x + BOUNDS.w + 1e-6);
      expect(r.y + r.h).toBeLessThanOrEqual(BOUNDS.y + BOUNDS.h + 1e-6);
    }
  });

  it("does not overlap", () => {
    for (let i = 0; i < placed.length; i++) {
      for (let j = i + 1; j < placed.length; j++) expect(overlap(placed[i].rect, placed[j].rect)).toBe(false);
    }
  });

  it("skips zero-byte items and handles empty input", () => {
    expect(squarify([item(0), item(5), item(0)], BOUNDS)).toHaveLength(1);
    expect(squarify([], BOUNDS)).toEqual([]);
    expect(squarify([item(0)], BOUNDS)).toEqual([]);
    expect(squarify([item(5)], { x: 0, y: 0, w: 0, h: 10 })).toEqual([]);
  });

  it("fills a single item's bounds", () => {
    expect(squarify([item(7)], BOUNDS)[0].rect).toEqual(BOUNDS);
  });
});

const node = (name: string, bytes: number, children: TreeNode[] = []): TreeNode => ({
  name,
  path: `/${name}`,
  bytes,
  files: 1,
  is_dir: children.length > 0,
  children,
});

describe("layoutTree", () => {
  const root = node("root", 100, [
    node("big", 70, [node("a", 40), node("b", 30)]),
    node("small", 30, [node("c", 30)]),
    node("empty", 0),
  ]);

  it("opens folders into frames and lists parents before children", () => {
    const cells = layoutTree(root, { x: 0, y: 0, w: 400, h: 300 });
    const names = cells.map((c) => c.node.name);
    expect(names).not.toContain("empty");
    expect(cells.find((c) => c.node.name === "big")?.frame).toBe(true);
    expect(names.indexOf("big")).toBeLessThan(names.indexOf("a"));
    expect(cells.find((c) => c.node.name === "a")?.depth).toBe(1);
  });

  it("keeps children inside their frame", () => {
    const cells = layoutTree(root, { x: 0, y: 0, w: 400, h: 300 });
    const big = cells.find((c) => c.node.name === "big")!.rect;
    for (const n of ["a", "b"]) {
      const r = cells.find((c) => c.node.name === n)!.rect;
      expect(r.x).toBeGreaterThanOrEqual(big.x);
      expect(r.y).toBeGreaterThanOrEqual(big.y);
      expect(r.x + r.w).toBeLessThanOrEqual(big.x + big.w + 1e-6);
      expect(r.y + r.h).toBeLessThanOrEqual(big.y + big.h + 1e-6);
    }
  });

  it("does not open folders that are too small or too deep", () => {
    const tiny = layoutTree(root, { x: 0, y: 0, w: 60, h: 40 });
    expect(tiny.every((c) => !c.frame)).toBe(true);
    const flat = layoutTree(root, { x: 0, y: 0, w: 400, h: 300 }, { maxDepth: 1, header: 14, pad: 2, minOpen: 48 });
    expect(flat.every((c) => !c.frame && c.depth === 0)).toBe(true);
  });
});
