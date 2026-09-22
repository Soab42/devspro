import { describe, expect, it } from "vitest";
import {
  DEFAULT_CONN,
  describeValue,
  formatDuration,
  formatTtl,
  formatValue,
  parseDb,
  parsePort,
  parseStoredConn,
  prettyIfJson,
  serialiseConn,
  typeTone,
} from "./utils";

describe("stored connection", () => {
  it("falls back to defaults for missing or broken storage", () => {
    expect(parseStoredConn(null)).toEqual(DEFAULT_CONN);
    expect(parseStoredConn("{not json")).toEqual(DEFAULT_CONN);
    expect(parseStoredConn('{"host":"","port":"abc","db":99}')).toEqual(DEFAULT_CONN);
  });

  it("restores host, port and db but never a password", () => {
    const json = '{"host":"redis.local","port":"6380","db":2,"password":"s3cret"}';
    expect(parseStoredConn(json)).toEqual({ host: "redis.local", port: 6380, db: 2 });
  });

  it("serialises without the password", () => {
    const json = serialiseConn({ host: "h", port: 1, db: 3, password: "x" });
    expect(JSON.parse(json)).toEqual({ host: "h", port: 1, db: 3 });
  });

  it("validates ports and db indexes", () => {
    expect(parsePort("6379")).toBe(6379);
    expect(parsePort(0)).toBeNull();
    expect(parsePort("70000")).toBeNull();
    expect(parsePort("6379.5")).toBeNull();
    expect(parseDb(" 4 ")).toBe(4);
    expect(parseDb(16)).toBeNull();
    expect(parseDb(-1)).toBeNull();
  });
});

describe("formatting", () => {
  it("formats TTLs with Redis's sentinel values", () => {
    expect(formatTtl(-1)).toBe("no expiry");
    expect(formatTtl(-2)).toBe("gone");
    expect(formatTtl(45)).toBe("45s");
    expect(formatTtl(3600 + 300)).toBe("1h 5m");
  });

  it("formats durations at the right granularity", () => {
    expect(formatDuration(90)).toBe("1m 30s");
    expect(formatDuration(7200)).toBe("2h");
    expect(formatDuration(86400 * 3 + 3600 * 2)).toBe("3d 2h");
    expect(formatDuration(86400)).toBe("1d");
  });

  it("assigns a tone per type and neutral for the rest", () => {
    expect(typeTone("string")).toBe("info");
    expect(typeTone("zset")).toBe("warning");
    expect(typeTone("stream")).toBe("neutral");
  });

  it("pretty-prints JSON strings and leaves others alone", () => {
    expect(prettyIfJson('{"a":1}')).toBe('{\n  "a": 1\n}');
    expect(prettyIfJson("{oops")).toBe("{oops");
    expect(prettyIfJson("plain")).toBe("plain");
  });
});

describe("values", () => {
  it("renders each type as text", () => {
    expect(formatValue({ type: "string", value: "hi" })).toBe("hi");
    expect(formatValue({ type: "list", items: ["a", "b"], length: 2 })).toBe("0) a\n1) b");
    expect(formatValue({ type: "hash", fields: [["k", "v"]], length: 1 })).toBe("k = v");
    expect(formatValue({ type: "set", members: ["x", "y"], length: 2 })).toBe("x\ny");
    expect(
      formatValue({ type: "zset", members: [{ member: "m", score: 1.5 }], length: 1 }),
    ).toBe("1.5\tm");
    expect(formatValue({ type: "none" })).toBe("");
    expect(formatValue({ type: "other", kind: "stream" })).toContain("stream");
  });

  it("describes sizes and notes truncation", () => {
    expect(describeValue({ type: "list", items: ["a"], length: 1 })).toBe("list, 1 items");
    expect(describeValue({ type: "list", items: ["a"], length: 500 })).toBe(
      "list, 500 items (showing first 1)",
    );
    expect(describeValue({ type: "none" })).toBe("key no longer exists");
  });
});
