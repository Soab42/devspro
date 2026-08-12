import { describe, expect, it } from "vitest";
import { generatePassword } from "./utils";

describe("generatePassword", () => {
  it("defaults to 20 characters", () => {
    expect(generatePassword()).toHaveLength(20);
  });

  it("respects a custom length", () => {
    expect(generatePassword(8)).toHaveLength(8);
    expect(generatePassword(64)).toHaveLength(64);
  });

  it("only uses characters from the declared charset", () => {
    const charset = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_!@#%^*+=";
    const password = generatePassword(200);
    for (const char of password) {
      expect(charset).toContain(char);
    }
  });

  it("is not trivially constant across calls", () => {
    const passwords = new Set(Array.from({ length: 5 }, () => generatePassword()));
    expect(passwords.size).toBeGreaterThan(1);
  });
});
