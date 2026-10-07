import { describe, expect, it } from "vitest";
import type { Finding } from "./api";
import { formatBytes, groupFindings, newScanId, suggestedIds, totalsByRisk } from "./cleaner";

const f = (over: Partial<Finding>): Finding => ({
  id: "x",
  tab: "caches",
  group: "npm",
  label: "X",
  reason: "",
  method: "trash",
  paths: [],
  bytes: 1,
  risk: "safe",
  suggested: false,
  blocked_by: null,
  needs_confirm_text: null,
  ...over,
});

describe("totalsByRisk", () => {
  it("sums bytes per risk and skips blocked findings", () => {
    const totals = totalsByRisk([
      f({ id: "a", bytes: 10, risk: "safe" }),
      f({ id: "b", bytes: 5, risk: "safe" }),
      f({ id: "c", bytes: 7, risk: "review" }),
      f({ id: "d", bytes: 100, risk: "danger", blocked_by: "docker is running" }),
    ]);
    expect(totals).toEqual({ safe: 15, review: 7, danger: 0 });
  });
});

describe("suggestedIds", () => {
  it("returns suggested, unblocked ids", () => {
    expect(
      suggestedIds([
        f({ id: "a", suggested: true }),
        f({ id: "b" }),
        f({ id: "c", suggested: true, blocked_by: "in use" }),
      ]),
    ).toEqual(["a"]);
  });
});

describe("groupFindings", () => {
  it("groups, largest group first and largest finding first", () => {
    const groups = groupFindings([
      f({ id: "a", group: "npm", bytes: 10 }),
      f({ id: "b", group: "pip", bytes: 50 }),
      f({ id: "c", group: "npm", bytes: 30 }),
    ]);
    expect(groups.map((g) => g.group)).toEqual(["pip", "npm"]);
    expect(groups[1].bytes).toBe(40);
    expect(groups[1].findings.map((x) => x.id)).toEqual(["c", "a"]);
  });

  it("is empty for no findings", () => {
    expect(groupFindings([])).toEqual([]);
  });
});

describe("newScanId", () => {
  it("is unique", () => {
    expect(newScanId()).not.toBe(newScanId());
  });
});

describe("formatBytes", () => {
  it("scales units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(500 * 1024 * 1024)).toBe("500 MB");
    expect(formatBytes(3 * 1024 ** 3)).toBe("3.0 GB");
  });
});
