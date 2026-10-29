import { describe, expect, it } from "vitest";
import { formatProgress, parseNotes } from "./updater";

describe("parseNotes", () => {
  it("reads CHANGELOG.md's groups and bullets as release.mjs copies them", () => {
    const notes = "### New\n- **In-app updates.** Signed.\n- Memory page.\n\n### Fixed\n- The `pump` loop.";
    expect(parseNotes(notes)).toEqual([
      { title: "New", items: ["**In-app updates.** Signed.", "Memory page."] },
      { title: "Fixed", items: ["The `pump` loop."] },
    ]);
  });

  it("keeps a wrapped line with its bullet, and bullets before any heading", () => {
    expect(parseNotes("- one\n  continued\n- two")).toEqual([{ title: null, items: ["one continued", "two"] }]);
  });

  it("drops headings with nothing under them, and reads nothing from empty notes", () => {
    expect(parseNotes("### New\n### Fixed\n- x")).toEqual([{ title: "Fixed", items: ["x"] }]);
    expect(parseNotes("")).toEqual([]);
  });
});

describe("formatProgress", () => {
  it("shows the total only when the server sent one", () => {
    expect(formatProgress({ downloaded: 12_340_000, total: 40_100_000 })).toBe("12.3 MB of 40.1 MB");
    expect(formatProgress({ downloaded: 500_000, total: null })).toBe("0.5 MB");
  });
});
