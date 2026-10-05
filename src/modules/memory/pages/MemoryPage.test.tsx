import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import type { AppDef, AppRow, MemorySnapshot, ResetPreview, WatchdogConfig, WatchdogEvent } from "../api";
import MemoryPage from "./MemoryPage";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
/** The handler the page registered for each event, so a test can fire one. */
const listeners: Record<string, (e: { payload: unknown }) => void> = {};
const unlisten = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (e: { payload: unknown }) => void) => {
    listeners[name] = handler;
    return Promise.resolve(unlisten);
  },
}));

const row = (over: Partial<AppRow>): AppRow => ({
  id: "x",
  name: "X",
  category: "Other",
  kind: "process",
  detected: false,
  state: "on",
  ram_mb: 100,
  cpu_pct: 0.5,
  process_count: 1,
  can_start: false,
  needs_admin: false,
  protected: false,
  ...over,
});

const APPS: AppRow[] = [
  row({ id: "chrome", name: "Chrome", category: "Browsers", ram_mb: 2150, cpu_pct: 12.3, process_count: 14 }),
  row({ id: "firefox", name: "Firefox", category: "Browsers", state: "idle", ram_mb: 900, process_count: 5 }),
  row({
    id: "gimp",
    name: "GIMP",
    category: "Creative",
    kind: "flatpak",
    state: "off",
    ram_mb: 0,
    process_count: 0,
    can_start: true,
  }),
  row({ id: "nginx", name: "Nginx", category: "Servers", kind: "service", ram_mb: 40, needs_admin: true }),
  row({ id: "auto:slack", name: "slack", category: "Communication", detected: true, ram_mb: 600 }),
  row({ id: "auto:devspro", name: "DevsPro", category: "Dev", detected: true, ram_mb: 300, protected: true }),
];

const CATALOG: AppDef[] = [
  {
    id: "chrome",
    name: "Chrome",
    category: "Browsers",
    kind: "process",
    matcher: "/opt/google/chrome/chrome",
    start: "/opt/google/chrome/chrome",
    flatpak_id: null,
    service: null,
  },
];

const PREVIEW: ResetPreview = {
  processes: [
    { pid: 101, start_time: 5001, name: "chrome", command: "/opt/google/chrome/chrome", ram_mb: 1500, app: "Chrome" },
    { pid: 102, start_time: 5002, name: "chrome", command: "/opt/google/chrome/chrome --type=renderer", ram_mb: 650, app: "Chrome" },
    { pid: 201, start_time: 6001, name: "firefox", command: "/usr/lib/firefox/firefox", ram_mb: 900, app: "Firefox" },
  ],
  services: ["nginx"],
};

let watchdog: WatchdogConfig;

function snapshot(): MemorySnapshot {
  return {
    taken_at_ms: 0,
    memory: { total_mb: 32768, available_mb: 5222, used_pct: 84, swap_total_mb: 8192, swap_used_mb: 6758 },
    apps: APPS,
    watchdog,
  };
}

function route(cmd: string, args: Record<string, unknown>) {
  switch (cmd) {
    case "memory_snapshot":
      return snapshot();
    case "memory_watchdog_get":
      return watchdog;
    case "memory_watchdog_set":
      watchdog = args.config as WatchdogConfig;
      return watchdog;
    case "memory_watchdog_log":
      return [];
    case "memory_stop":
      return (args.ids as string[]).map((id) => ({ id, ok: true, message: "" }));
    case "memory_start":
      return { id: args.id, ok: true, message: "" };
    case "memory_drop_caches":
      return [
        { success: true, message: "dropped" },
        { success: true, message: "reclaimed" },
      ];
    case "memory_reset_preview":
      return PREVIEW;
    case "memory_reset":
      return { killed: 2, failed: [], admin: [{ success: true, message: "Stopped nginx" }] };
    case "memory_apps_get":
      return CATALOG;
    default:
      return undefined;
  }
}

const calls = (command: string) =>
  invoke.mock.calls.filter(([cmd]) => cmd === command).map(([, args]) => args);

function renderPage() {
  return render(
    <ConfirmProvider>
      <MemoryPage />
    </ConfirmProvider>,
  );
}

const appsTable = () => screen.getByRole("table", { name: "Apps and the memory they use" });
const appNames = () =>
  within(appsTable())
    .getAllByRole("row")
    .slice(1)
    .map((tr) => tr.querySelector("td")?.textContent ?? "");
const rowOf = (name: string) => within(appsTable()).getByText(name).closest("tr") as HTMLElement;

let hidden = false;
Object.defineProperty(document, "visibilityState", {
  configurable: true,
  get: () => (hidden ? "hidden" : "visible"),
});
function setHidden(next: boolean) {
  hidden = next;
  act(() => {
    document.dispatchEvent(new Event("visibilitychange"));
  });
}

beforeEach(() => {
  cleanup();
  hidden = false;
  watchdog = { enabled: true, threshold_pct: 95, prefer_idle: true };
  invoke.mockReset();
  invoke.mockImplementation(async (cmd: string, args: Record<string, unknown>) => route(cmd, args));
});

afterEach(() => {
  vi.useRealTimers();
});

describe("MemoryPage", () => {
  it("shows RAM, swap and the watchdog, with the largest app first", async () => {
    renderPage();
    await screen.findByText("Chrome");

    expect(document.body.textContent).toContain("5.1 GB free of 32.0 GB");
    expect(screen.getByText("Swap: 6.6 GB used of 8.0 GB")).toBeDefined();
    expect(screen.getByText("Watchdog on — closes an app at 95%")).toBeDefined();
    expect(appNames()[0]).toMatch(/^Chrome/);
    expect(within(rowOf("Chrome")).getByText("2.1 GB")).toBeDefined();
    expect(within(rowOf("Firefox")).getByText("900 MB")).toBeDefined();
    expect(within(rowOf("Firefox")).getByText("Idle")).toBeDefined();
    expect(within(rowOf("slack")).getByText("Detected")).toBeDefined();
  });

  it("filters by category and by name", async () => {
    renderPage();
    await screen.findByText("Chrome");

    fireEvent.click(screen.getByRole("radio", { name: /^Browsers/ }));
    expect(appNames()).toEqual(["Chrome", "Firefox"]);

    fireEvent.change(screen.getByRole("searchbox", { name: "Filter by app name" }), {
      target: { value: "fire" },
    });
    expect(appNames()).toEqual(["Firefox"]);
  });

  it("stops one app after a confirm that names it", async () => {
    renderPage();
    await screen.findByText("Chrome");

    fireEvent.click(within(rowOf("Chrome")).getByRole("button", { name: "Stop Chrome" }));
    const dialog = screen.getByRole("dialog", { name: "Stop Chrome?" });
    expect(dialog.textContent).toContain("Closes Chrome and its 14 processes, freeing about 2.1 GB.");
    fireEvent.click(within(dialog).getByRole("button", { name: "Stop" }));

    await screen.findByText("Stopped Chrome.");
    expect(calls("memory_stop")).toEqual([{ ids: ["chrome"] }]);
  });

  it("starts a stopped app", async () => {
    renderPage();
    await screen.findByText("Chrome");

    fireEvent.click(within(rowOf("GIMP")).getByRole("button", { name: "Start GIMP" }));
    await screen.findByText("Started GIMP.");
    expect(calls("memory_start")).toEqual([{ id: "gimp" }]);
  });

  it("marks password-prompting actions, and won't stop a protected app", async () => {
    renderPage();
    await screen.findByText("Chrome");

    expect(
      within(rowOf("Nginx")).getByRole("button", {
        name: "Stop Nginx (asks for your admin password)",
      }),
    ).toBeDefined();
    const devspro = within(rowOf("DevsPro")).getByRole("button", { name: /can't be stopped/ });
    expect((devspro as HTMLButtonElement).disabled).toBe(true);
  });

  it("stops everything in a category after listing it", async () => {
    renderPage();
    await screen.findByText("Chrome");
    expect(screen.queryByRole("button", { name: /^Stop all in/ })).toBeNull();

    fireEvent.click(screen.getByRole("radio", { name: /^Browsers/ }));
    fireEvent.click(screen.getByRole("button", { name: "Stop all in Browsers" }));
    const dialog = screen.getByRole("dialog", { name: "Stop all in Browsers?" });
    expect(within(dialog).getByText("Chrome")).toBeDefined();
    expect(within(dialog).getByText("Firefox")).toBeDefined();
    fireEvent.click(within(dialog).getByRole("button", { name: "Stop 2 apps" }));

    await screen.findByText("Stopped 2 apps in Browsers.");
    expect(calls("memory_stop")).toEqual([{ ids: ["chrome", "firefox"] }]);
  });

  it("turns the watchdog off", async () => {
    renderPage();
    await screen.findByText("Watchdog on — closes an app at 95%");

    fireEvent.click(screen.getByRole("switch", { name: "Close an app when memory runs out" }));

    await screen.findByText("Watchdog off");
    expect(calls("memory_watchdog_set")).toEqual([
      { config: { enabled: false, threshold_pct: 95, prefer_idle: true } },
    ]);
  });

  it("says when the watchdog closed an app", async () => {
    renderPage();
    await screen.findByText("Chrome");
    await waitFor(() => expect(listeners["memory://watchdog"]).toBeDefined());

    const event: WatchdogEvent = {
      at_ms: Date.now(),
      app: "Chrome",
      ram_mb: 2150,
      ram_pct: 96,
      reason: "idle",
      ok: true,
      message: null,
    };
    act(() => listeners["memory://watchdog"]({ payload: event }));

    expect(screen.getByText("Watchdog closed Chrome (2.1 GB) — RAM was 96%")).toBeDefined();
  });

  it("confirms dropping the cache, and says it asks for the admin password", async () => {
    renderPage();
    await screen.findByText("Chrome");

    fireEvent.click(screen.getByRole("button", { name: "Reclaim swap" }));
    const dialog = screen.getByRole("dialog", { name: "Drop cache and reclaim swap?" });
    expect(dialog.textContent).toContain("This asks for your admin password.");
    expect(calls("memory_drop_caches")).toEqual([]);

    fireEvent.click(within(dialog).getByRole("button", { name: "Drop cache and reclaim swap" }));
    await screen.findByText("Dropped the cache and reclaimed swap.");
    expect(calls("memory_drop_caches")).toEqual([{ reclaimSwap: true }]);
  });

  it("runs a full reset only once YES is typed, and leaves out what was unticked", async () => {
    renderPage();
    await screen.findByText("Chrome");

    fireEvent.click(screen.getByRole("button", { name: "Full reset…" }));
    const dialog = screen.getByRole("dialog", { name: "Full reset" });
    await within(dialog).findByRole("checkbox", { name: "Chrome" });

    const go = () => within(dialog).getByRole("button", { name: /^Stop / }) as HTMLButtonElement;
    expect(go().textContent).toBe("Stop 3 processes and 1 service");
    expect(go().disabled).toBe(true);

    fireEvent.click(within(dialog).getByRole("checkbox", { name: "Firefox" }));
    expect(go().textContent).toBe("Stop 2 processes and 1 service");

    const yes = within(dialog).getByLabelText("Type YES to confirm");
    fireEvent.change(yes, { target: { value: "no" } });
    expect(go().disabled).toBe(true);
    fireEvent.click(go());
    expect(calls("memory_reset")).toEqual([]);

    fireEvent.change(yes, { target: { value: "YES" } });
    expect(go().disabled).toBe(false);
    fireEvent.click(go());

    await within(dialog).findByText("Stopped 2 processes.");
    expect(calls("memory_reset")).toEqual([
      {
        request: {
          processes: [
            [101, 5001],
            [102, 5002],
          ],
          services: ["nginx"],
          drop_caches: true,
        },
      },
    ]);
  });

  it("adds an app to the catalog and saves the whole list", async () => {
    renderPage();
    await screen.findByText("Chrome");

    fireEvent.click(screen.getByRole("button", { name: "Edit apps…" }));
    await within(screen.getByRole("dialog", { name: "Edit apps" })).findByText(
      "/opt/google/chrome/chrome",
    );
    fireEvent.click(screen.getByRole("button", { name: "Add app" }));

    const form = screen.getByRole("dialog", { name: "Add app" });
    fireEvent.click(within(form).getByRole("button", { name: "Add" }));
    expect(within(form).getByText("Give it a name.")).toBeDefined();

    fireEvent.change(within(form).getByLabelText(/^Name/), { target: { value: "Redis" } });
    fireEvent.click(within(form).getByRole("radio", { name: "Service" }));
    fireEvent.change(within(form).getByLabelText(/^Command line contains/), {
      target: { value: "redis-server" },
    });
    fireEvent.change(within(form).getByLabelText(/^systemd unit/), {
      target: { value: "redis-server" },
    });
    fireEvent.click(within(form).getByRole("button", { name: "Add" }));

    const list = screen.getByRole("dialog", { name: "Edit apps" });
    expect(within(list).getByText("Redis")).toBeDefined();
    fireEvent.click(within(list).getByRole("button", { name: "Save" }));

    await screen.findByText("Saved 2 apps.");
    expect(calls("memory_apps_set")).toEqual([
      {
        apps: [
          CATALOG[0],
          {
            id: "redis",
            name: "Redis",
            category: "Other",
            kind: "service",
            matcher: "redis-server",
            start: null,
            flatpak_id: null,
            service: "redis-server",
          },
        ],
      },
    ]);
  });

  it("polls only while the window is showing, and stops once the page is gone", async () => {
    vi.useFakeTimers();
    const { unmount } = renderPage();
    const polls = () => calls("memory_snapshot").length;
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(polls()).toBe(1);

    await act(() => vi.advanceTimersByTimeAsync(4000));
    expect(polls()).toBe(2);

    setHidden(true);
    await act(() => vi.advanceTimersByTimeAsync(20000));
    expect(polls()).toBe(2);

    // Back in view: refreshed straight away, then on the interval again.
    setHidden(false);
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(polls()).toBe(3);

    unmount();
    await act(() => vi.advanceTimersByTimeAsync(20000));
    expect(polls()).toBe(3);
    expect(unlisten).toHaveBeenCalled();
  });
});
