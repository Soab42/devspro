import { describe, expect, it } from "vitest";
import {
  displayBody,
  findHeader,
  formatBytes,
  formatElapsed,
  formatTime,
  isJsonContentType,
  newHeaderRow,
  pairsToRows,
  prettyJson,
  rowsToPairs,
  siteUrl,
  statusVariant,
} from "./utils";

describe("header rows", () => {
  it("drops blank keys and trims when converting to pairs", () => {
    const rows = [newHeaderRow(" Accept ", " application/json "), newHeaderRow("", "x"), newHeaderRow("X", "")];
    expect(rowsToPairs(rows)).toEqual([
      ["Accept", "application/json"],
      ["X", ""],
    ]);
  });

  it("adds a trailing blank row when loading pairs", () => {
    const rows = pairsToRows([["A", "1"]]);
    expect(rows).toHaveLength(2);
    expect(rows[0]).toMatchObject({ key: "A", value: "1" });
    expect(rows[1]).toMatchObject({ key: "", value: "" });
    expect(rows[0].id).not.toBe(rows[1].id);
  });
});

describe("statusVariant", () => {
  it("maps status classes", () => {
    expect(statusVariant(200)).toBe("ok");
    expect(statusVariant(204)).toBe("ok");
    expect(statusVariant(301)).toBe("info");
    expect(statusVariant(404)).toBe("warn");
    expect(statusVariant(500)).toBe("bad");
    expect(statusVariant(0)).toBe("neutral");
    expect(statusVariant(101)).toBe("neutral");
  });
});

describe("formatting", () => {
  it("formats bytes", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1536)).toBe("1.5 kB");
    expect(formatBytes(2 * 1024 * 1024)).toBe("2.00 MB");
  });

  it("formats elapsed time", () => {
    expect(formatElapsed(42)).toBe("42 ms");
    expect(formatElapsed(1500)).toBe("1.50 s");
  });

  it("formats history timestamps relative to today", () => {
    const now = new Date(2026, 8, 22, 12, 0, 0);
    const today = new Date(2026, 8, 22, 9, 5, 0).toISOString();
    const yesterday = new Date(2026, 8, 21, 9, 5, 0).toISOString();
    expect(formatTime(today, now)).not.toContain("2026");
    expect(formatTime(yesterday, now)).toContain(new Date(yesterday).toLocaleDateString());
    expect(formatTime("garbage", now)).toBe("garbage");
  });
});

describe("content-type handling", () => {
  it("finds headers case-insensitively", () => {
    expect(findHeader([["Content-Type", "text/html"]], "content-type")).toBe("text/html");
    expect(findHeader([], "content-type")).toBeNull();
  });

  it("detects JSON content types", () => {
    expect(isJsonContentType("application/json")).toBe(true);
    expect(isJsonContentType("Application/JSON; charset=utf-8")).toBe(true);
    expect(isJsonContentType("application/problem+json")).toBe(true);
    expect(isJsonContentType("text/html")).toBe(false);
    expect(isJsonContentType(null)).toBe(false);
  });

  it("pretty-prints JSON and rejects junk", () => {
    expect(prettyJson('{"a":[1]}')).toBe('{\n  "a": [\n    1\n  ]\n}');
    expect(prettyJson("{nope")).toBeNull();
  });

  it("only pretty-prints when the server says JSON, and falls back on bad bodies", () => {
    expect(displayBody('{"a":1}', "application/json")).toBe('{\n  "a": 1\n}');
    expect(displayBody('{"a":1}', "text/plain")).toBe('{"a":1}');
    expect(displayBody('{"a":', "application/json")).toBe('{"a":');
  });
});

describe("siteUrl", () => {
  it("builds the HTTPS root", () => {
    expect(siteUrl("app.test")).toBe("https://app.test/");
  });
});
