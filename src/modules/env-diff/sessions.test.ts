import { describe, expect, it } from "vitest";
import { EnvSession, parseSessions, suggestName, withSession } from "./sessions";

const session = (name: string, extra: Partial<EnvSession> = {}): EnvSession => ({
  name,
  left: "/w/api/.env",
  right: "/w/api/.env.example",
  keysOnly: false,
  syncScroll: true,
  filter: "diff",
  ...extra,
});

describe("parseSessions", () => {
  it("reads back what was stored", () => {
    const list = [session("api", { keysOnly: true, filter: "only-left", syncScroll: false })];
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
      { name: "half", left: "/a/.env", right: null, keysOnly: false, syncScroll: true, filter: "diff" },
    ]);
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
