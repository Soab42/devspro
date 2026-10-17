import { describe, expect, it } from "vitest";
import type { FileEntry } from "./api";
import {
  canGoBack,
  canGoForward,
  clickSelect,
  columnChain,
  flattenTree,
  planMove,
  fileKind,
  filterEntries,
  historyPush,
  historyStep,
  isEditable,
  parentPath,
  sortEntries,
  splitBreadcrumb,
  THUMB_MAX,
  THUMB_MIN,
  clampThumb,
  relativeTime,
  thumbRenderSize,
  tildePath,
  viewerKind,
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

  it("picks the viewer a file opens in", () => {
    expect(viewerKind(entry({ name: "main.rs" }))).toBe("text");
    expect(viewerKind(entry({ name: "Dockerfile" }))).toBe("text");
    expect(viewerKind(entry({ name: "photo.JPG" }))).toBe("image");
    expect(viewerKind(entry({ name: "clip.mkv" }))).toBe("video");
    expect(viewerKind(entry({ name: "song.flac" }))).toBe("audio");
    expect(viewerKind(entry({ name: "spec.pdf" }))).toBe("pdf");
    expect(viewerKind(entry({ name: "report.xlsx" }))).toBe("other");
    expect(viewerKind(entry({ name: "scan.tiff" }))).toBe("other");
    expect(viewerKind(entry({ name: "src", is_dir: true }))).toBeNull();
    // A Google Doc on a Drive mount has no bytes, only a web page.
    expect(viewerKind(entry({ name: "Plan.md", is_symlink: true, link: "https://docs.google.com/document/d/1" }))).toBeNull();
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

describe("columnChain", () => {
  const places = ["/home/me", "/home/me/Downloads", "/"];

  it("starts at the deepest place containing the path", () => {
    expect(columnChain("/home/me/projects/app", places)).toEqual(["/home/me", "/home/me/projects", "/home/me/projects/app"]);
    expect(columnChain("/home/me/Downloads/x", places)).toEqual(["/home/me/Downloads", "/home/me/Downloads/x"]);
  });

  it("is a single column at the place itself", () => {
    expect(columnChain("/home/me", places)).toEqual(["/home/me"]);
  });

  it("falls back to / for paths under no place", () => {
    expect(columnChain("/var/log", ["/home/me"])).toEqual(["/", "/var", "/var/log"]);
    expect(columnChain("/", [])).toEqual(["/"]);
  });

  it("does not treat a sibling with a shared prefix as an ancestor", () => {
    expect(columnChain("/home/me2/x", ["/home/me"])).toEqual(["/", "/home", "/home/me2", "/home/me2/x"]);
  });
});

describe("clickSelect", () => {
  const rows = ["a", "b", "c", "d", "e"];

  it("plain click selects one and sets the anchor", () => {
    expect(clickSelect(rows, ["a", "b"], "a", "c", {})).toEqual({ selected: ["c"], anchor: "c" });
  });

  it("ctrl toggles and moves the anchor", () => {
    expect(clickSelect(rows, ["a"], "a", "c", { ctrl: true })).toEqual({ selected: ["a", "c"], anchor: "c" });
    expect(clickSelect(rows, ["a", "c"], "c", "a", { ctrl: true })).toEqual({ selected: ["c"], anchor: "a" });
  });

  it("shift extends from the anchor in either direction and keeps it", () => {
    expect(clickSelect(rows, ["b"], "b", "d", { shift: true })).toEqual({ selected: ["b", "c", "d"], anchor: "b" });
    expect(clickSelect(rows, ["d"], "d", "b", { shift: true })).toEqual({ selected: ["b", "c", "d"], anchor: "d" });
  });

  it("shift with no usable anchor falls back to a plain click", () => {
    expect(clickSelect(rows, [], null, "b", { shift: true })).toEqual({ selected: ["b"], anchor: "b" });
  });

  it("ctrl+shift adds the range to the existing selection", () => {
    expect(clickSelect(rows, ["a", "e"], "e", "d", { shift: true, ctrl: true })).toEqual({ selected: ["a", "d", "e"], anchor: "e" });
  });
});

describe("flattenTree", () => {
  const dir = (path: string) => entry({ name: path.split("/").pop(), path, is_dir: true });
  const file = (path: string) => entry({ name: path.split("/").pop(), path });

  it("interleaves unfolded children under their parent, depth-first", () => {
    const rows = flattenTree(
      [dir("/p/a"), file("/p/z")],
      new Set(["/p/a", "/p/a/b"]),
      { "/p/a": [dir("/p/a/b"), file("/p/a/c")], "/p/a/b": [file("/p/a/b/d")] },
    );
    expect(rows.map((r) => [r.entry.path, r.depth, r.expanded, r.pending])).toEqual([
      ["/p/a", 0, true, false],
      ["/p/a/b", 1, true, false],
      ["/p/a/b/d", 2, false, false],
      ["/p/a/c", 1, false, false],
      ["/p/z", 0, false, false],
    ]);
  });

  it("marks an open folder whose listing is missing as pending", () => {
    const rows = flattenTree([dir("/p/a")], new Set(["/p/a"]), {});
    expect(rows).toHaveLength(1);
    expect(rows[0].pending).toBe(true);
  });

  it("ignores expansion state for files and folded folders", () => {
    const rows = flattenTree([file("/p/f"), dir("/p/a")], new Set(["/p/f"]), { "/p/a": [file("/p/a/x")] });
    expect(rows.map((r) => r.entry.path)).toEqual(["/p/f", "/p/a"]);
  });
});

describe("planMove", () => {
  it("skips items already in the folder and folders dropped on themselves or their descendants", () => {
    expect(planMove(["/a/x", "/b/y", "/c", "/d"], "/c/deep")).toEqual({ moves: ["/a/x", "/b/y", "/d"], skipped: ["/c"] });
    expect(planMove(["/a/x"], "/a")).toEqual({ moves: [], skipped: ["/a/x"] });
    expect(planMove(["/c"], "/c")).toEqual({ moves: [], skipped: ["/c"] });
  });
});

describe("icon sizes", () => {
  it("keeps a chosen size in range and on the slider's step", () => {
    expect(clampThumb(99)).toBe(96);
    expect(clampThumb(101)).toBe(104);
    expect(clampThumb(5)).toBe(THUMB_MIN);
    expect(clampThumb(9999)).toBe(THUMB_MAX);
    expect(clampThumb(Number.NaN)).toBe(96);
  });

  it("renders thumbnails at a few fixed sizes, sharper on HiDPI", () => {
    expect(thumbRenderSize(48)).toBe(128);
    expect(thumbRenderSize(160)).toBe(256);
    expect(thumbRenderSize(160, 2)).toBe(512);
    expect(thumbRenderSize(256, 3)).toBe(512);
  });
});

describe("relative time", () => {
  const now = Date.UTC(2026, 9, 18, 12, 0, 0);
  const ago = (secs: number) => relativeTime(now / 1000 - secs, now);
  it("dates a commit the way GitLens does", () => {
    expect(ago(10)).toBe("just now");
    expect(ago(5 * 60)).toMatch(/5 minutes ago/);
    expect(ago(3 * 24 * 3600)).toMatch(/3 days ago/);
    expect(ago(400 * 24 * 3600)).toMatch(/last year|1 year ago/);
  });
});
