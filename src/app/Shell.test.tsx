import { describe, expect, it, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";

// The shell itself makes no Tauri calls, but AuthContext does at import time.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
}));

vi.mock("../auth/AuthContext", () => ({
  useAuth: () => ({
    profile: { id: "1", display_name: "Ada Lovelace", created_at: "" },
    lock: vi.fn(),
  }),
}));

import Shell from "./Shell";
import { MODULE_GROUPS, modules } from "./ModuleRegistry";
import { ThemeProvider } from "./ThemeContext";

function renderShell(width = 1400) {
  window.innerWidth = width;
  return render(
    <ThemeProvider>
      <MemoryRouter initialEntries={["/dashboard"]}>
        <Routes>
          <Route element={<Shell />}>
            <Route path="/dashboard" element={<p>page content</p>} />
          </Route>
        </Routes>
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe("Shell", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("renders a nav link for every registered module", () => {
    renderShell();
    const nav = screen.getByRole("navigation", { name: "Main navigation" });
    for (const m of modules) {
      expect(within(nav).getByRole("link", { name: m.label })).toBeDefined();
    }
  });

  it("renders each non-overview group heading", () => {
    renderShell();
    const nav = screen.getByRole("navigation", { name: "Main navigation" });
    for (const group of MODULE_GROUPS) {
      if (!group.label) continue;
      expect(within(nav).getByText(group.label)).toBeDefined();
    }
  });

  it("marks the active route", () => {
    renderShell();
    const active = screen.getByRole("link", { name: "Dashboard" });
    expect(active.className).toContain("sidebar-link-active");
  });

  it("exposes the theme control as a radiogroup", () => {
    renderShell();
    const group = screen.getByRole("radiogroup", { name: "Colour theme" });
    // Icon-only in the topbar, so the labels are the accessible names.
    for (const label of ["Light", "System", "Dark"]) {
      expect(within(group).getByRole("radio", { name: label })).toBeDefined();
    }
  });

  it("shows the profile and offers Lock rather than Log out", async () => {
    renderShell();
    // The handler is lock(), so "Log out" was simply the wrong verb.
    const trigger = screen.getByRole("button", { name: /Ada Lovelace/ });
    trigger.click();
    expect(await screen.findByRole("menuitem", { name: "Lock" })).toBeDefined();
    expect(screen.queryByRole("menuitem", { name: "Log out" })).toBeNull();
  });

  it("renders the routed page content", () => {
    renderShell();
    expect(screen.getByText("page content")).toBeDefined();
  });

  it("keeps the sidebar expandable after it has been collapsed", () => {
    // The collapse control used to be hidden by .shell-railed — the very state
    // collapsing produces — so pinning the rail was a one-way door with no way
    // back short of clearing localStorage.
    const { container } = renderShell(1400);

    fireEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }));

    expect(container.querySelector(".shell")?.className).toContain("shell-railed");
    // A pinned rail is not a forced one, so the toggle must survive.
    expect(container.querySelector(".shell")?.className).not.toContain(
      "shell-rail-forced",
    );
    const expand = screen.getByRole("button", { name: "Expand sidebar" });
    expect(expand).toBeDefined();

    fireEvent.click(expand);
    expect(container.querySelector(".shell")?.className).not.toContain("shell-railed");
  });

  it("marks a width-forced rail distinctly from a pinned one", () => {
    // Between 40rem and 60rem the rail is imposed by the window, and there is
    // nothing to expand into — that is the only case where hiding the toggle
    // is correct.
    const { container } = renderShell(700);
    const shell = container.querySelector(".shell")?.className ?? "";
    expect(shell).toContain("shell-railed");
    expect(shell).toContain("shell-rail-forced");
  });

  it("uses the drawer layout only in a narrow window", () => {
    const { container, unmount } = renderShell(1400);
    expect(container.querySelector(".shell")?.className).not.toContain("shell-drawer");
    // No menu toggle at full width — the sidebar is already visible.
    expect(screen.queryByRole("button", { name: "Toggle navigation" })).toBeNull();
    unmount();

    // 500px is below the 40rem drawer breakpoint at the 13px clamp floor.
    const narrow = renderShell(500);
    expect(narrow.container.querySelector(".shell")?.className).toContain("shell-drawer");
    expect(screen.getByRole("button", { name: "Toggle navigation" })).toBeDefined();
  });
});
