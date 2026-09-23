import { describe, expect, it, vi, beforeEach } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { ReactNode } from "react";

const invoke = vi.fn(async (_cmd: string, _args?: unknown): Promise<unknown> => null);
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invoke(cmd, args),
}));

import { ModulesProvider, sanitiseDisabled, useModules } from "./ModulesContext";
import { modules } from "./ModuleRegistry";

const wrapper = ({ children }: { children: ReactNode }) => (
  <ModulesProvider>{children}</ModulesProvider>
);

describe("ModulesContext", () => {
  beforeEach(() => {
    localStorage.clear();
    invoke.mockReset();
    invoke.mockResolvedValue(null);
  });

  it("starts with every module enabled", () => {
    const { result } = renderHook(() => useModules(), { wrapper });
    expect(result.current.enabled.map((m) => m.id)).toEqual(modules.map((m) => m.id));
  });

  it("persists a switch-off to the settings table and the local cache", async () => {
    const { result } = renderHook(() => useModules(), { wrapper });
    await act(async () => {
      await result.current.setEnabled("redis", false);
    });
    expect(result.current.isEnabled("redis")).toBe(false);
    expect(result.current.enabled.some((m) => m.id === "redis")).toBe(false);
    expect(invoke).toHaveBeenCalledWith("settings_set", {
      key: "disabled_modules",
      value: JSON.stringify(["redis"]),
    });
    expect(JSON.parse(localStorage.getItem("devspro:disabled-modules") ?? "[]")).toEqual(["redis"]);
  });

  it("reads the stored value from the database on mount, and the database wins over the cache", async () => {
    localStorage.setItem("devspro:disabled-modules", JSON.stringify(["ssh"]));
    invoke.mockImplementation(async (cmd: string) =>
      cmd === "settings_get" ? JSON.stringify(["docker"]) : null,
    );
    const { result } = renderHook(() => useModules(), { wrapper });
    // Cache first, so the first paint is already right when the two agree.
    expect(result.current.isEnabled("ssh")).toBe(false);
    await waitFor(() => expect(result.current.isEnabled("docker")).toBe(false));
    expect(result.current.isEnabled("ssh")).toBe(true);
  });

  it("refuses to switch off a core module", async () => {
    const { result } = renderHook(() => useModules(), { wrapper });
    await act(async () => {
      await result.current.setEnabled("settings", false);
      await result.current.setEnabled("dashboard", false);
    });
    expect(result.current.isEnabled("settings")).toBe(true);
    expect(result.current.isEnabled("dashboard")).toBe(true);
    expect(invoke).not.toHaveBeenCalledWith("settings_set", expect.anything());
  });

  it("drops unknown and core ids from a stored value", () => {
    // A hand-edited or stale value must not be able to hide the Dashboard or
    // Settings — the only way back on would be gone.
    expect([...sanitiseDisabled(["dashboard", "settings", "gone", 42, "redis"])]).toEqual(["redis"]);
    expect([...sanitiseDisabled("nonsense")]).toEqual([]);
  });
});
