import { describe, expect, it } from "vitest";
import { isValidHostname, isValidIp, splitHostnames, validateEntry } from "./utils";

describe("isValidIp", () => {
  it("accepts v4 and v6", () => {
    expect(isValidIp("127.0.0.1")).toBe(true);
    expect(isValidIp("192.168.1.255")).toBe(true);
    expect(isValidIp("::1")).toBe(true);
    expect(isValidIp("fe80::1")).toBe(true);
    expect(isValidIp(" 10.0.0.1 ")).toBe(true);
  });
  it("rejects junk", () => {
    expect(isValidIp("")).toBe(false);
    expect(isValidIp("256.0.0.1")).toBe(false);
    expect(isValidIp("1.2.3")).toBe(false);
    expect(isValidIp("localhost")).toBe(false);
    expect(isValidIp("1::2::3")).toBe(false);
  });
});

describe("isValidHostname", () => {
  it("accepts dev-style names", () => {
    expect(isValidHostname("app.test")).toBe(true);
    expect(isValidHostname("my_app.local")).toBe(true);
    expect(isValidHostname("localhost")).toBe(true);
    expect(isValidHostname("a-b.c-d.e")).toBe(true);
  });
  it("rejects bad labels", () => {
    expect(isValidHostname("")).toBe(false);
    expect(isValidHostname("-bad.test")).toBe(false);
    expect(isValidHostname("bad-.test")).toBe(false);
    expect(isValidHostname("a..b")).toBe(false);
    expect(isValidHostname("has space")).toBe(false);
    expect(isValidHostname("x".repeat(64) + ".test")).toBe(false);
  });
});

describe("splitHostnames", () => {
  it("splits on whitespace and commas", () => {
    expect(splitHostnames("a.test  b.test,c.test\n d.test")).toEqual([
      "a.test",
      "b.test",
      "c.test",
      "d.test",
    ]);
    expect(splitHostnames("  ")).toEqual([]);
  });
});

describe("validateEntry", () => {
  it("returns the first problem, or null", () => {
    expect(validateEntry("127.0.0.1", ["app.test"])).toBeNull();
    expect(validateEntry("nope", ["app.test"])).toMatch(/address/);
    expect(validateEntry("127.0.0.1", [])).toMatch(/hostname/);
    expect(validateEntry("127.0.0.1", ["ok.test", "bad host"])).toMatch(/"bad host"/);
  });
});
