import { describe, expect, it } from "vitest";
import {
  NO_OPEN_FILES,
  OpenFiles,
  closeTab,
  closeTabsUnder,
  cycleTab,
  moveTab,
  openTab,
  parseOpenFiles,
  retargetTabs,
  setDirty,
} from "./openFiles";

const file = (path: string) => ({ path, name: path.split("/").pop() ?? path });
const paths = (s: OpenFiles) => s.files.map((f) => f.path);

function three(): OpenFiles {
  let s = NO_OPEN_FILES;
  for (const p of ["/a", "/b", "/c"]) s = openTab(s, file(p));
  return s;
}

describe("open file tabs", () => {
  it("opens beside the tab in front and brings an open file forward instead of duplicating it", () => {
    let s = three();
    expect(paths(s)).toEqual(["/a", "/b", "/c"]);
    s = openTab({ ...s, active: "/a" }, file("/d"));
    expect(paths(s)).toEqual(["/a", "/d", "/b", "/c"]);
    expect(s.active).toBe("/d");
    s = openTab(s, file("/b"));
    expect(paths(s)).toHaveLength(4);
    expect(s.active).toBe("/b");
    // From the folder view, a new tab goes last.
    expect(paths(openTab({ ...s, active: null }, file("/e")))).toEqual(["/a", "/d", "/b", "/c", "/e"]);
  });

  it("closing the tab in front hands over to the one that slides into its place", () => {
    const s = { ...three(), active: "/b" };
    expect(closeTab(s, "/b").active).toBe("/c");
    expect(closeTab({ ...s, active: "/c" }, "/c").active).toBe("/b");
    expect(closeTab({ ...s, active: "/a" }, "/c").active).toBe("/a");
    expect(closeTab(closeTab(closeTab(s, "/a"), "/b"), "/c").active).toBeNull();
  });

  it("cycles through the folder view and every tab, both ways", () => {
    const s = { ...three(), active: null };
    expect(cycleTab(s, 1).active).toBe("/a");
    expect(cycleTab(s, -1).active).toBe("/c");
    expect(cycleTab({ ...s, active: "/c" }, 1).active).toBeNull();
  });

  it("reorders by drag", () => {
    const s = three();
    expect(paths(moveTab(s, "/c", "/a"))).toEqual(["/c", "/a", "/b"]);
    expect(paths(moveTab(s, "/a", null))).toEqual(["/b", "/c", "/a"]);
    expect(moveTab(s, "/a", "/a")).toBe(s);
  });

  it("follows a renamed file or folder, and closes what was trashed", () => {
    let s = NO_OPEN_FILES;
    for (const p of ["/p/src/a.ts", "/p/src/b.ts", "/p/README.md"]) s = openTab(s, file(p));
    s = setDirty(s, "/p/src/a.ts", true);
    const moved = retargetTabs(s, "/p/src", "/p/lib");
    expect(paths(moved)).toEqual(["/p/lib/a.ts", "/p/lib/b.ts", "/p/README.md"]);
    expect(moved.dirty).toEqual(["/p/lib/a.ts"]);
    const renamed = retargetTabs(s, "/p/README.md", "/p/NOTES.md", "NOTES.md");
    expect(renamed.files[2]).toEqual({ path: "/p/NOTES.md", name: "NOTES.md" });
    expect(renamed.active).toBe("/p/NOTES.md");
    // "/p/src2" is not under "/p/src".
    expect(paths(retargetTabs(openTab(s, file("/p/src2/x")), "/p/src", "/q"))).toContain("/p/src2/x");
    const trashed = closeTabsUnder(s, ["/p/src"]);
    expect(paths(trashed)).toEqual(["/p/README.md"]);
    expect(trashed.dirty).toEqual([]);
  });

  it("restores only well-formed saved lists, never their dirty marks", () => {
    expect(parseOpenFiles(null)).toEqual(NO_OPEN_FILES);
    expect(parseOpenFiles("{nope")).toEqual(NO_OPEN_FILES);
    const saved = JSON.stringify({ files: [file("/a"), { path: 3 }, file("/b")], active: "/b", dirty: ["/a"] });
    expect(parseOpenFiles(saved)).toEqual({ files: [file("/a"), file("/b")], active: "/b", dirty: [] });
    expect(parseOpenFiles(JSON.stringify({ files: [file("/a")], active: "/gone" })).active).toBeNull();
  });
});
