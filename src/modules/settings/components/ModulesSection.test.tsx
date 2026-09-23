import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
}));

import { ModulesProvider } from "../../../app/ModulesContext";
import { modules } from "../../../app/ModuleRegistry";
import ModulesSection from "./ModulesSection";

function renderSection() {
  return render(
    <ModulesProvider>
      <ModulesSection />
    </ModulesProvider>,
  );
}

describe("ModulesSection", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(invoke).mockClear();
  });

  it("renders one switch per module, named after the module label", () => {
    renderSection();
    for (const m of modules) {
      expect(screen.getByRole("switch", { name: m.label })).toBeDefined();
    }
  });

  it("renders core modules' switches disabled and checked", () => {
    renderSection();
    for (const m of modules.filter((m) => m.core)) {
      const el = screen.getByRole("switch", { name: m.label });
      expect(el).toHaveProperty("disabled", true);
      expect(el.getAttribute("aria-checked")).toBe("true");
    }
  });

  it("toggling a non-core switch off persists the change and unchecks it", async () => {
    renderSection();
    const target = modules.find((m) => !m.core);
    if (!target) throw new Error("expected at least one non-core module");

    const el = screen.getByRole("switch", { name: target.label });
    el.click();

    await vi.waitFor(() => {
      expect(el.getAttribute("aria-checked")).toBe("false");
    });

    expect(invoke).toHaveBeenCalledWith(
      "settings_set",
      expect.objectContaining({
        key: "disabled_modules",
        value: expect.stringContaining(target.id),
      }),
    );
  });

  it("persists every toggle, not just the first one after a render", async () => {
    renderSection();
    const target = modules.find((m) => !m.core);
    if (!target) throw new Error("expected at least one non-core module");
    const el = screen.getByRole("switch", { name: target.label });

    el.click();
    await vi.waitFor(() => expect(el.getAttribute("aria-checked")).toBe("false"));
    el.click();
    await vi.waitFor(() => expect(el.getAttribute("aria-checked")).toBe("true"));
    el.click();
    await vi.waitFor(() => expect(el.getAttribute("aria-checked")).toBe("false"));

    const writes = vi
      .mocked(invoke)
      .mock.calls.filter(([cmd, args]) => cmd === "settings_set" && (args as { key: string }).key === "disabled_modules")
      .map(([, args]) => JSON.parse((args as { value: string }).value) as string[]);
    expect(writes).toEqual([[target.id], [], [target.id]]);
  });

  it("updates the count line after a toggle", async () => {
    renderSection();
    const total = modules.length;
    expect(screen.getByText(`${total} of ${total} modules on`)).toBeDefined();

    const target = modules.find((m) => !m.core);
    if (!target) throw new Error("expected at least one non-core module");
    screen.getByRole("switch", { name: target.label }).click();

    expect(
      await screen.findByText(`${total - 1} of ${total} modules on`),
    ).toBeDefined();
  });
});
