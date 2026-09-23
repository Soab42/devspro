/**
 * A tmux-style split tree: leaves are panes, inner nodes split their
 * children along one axis with a fractional size each.
 *
 * Pure functions over an immutable tree. Nothing here knows what a pane
 * shows — the terminal workspace keeps its own map from leaf id to session,
 * so a pane can be moved around the tree without its content noticing.
 */

export type SplitDirection = "row" | "column";

export interface PaneLeaf {
  kind: "leaf";
  id: string;
}

export interface PaneSplit {
  kind: "split";
  id: string;
  direction: SplitDirection;
  children: PaneNode[];
  /** One fraction per child, summing to 1. */
  sizes: number[];
}

export type PaneNode = PaneLeaf | PaneSplit;

/** The smallest share a pane can be dragged down to, so nothing vanishes. */
export const MIN_PANE_FRACTION = 0.1;

export function leaf(id: string): PaneLeaf {
  return { kind: "leaf", id };
}

export function leafIds(node: PaneNode): string[] {
  if (node.kind === "leaf") return [node.id];
  return node.children.flatMap(leafIds);
}

export function findLeaf(node: PaneNode, id: string): PaneLeaf | null {
  if (node.kind === "leaf") return node.id === id ? node : null;
  for (const child of node.children) {
    const found = findLeaf(child, id);
    if (found) return found;
  }
  return null;
}

function equalShares(n: number): number[] {
  return Array.from({ length: n }, () => 1 / n);
}

/**
 * Splits `targetId` so the new leaf sits after it. When the target already
 * lives in a split of the same direction, the new pane joins that split as
 * a sibling (tmux does the same: repeatedly splitting sideways yields one
 * row of panes, not a nested ladder). The target's share is halved between
 * itself and the newcomer.
 */
export function splitLeaf(
  node: PaneNode,
  targetId: string,
  direction: SplitDirection,
  newLeaf: PaneLeaf,
  splitId: string,
): PaneNode {
  if (node.kind === "leaf") {
    if (node.id !== targetId) return node;
    return { kind: "split", id: splitId, direction, children: [node, newLeaf], sizes: [0.5, 0.5] };
  }
  const index = node.children.findIndex((c) => c.kind === "leaf" && c.id === targetId);
  if (index >= 0 && node.direction === direction) {
    const share = node.sizes[index] / 2;
    const children = [...node.children];
    children.splice(index + 1, 0, newLeaf);
    const sizes = [...node.sizes];
    sizes.splice(index, 1, share, share);
    return { ...node, children, sizes };
  }
  return {
    ...node,
    children: node.children.map((c) => splitLeaf(c, targetId, direction, newLeaf, splitId)),
  };
}

/**
 * Removes a leaf. Its share goes to its neighbours in proportion; a split
 * left with one child collapses into that child. Returns null when the
 * last pane goes.
 */
export function removeLeaf(node: PaneNode, id: string): PaneNode | null {
  if (node.kind === "leaf") return node.id === id ? null : node;
  const kept: PaneNode[] = [];
  const sizes: number[] = [];
  node.children.forEach((child, i) => {
    const next = removeLeaf(child, id);
    if (next) {
      kept.push(next);
      sizes.push(node.sizes[i]);
    }
  });
  if (kept.length === 0) return null;
  if (kept.length === 1) return kept[0];
  const total = sizes.reduce((a, b) => a + b, 0);
  return { ...node, children: kept, sizes: sizes.map((s) => s / total) };
}

/** Replaces the sizes of one split, after a divider drag. */
export function setSplitSizes(node: PaneNode, splitId: string, sizes: number[]): PaneNode {
  if (node.kind === "leaf") return node;
  if (node.id === splitId) return { ...node, sizes };
  return { ...node, children: node.children.map((c) => setSplitSizes(c, splitId, sizes)) };
}

/** Gives every child of every split an equal share. */
export function equalizeSizes(node: PaneNode): PaneNode {
  if (node.kind === "leaf") return node;
  return { ...node, sizes: equalShares(node.children.length), children: node.children.map(equalizeSizes) };
}

/**
 * The sizes after dragging the divider between children `index` and
 * `index + 1` by `delta`, a fraction of the split's full extent. The two
 * neighbours trade share; every other child is untouched. Neither can go
 * below the minimum, so a drag past the edge just stops there.
 */
export function dragSizes(sizes: number[], index: number, delta: number): number[] {
  if (index < 0 || index >= sizes.length - 1) return sizes;
  const left = sizes[index];
  const right = sizes[index + 1];
  const clamped = Math.max(MIN_PANE_FRACTION - left, Math.min(right - MIN_PANE_FRACTION, delta));
  const next = [...sizes];
  next[index] = left + clamped;
  next[index + 1] = right - clamped;
  return next;
}

/**
 * The leaf to focus after `id` closes: the one that appears next in reading
 * order, else the previous one. Null when nothing is left.
 */
export function neighbourLeaf(node: PaneNode, id: string): string | null {
  const ids = leafIds(node);
  const index = ids.indexOf(id);
  if (index < 0) return ids[0] ?? null;
  return ids[index + 1] ?? ids[index - 1] ?? null;
}
