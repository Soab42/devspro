import { describe, expect, it } from "vitest";
import type { FileEntry } from "./api";
import {
  canGoBack,
  canGoForward,
  fileKind,
  filterEntries,
  historyPush,
  historyStep,
  isEditable,
  parentPath,
  sortEntries,
  splitBreadcrumb,
  tildePath,
} from "./utils";

function entry(over: Partial<FileEntry>): FileEntry {
  return {
    name: "x",
    path: "/x",
    is_dir: false,
    is_symlink: false,
    is_hidden: false,
    size: 0,
    modified: 0,
    permissions: "644",
    ...over,
  };
}

describe("paths", () => {
  it("splits breadcrumbs from the root", () => {
    expect(splitBreadcrumb("/var/log").map((c) => c.path)).toEqual(["/", "/var", "/var/log"]);
    expect(splitBreadcrumb("/")).toEqual([{ label: "/", path: "/" }]);
  });

  it("walks up to the root", () => {
    expect(parentPath("/a/b/")).toBe("/a");
    expect(parentPath("/a")).toBe("/");
    expect(parentPath("/")).toBe("/");
  });

  it("abbreviates the home directory", () => {
    expect(tildePath("/home/u/Projects", "/home/u")).toBe("~/Projects");
    expect(tildePath("/home/u", "/home/u")).toBe("~");
    expect(tildePath("/home/user2", "/home/u")).toBe("/home/user2");
  });
});

describe("kinds", () => {
  it("names common developer files", () => {
    expect(fileKind(entry({ name: "app.tsx" }))).toBe("TypeScript");
    expect(fileKind(entry({ name: ".env.local" }))).toBe("Env file");
    expect(fileKind(entry({ name: "Dockerfile" }))).toBe("Dockerfile");
    expect(fileKind(entry({ name: "README" }))).toBe("Document");
    expect(fileKind(entry({ name: "x.weird" }))).toBe("WEIRD file");
    expect(fileKind(entry({ name: "lib", is_dir: true, is_symlink: true }))).toBe("Folder (link)");
  });

  it("keeps binaries out of the text editor", () => {
    expect(isEditable(entry({ name: "a.png" }))).toBe(false);
    expect(isEditable(entry({ name: "a.tar.gz" }))).toBe(false);
    expect(isEditable(entry({ name: "a.ts" }))).toBe(true);
    expect(isEditable(entry({ name: "d", is_dir: true }))).toBe(false);
  });
});

describe("sorting and filtering", () => {
  const rows = [
    entry({ name: "b.txt", size: 5, modified: 2 }),
    entry({ name: "Zed", is_dir: true }),
    entry({ name: "a.png", size: 9, modified: 1 }),
    entry({ name: ".git", is_dir: true, is_hidden: true }),
  ];

  it("keeps folders first in either direction", () => {
    expect(sortEntries(rows, "name", "asc").map((r) => r.name)).toEqual([".git", "Zed", "a.png", "b.txt"]);
    expect(sortEntries(rows, "size", "desc").map((r) => r.name)).toEqual(["Zed", ".git", "a.png", "b.txt"]);
    expect(sortEntries(rows, "kind", "asc").map((r) => r.name)).toEqual([".git", "Zed", "a.png", "b.txt"]);
  });

  it("hides dotfiles unless asked, and matches case-insensitively", () => {
    expect(filterEntries(rows, "", false).map((r) => r.name)).not.toContain(".git");
    expect(filterEntries(rows, "", true).map((r) => r.name)).toContain(".git");
    expect(filterEntries(rows, "ZED", true).map((r) => r.name)).toEqual(["Zed"]);
  });
});

describe("history", () => {
  it("behaves like a browser's back and forward", () => {
    let h = historyPush({ entries: [], index: -1 }, "/a");
    h = historyPush(h, "/a/b");
    h = historyPush(h, "/a/b");
    expect(h.entries).toEqual(["/a", "/a/b"]);
    expect(canGoBack(h)).toBe(true);
    expect(canGoForward(h)).toBe(false);

    h = historyStep(h, -1);
    expect(h.entries[h.index]).toBe("/a");
    expect(canGoForward(h)).toBe(true);

    h = historyPush(h, "/c");
    expect(h.entries).toEqual(["/a", "/c"]);
    expect(historyStep(h, 1)).toBe(h);
  });
});
