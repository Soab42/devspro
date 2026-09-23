import { describe, expect, it, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { registerPopoutHandoff } from "./popoutState";

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

// Window creation needs the Tauri runtime; the shell only has to ask for it.
const openModuleWindow = vi.fn(async (_m: { id: string }, _params?: Record<string, string>) => undefined);
let popoutId: string | null = null;
vi.mock("./popout", () => ({
  openModuleWindow: (m: { id: string }, params?: Record<string, string>) => openModuleWindow(m, params),
  currentPopoutModuleId: () => popoutId,
  isBrowserTabWindow: () => false,
  POPOUT_ACTIONS_SLOT: "popout-topbar-actions",
  POPOUT_TABS_SLOT: "popout-topbar-tabs",
}));

import Shell from "./Shell";
import { MODULE_GROUPS, modules } from "./ModuleRegistry";
import { ModulesProvider } from "./ModulesContext";
import { ThemeProvider } from "./ThemeContext";

function renderShell(width = 1400, path = "/dashboard") {
  window.innerWidth = width;
  return render(
    <ThemeProvider>
      <ModulesProvider>
        <MemoryRouter initialEntries={[path]}>
          <Routes>
            <Route element={<Shell />}>
              <Route path="/dashboard" element={<p>page content</p>} />
              <Route path="/docker" element={<p>docker content</p>} />
            </Route>
          </Routes>
        </MemoryRouter>
      </ModulesProvider>
    </ThemeProvider>,
  );
}

describe("Shell", () => {
  beforeEach(() => {
    localStorage.clear();
    openModuleWindow.mockClear();
    popoutId = null;
  });

  it("renders a nav link for every registered module", () => {
    renderShell();
    const nav = screen.getByRole("navigation", { name: "Main navigation" });
    for (const m of modules) {
      expect(within(nav).getByRole("link", { name: m.label })).toBeDefined();
    }
  });

  it("leaves switched-off modules out of the sidebar", () => {
    localStorage.setItem("devspro:disabled-modules", JSON.stringify(["redis", "ssh"]));
    renderShell();
    const nav = screen.getByRole("navigation", { name: "Main navigation" });
    expect(within(nav).queryByRole("link", { name: "Redis" })).toBeNull();
    expect(within(nav).queryByRole("link", { name: "SSH" })).toBeNull();
    expect(within(nav).getByRole("link", { name: "Docker" })).toBeDefined();
  });

  it("opens the launcher from the topbar and with Ctrl+K", () => {
    renderShell();
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Search modules" }));
    expect(screen.getByRole("dialog")).toBeDefined();
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    expect(screen.getByRole("dialog")).toBeDefined();
  });

  it("pops the current module out from the topbar", async () => {
    renderShell(1400, "/docker");
    fireEvent.click(screen.getByRole("button", { name: "Open Docker in a new window" }));
    await waitFor(() => expect(openModuleWindow).toHaveBeenCalledTimes(1));
    expect(openModuleWindow.mock.calls[0][0]).toMatchObject({ id: "docker" });
    expect(openModuleWindow.mock.calls[0][1]).toBeUndefined();
  });

  it("carries the page's state into the popped-out window", async () => {
    const unregister = registerPopoutHandoff(() => ({ params: { path: "/srv/www" } }));
    try {
      renderShell(1400, "/docker");
      fireEvent.click(screen.getByRole("button", { name: "Open Docker in a new window" }));
      await waitFor(() => expect(openModuleWindow).toHaveBeenCalledTimes(1));
      expect(openModuleWindow.mock.calls[0][1]).toEqual({ path: "/srv/www" });
    } finally {
      unregister();
    }
  });

  it("lets the page move its own state instead, falling back to a fresh window when it has none", async () => {
    let moved = true;
    const open = vi.fn(async () => moved);
    const unregister = registerPopoutHandoff(() => ({ open }));
    try {
      renderShell(1400, "/docker");
      const button = screen.getByRole("button", { name: "Open Docker in a new window" });
      fireEvent.click(button);
      await waitFor(() => expect(open).toHaveBeenCalledTimes(1));
      expect(openModuleWindow).not.toHaveBeenCalled();

      moved = false;
      fireEvent.click(button);
      await waitFor(() => expect(openModuleWindow).toHaveBeenCalledTimes(1));
      expect(openModuleWindow.mock.calls[0][0]).toMatchObject({ id: "docker" });
    } finally {
      unregister();
    }
  });

  it("opens a module in a new window on Ctrl+click or middle-click, not on plain click", () => {
    renderShell();
    const link = screen.getByRole("link", { name: "Docker" });

    fireEvent.click(link, { ctrlKey: true });
    expect(openModuleWindow).toHaveBeenCalledTimes(1);
    // The navigation was intercepted: the page did not change.
    expect(screen.getByText("page content")).toBeDefined();

    fireEvent(link, new MouseEvent("auxclick", { button: 1, bubbles: true }));
    expect(openModuleWindow).toHaveBeenCalledTimes(2);

    fireEvent.click(link);
    expect(openModuleWindow).toHaveBeenCalledTimes(2);
    expect(screen.getByText("docker content")).toBeDefined();
  });

  it("renders only the page in a popped-out window", () => {
    popoutId = "docker";
    renderShell(1400, "/docker");
    expect(screen.queryByRole("navigation", { name: "Main navigation" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Search modules" })).toBeNull();
    expect(screen.queryByRole("button", { name: /Ada Lovelace/ })).toBeNull();
    expect(screen.getByText("docker content")).toBeDefined();
    expect(document.title).toContain("Docker");
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
