import { afterEach, describe, expect, it, vi } from "vitest";

const UNFINISHED = ["http", "browser", "remote-desktop"];

describe("modules still in development", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.resetModules();
  });

  it("are registered in dev builds", async () => {
    vi.stubEnv("DEV", true);
    const { modules } = await import("./ModuleRegistry");
    expect(modules.filter((m) => m.inDevelopment).map((m) => m.id)).toEqual(UNFINISHED);
  });

  it("are left out of production builds", async () => {
    vi.stubEnv("DEV", false);
    const { modules, moduleById, moduleByPath } = await import("./ModuleRegistry");
    expect(modules.some((m) => m.inDevelopment)).toBe(false);
    for (const id of UNFINISHED) expect(moduleById(id)).toBeUndefined();
    expect(moduleByPath("/browser")).toBeUndefined();
  });
});
