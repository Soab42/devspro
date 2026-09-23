import { describe, expect, it } from "vitest";
import {
  defaultBackupFilename,
  defaultSelection,
  describeCounts,
} from "./backup";

describe("defaultBackupFilename", () => {
  it("uses the ISO date only", () => {
    expect(defaultBackupFilename(new Date("2026-09-22T15:04:05Z"))).toBe(
      "devspro-backup-2026-09-22.json",
    );
  });
});

describe("defaultSelection", () => {
  it("checks only sections with content", () => {
    expect(
      defaultSelection([
        { section: "sites", count: 2 },
        { section: "workers", count: 0 },
        { section: "settings", count: 1 },
        { section: "ca", count: 0 },
      ]),
    ).toEqual(["sites", "settings"]);
  });
});

describe("describeCounts", () => {
  it("pluralises and names the CA", () => {
    expect(
      describeCounts([
        { section: "sites", count: 2 },
        { section: "workers", count: 1 },
        { section: "settings", count: 3 },
        { section: "ca", count: 1 },
      ]),
    ).toBe("2 sites, 1 worker, 3 preferences, root CA");
  });

  it("says nothing when every section is empty", () => {
    expect(describeCounts([{ section: "sites", count: 0 }])).toBe("nothing");
  });
});
