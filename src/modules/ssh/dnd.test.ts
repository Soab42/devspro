import { describe, expect, it } from "vitest";
import { canMoveInto, sameOrigin } from "./dnd";

describe("canMoveInto", () => {
  it("refuses a move into the folder the entry is already in", () => {
    expect(canMoveInto({ path: "/home/me/a.txt", is_dir: false }, "/home/me")).toBe(false);
    expect(canMoveInto({ path: "/a.txt", is_dir: false }, "/")).toBe(false);
  });

  it("refuses a folder into itself or its own subtree", () => {
    expect(canMoveInto({ path: "/home/me/src", is_dir: true }, "/home/me/src")).toBe(false);
    expect(canMoveInto({ path: "/home/me/src", is_dir: true }, "/home/me/src/lib")).toBe(false);
  });

  it("allows a sibling folder whose name merely shares a prefix", () => {
    expect(canMoveInto({ path: "/home/me/src", is_dir: true }, "/home/me/src2")).toBe(true);
  });

  it("allows an ordinary move", () => {
    expect(canMoveInto({ path: "/home/me/a.txt", is_dir: false }, "/home/me/docs")).toBe(true);
    expect(canMoveInto({ path: "/home/me/docs/a.txt", is_dir: false }, "/home/me")).toBe(true);
  });
});

describe("sameOrigin", () => {
  it("treats every local view as one filesystem", () => {
    expect(sameOrigin({ source: "local" }, { source: "local", session: "x" })).toBe(true);
  });

  it("separates remote hosts by session", () => {
    expect(sameOrigin({ source: "remote", session: "a" }, { source: "remote", session: "a" })).toBe(true);
    expect(sameOrigin({ source: "remote", session: "a" }, { source: "remote", session: "b" })).toBe(false);
    expect(sameOrigin({ source: "local" }, { source: "remote", session: "a" })).toBe(false);
  });
});
