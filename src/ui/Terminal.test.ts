import { describe, expect, it } from "vitest";
import { looksLikePrompt, osc7Path } from "./Terminal";

describe("osc7Path", () => {
  it("extracts and decodes the path from a file:// URL", () => {
    expect(osc7Path("file://box/home/me")).toBe("/home/me");
    expect(osc7Path("file:///var/log")).toBe("/var/log");
    expect(osc7Path("file://box/a%20b")).toBe("/a b");
    expect(osc7Path("http://box/x")).toBeNull();
    expect(osc7Path("")).toBeNull();
  });
});

describe("looksLikePrompt", () => {
  it("accepts common shell prompts", () => {
    expect(looksLikePrompt("user@box:~$ ")).toBe(true);
    expect(looksLikePrompt("root@box:/var# ")).toBe(true);
    expect(looksLikePrompt("box% ")).toBe(true);
    expect(looksLikePrompt("~ ❯ ")).toBe(true);
  });

  it("refuses secret prompts and blank lines", () => {
    expect(looksLikePrompt("user@box's password: ")).toBe(false);
    expect(looksLikePrompt("Enter passphrase for key '/home/me/.ssh/id_ed25519': ")).toBe(false);
    expect(looksLikePrompt("Verification code: ")).toBe(false);
    expect(looksLikePrompt("")).toBe(false);
    expect(looksLikePrompt("Last login: Tue")).toBe(false);
  });
});
