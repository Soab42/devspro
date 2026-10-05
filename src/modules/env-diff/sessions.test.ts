import { beforeEach, describe, expect, it } from "vitest";
import {
  EnvSession,
  parseSessions,
  PASTED_KEY,
  readPasted,
  suggestName,
  withSession,
  writePasted,
} from "./sessions";

const session = (name: string, extra: Partial<EnvSession> = {}): EnvSession => ({
  name,
  left: "/w/api/.env",
  right: "/w/api/.env.example",
  pasted: [],
  labels: {},
  keysOnly: false,
  syncScroll: true,
  filter: "diff",
  ...extra,
});

describe("parseSessions", () => {
  it("reads back what was stored", () => {
    const list = [
      session("api", {
        keysOnly: true,
        filter: "only-left",
        syncScroll: false,
        labels: { left: "dev", right: "live" },
      }),
    ];
    expect(parseSessions(JSON.stringify(list))).toEqual(list);
  });

  it("returns nothing for missing or unreadable storage", () => {
    expect(parseSessions(null)).toEqual([]);
    expect(parseSessions(undefined)).toEqual([]);
    expect(parseSessions("not json")).toEqual([]);
    expect(parseSessions('{"name":"x"}')).toEqual([]);
  });

  it("drops entries without a name or any file, and repairs the rest", () => {
    expect(
      parseSessions(
        JSON.stringify([
          { name: "", left: "/a" },
          { name: "no files", left: null, right: "" },
          { name: "half", left: "/a/.env", filter: "bogus", keysOnly: "yes" },
          42,
        ]),
      ),
    ).toEqual([
      {
        name: "half",
        left: "/a/.env",
        right: null,
        pasted: [],
        labels: {},
        keysOnly: false,
        syncScroll: true,
        filter: "diff",
      },
    ]);
  });

  it("keeps only the side names that say something", () => {
    const labelsOf = (labels: unknown) =>
      parseSessions(JSON.stringify([{ name: "x", left: "/a/.env", labels }]))[0].labels;
    expect(labelsOf({ left: "  dev ", right: "" })).toEqual({ left: "dev" });
    expect(labelsOf({ left: 3, right: "live", middle: "x" })).toEqual({ right: "live" });
    expect(labelsOf("dev")).toEqual({});
    expect(labelsOf(undefined)).toEqual({});
  });

  it("keeps a session of pasted text alone, and reads older ones without the field", () => {
    expect(
      parseSessions(JSON.stringify([{ name: "pasted", left: null, right: null, pasted: ["right", "bogus"] }])),
    ).toMatchObject([{ name: "pasted", left: null, right: null, pasted: ["right"] }]);
    expect(parseSessions(JSON.stringify([{ name: "old", left: "/a/.env" }]))[0].pasted).toEqual([]);
  });

  it("never calls a side both a path and pasted text", () => {
    expect(
      parseSessions(JSON.stringify([{ name: "x", left: "/a/.env", pasted: ["left"] }]))[0].pasted,
    ).toEqual([]);
  });
});

describe("pasted text", () => {
  beforeEach(() => localStorage.clear());

  it("is kept by session name in local storage, apart from the session list", () => {
    writePasted("a", { right: "TOKEN=1\n" });
    writePasted("b", { left: "X=2\n" });
    expect(readPasted("a")).toEqual({ right: "TOKEN=1\n" });
    expect(JSON.parse(localStorage.getItem(PASTED_KEY) ?? "")).toEqual({
      a: { right: "TOKEN=1\n" },
      b: { left: "X=2\n" },
    });
  });

  it("forgets a session's text when there is none to keep", () => {
    writePasted("a", { right: "TOKEN=1\n" });
    writePasted("a", {});
    expect(readPasted("a")).toEqual({});
    writePasted("b", { left: "X" });
    writePasted("b", null);
    expect(localStorage.getItem(PASTED_KEY)).toBe("{}");
  });

  it("reads nothing from missing or broken storage", () => {
    expect(readPasted("nope")).toEqual({});
    localStorage.setItem(PASTED_KEY, "not json");
    expect(readPasted("a")).toEqual({});
  });
});

describe("withSession", () => {
  it("adds in name order and replaces a session of the same name", () => {
    const list = withSession(withSession([], session("web")), session("api"));
    expect(list.map((s) => s.name)).toEqual(["api", "web"]);
    const replaced = withSession(list, session("web", { keysOnly: true }));
    expect(replaced).toHaveLength(2);
    expect(replaced.find((s) => s.name === "web")?.keysOnly).toBe(true);
  });
});

describe("suggestName", () => {
  it("names a session after each file and its folder", () => {
    expect(suggestName("/w/api/.env", "/w/web/.env")).toBe("api/.env vs web/.env");
    expect(suggestName("C:\\w\\api\\.env", null)).toBe("api/.env");
    expect(suggestName(null, "/.env")).toBe(".env");
    expect(suggestName(null, null)).toBe("");
  });
});
