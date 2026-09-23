import { describe, expect, it } from "vitest";
import { describeRdpExit, desktopLabel, portForProtocol, validateDesktopForm } from "./utils";

describe("desktopLabel", () => {
  it("shows the user and a non-default port only when they matter", () => {
    expect(desktopLabel({ protocol: "vnc", host: "pi.local", port: 5900, username: "" })).toBe("pi.local");
    expect(desktopLabel({ protocol: "vnc", host: "pi.local", port: 5901, username: "" })).toBe("pi.local:5901");
    expect(desktopLabel({ protocol: "rdp", host: "pc", port: 3389, username: "alice" })).toBe("alice@pc");
    expect(desktopLabel({ protocol: "rdp", host: "pc", port: 3390, username: "alice" })).toBe("alice@pc:3390");
  });
});

describe("portForProtocol", () => {
  it("follows the protocol while the port is untouched, and keeps a typed one", () => {
    expect(portForProtocol("5900", "vnc", "rdp")).toBe("3389");
    expect(portForProtocol("3389", "rdp", "vnc")).toBe("5900");
    expect(portForProtocol("", "vnc", "rdp")).toBe("3389");
    expect(portForProtocol("5901", "vnc", "rdp")).toBe("5901");
  });
});

describe("validateDesktopForm", () => {
  const ok = { name: "Office", protocol: "vnc" as const, host: "pc", port: "5900", username: "" };

  it("accepts a plain vnc connection without a username", () => {
    expect(validateDesktopForm(ok)).toBeNull();
  });

  it("rejects blanks, spaces and bad ports", () => {
    expect(validateDesktopForm({ ...ok, name: " " })).toMatch(/name/i);
    expect(validateDesktopForm({ ...ok, host: "" })).toMatch(/host/i);
    expect(validateDesktopForm({ ...ok, host: "my pc" })).toMatch(/spaces/i);
    expect(validateDesktopForm({ ...ok, port: "0" })).toMatch(/port/i);
    expect(validateDesktopForm({ ...ok, port: "abc" })).toMatch(/port/i);
  });

  it("requires a username for rdp only", () => {
    expect(validateDesktopForm({ ...ok, protocol: "rdp" })).toMatch(/username/i);
    expect(validateDesktopForm({ ...ok, protocol: "rdp", username: "alice" })).toBeNull();
  });
});

describe("describeRdpExit", () => {
  it("treats a zero exit as a plain close", () => {
    expect(describeRdpExit({ client: "FreeRDP 3", code: 0, output: "noise" })).toEqual({
      tone: "success",
      text: "FreeRDP 3 closed.",
    });
  });

  it("surfaces the last line of output on failure", () => {
    expect(describeRdpExit({ client: "FreeRDP 3", code: 131, output: "a\n[ERROR] authentication failed\n" })).toEqual({
      tone: "danger",
      text: "FreeRDP 3 exited with code 131: [ERROR] authentication failed",
    });
    expect(describeRdpExit({ client: "Remmina", code: null, output: "" })).toEqual({
      tone: "danger",
      text: "Remmina was terminated.",
    });
  });
});
