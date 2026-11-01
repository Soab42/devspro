import { describe, expect, it, vi, beforeEach } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router-dom";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
}));

const openModuleWindow = vi.fn(async (_m: { id: string }) => undefined);
vi.mock("./popout", () => ({
  openModuleWindow: (m: { id: string }) => openModuleWindow(m),
}));

import { CommandPalette } from "./CommandPalette";
import { ModulesProvider } from "./ModulesContext";

/** Reads the current route so activation can be asserted without a real page. */
function LocationProbe() {
  const location = useLocation();
  return <p data-testid="location">{location.pathname}</p>;
}

function setup(open = true, onClose = vi.fn()) {
  render(
    <MemoryRouter initialEntries={["/dashboard"]}>
      <ModulesProvider>
        <CommandPalette open={open} onClose={onClose} />
        <LocationProbe />
      </ModulesProvider>
    </MemoryRouter>,
  );
  return { onClose };
}

describe("CommandPalette", () => {
  beforeEach(() => {
    localStorage.clear();
    openModuleWindow.mockClear();
  });

  it("renders nothing when closed", () => {
    setup(false);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("combobox")).toBeNull();
  });

  it("filters options as the user types", () => {
    setup();
    const input = screen.getByRole("combobox");
    fireEvent.change(input, { target: { value: "docker" } });
    expect(screen.getByRole("option", { name: /^Docker/ })).toBeDefined();
    expect(screen.queryByRole("option", { name: /^Mail Catcher/ })).toBeNull();
  });

  it("opens the highlighted module in a fresh window on ArrowDown + Enter, without navigating", () => {
    setup();
    const input = screen.getByRole("combobox");
    // Dashboard (index 0, already the current route) then Web Server & SSL (index 1).
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(openModuleWindow).toHaveBeenCalledTimes(1);
    expect(openModuleWindow.mock.calls[0][0]).toMatchObject({ id: "nginx-ssl" });
    expect(screen.getByTestId("location").textContent).toBe("/dashboard");
  });

  it("calls onClose on Escape", () => {
    const { onClose } = setup();
    fireEvent.keyDown(screen.getByRole("combobox"), { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("opens the highlighted module in this window on Ctrl+Enter", () => {
    setup();
    const input = screen.getByRole("combobox");
    fireEvent.keyDown(input, { key: "ArrowDown" }); // highlight Web Server & SSL
    fireEvent.keyDown(input, { key: "Enter", ctrlKey: true });

    expect(openModuleWindow).not.toHaveBeenCalled();
    expect(screen.getByTestId("location").textContent).toBe("/nginx-ssl");
  });

  it("lists a switched-off module under Switched off, and enables it on activation", async () => {
    localStorage.setItem("devspro:disabled-modules", JSON.stringify(["redis"]));
    setup();

    expect(screen.getByText("Switched off")).toBeDefined();
    const option = screen.getByRole("option", { name: /Redis/ });
    expect(option.textContent).toContain("Enable and open");

    await act(async () => {
      fireEvent.click(option);
    });

    await waitFor(() => {
      const stored = JSON.parse(localStorage.getItem("devspro:disabled-modules") ?? "[]");
      expect(stored).not.toContain("redis");
    });
    expect(openModuleWindow).toHaveBeenCalledTimes(1);
    expect(openModuleWindow.mock.calls[0][0]).toMatchObject({ id: "redis" });
    expect(screen.getByTestId("location").textContent).toBe("/dashboard");
  });
});
