import { describe, expect, it } from "vitest";
import {
  dragSizes,
  equalizeSizes,
  findLeaf,
  leaf,
  leafIds,
  neighbourLeaf,
  removeLeaf,
  setSplitSizes,
  splitLeaf,
  type PaneNode,
} from "./paneTree";

describe("splitLeaf", () => {
  it("turns a lone leaf into a two-way split", () => {
    const tree = splitLeaf(leaf("a"), "a", "row", leaf("b"), "s1");
    expect(tree).toEqual({
      kind: "split",
      id: "s1",
      direction: "row",
      children: [leaf("a"), leaf("b")],
      sizes: [0.5, 0.5],
    });
  });

  it("joins an existing split of the same direction as a sibling", () => {
    const first = splitLeaf(leaf("a"), "a", "row", leaf("b"), "s1");
    const tree = splitLeaf(first, "a", "row", leaf("c"), "s2") as Extract<PaneNode, { kind: "split" }>;
    expect(tree.id).toBe("s1");
    expect(leafIds(tree)).toEqual(["a", "c", "b"]);
    expect(tree.sizes).toEqual([0.25, 0.25, 0.5]);
  });

  it("nests when the direction differs", () => {
    const first = splitLeaf(leaf("a"), "a", "row", leaf("b"), "s1");
    const tree = splitLeaf(first, "b", "column", leaf("c"), "s2") as Extract<PaneNode, { kind: "split" }>;
    expect(tree.children[1]).toEqual({
      kind: "split",
      id: "s2",
      direction: "column",
      children: [leaf("b"), leaf("c")],
      sizes: [0.5, 0.5],
    });
    expect(leafIds(tree)).toEqual(["a", "b", "c"]);
  });
});

describe("removeLeaf", () => {
  const three = splitLeaf(splitLeaf(leaf("a"), "a", "row", leaf("b"), "s1"), "b", "column", leaf("c"), "s2");

  it("collapses a split left with one child", () => {
    expect(removeLeaf(three, "c")).toEqual({
      kind: "split",
      id: "s1",
      direction: "row",
      children: [leaf("a"), leaf("b")],
      sizes: [0.5, 0.5],
    });
  });

  it("gives the removed share to the remaining siblings", () => {
    const wide = setSplitSizes(splitLeaf(three, "a", "row", leaf("d"), "x"), "s1", [0.2, 0.3, 0.5]);
    const after = removeLeaf(wide, "d") as Extract<PaneNode, { kind: "split" }>;
    expect(leafIds(after)).toEqual(["a", "b", "c"]);
    expect(after.sizes[0]).toBeCloseTo(0.2 / 0.7);
    expect(after.sizes[1]).toBeCloseTo(0.5 / 0.7);
  });

  it("returns null when the last pane closes", () => {
    expect(removeLeaf(leaf("a"), "a")).toBeNull();
    expect(removeLeaf(leaf("a"), "zzz")).toEqual(leaf("a"));
  });
});

describe("dragSizes", () => {
  it("trades share between the two neighbours only", () => {
    expect(dragSizes([0.25, 0.25, 0.5], 0, 0.1)).toEqual([0.35, 0.15, 0.5]);
  });

  it("stops at the minimum pane size", () => {
    const sizes = dragSizes([0.5, 0.5], 0, 0.9);
    expect(sizes[1]).toBeCloseTo(0.1);
    expect(sizes[0]).toBeCloseTo(0.9);
    expect(dragSizes([0.5, 0.5], 5, 0.1)).toEqual([0.5, 0.5]);
  });
});

describe("helpers", () => {
  const tree = splitLeaf(splitLeaf(leaf("a"), "a", "row", leaf("b"), "s1"), "b", "column", leaf("c"), "s2");

  it("finds leaves and picks the next neighbour to focus", () => {
    expect(findLeaf(tree, "c")).toEqual(leaf("c"));
    expect(findLeaf(tree, "q")).toBeNull();
    expect(neighbourLeaf(tree, "a")).toBe("b");
    expect(neighbourLeaf(tree, "c")).toBe("b");
    expect(neighbourLeaf(leaf("a"), "a")).toBeNull();
  });

  it("equalises every split", () => {
    const uneven = setSplitSizes(tree, "s1", [0.9, 0.1]);
    const even = equalizeSizes(uneven) as Extract<PaneNode, { kind: "split" }>;
    expect(even.sizes).toEqual([0.5, 0.5]);
  });
});
