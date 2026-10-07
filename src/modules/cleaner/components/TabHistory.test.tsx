import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import type { ComponentType } from "react";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import type { Finding, LargeFile, ProjectJunk, StorageTree } from "../api";
import { flushSnapshots, resetTabState, STALE_AFTER_MS } from "../tabState";
import type { TabId } from "../tabState";
import CachesTab from "./CachesTab";
import DockerTab from "./DockerTab";
import LargeFilesTab from "./LargeFilesTab";
import OverviewTab from "./OverviewTab";
import ProjectsTab from "./ProjectsTab";
import StorageMapTab from "./StorageMapTab";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("../../../app/ModuleRegistry", () => ({ useModule: () => ({ label: "Cleaner", icon: () => null }) }));
const onCloseRequested = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ onCloseRequested }) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => null) }));

const MB = 1024 * 1024;
const finding = (over: Partial<Finding>): Finding => ({
  id: "npm",
  tab: "caches",
  group: "JavaScript",
  label: "npm cache",
  reason: "rebuilds itself",
  method: "delete",
  paths: ["/home/u/.npm"],
  bytes: 2048,
  risk: "safe",
  suggested: true,
  blocked_by: null,
  needs_confirm_text: null,
  ...over,
});
const FINDING = finding({});
const DOCKER_FINDING = finding({ id: "image:1", tab: "docker", group: "Dangling images", label: "Untagged image 1" });
const FILE: LargeFile = { path: "/home/u/movie.mkv", bytes: 900 * MB, modified: 1, accessed: 1, kind: "video" };
const PROJECT: ProjectJunk = {
  project_path: "/w/old",
  project_name: "old-project",
  artifact_path: "/w/old/node_modules",
  artifact_kind: "node_modules",
  bytes: 200 * MB,
  last_activity: 1,
  idle_days: 90,
  suggested: true,
};
const TREE: StorageTree = {
  root: {
    name: "r",
    path: "/r",
    bytes: 1000,
    files: 1,
    is_dir: true,
    children: [{ name: "a.log", path: "/r/a.log", bytes: 1000, files: 1, is_dir: false, children: [] }],
  },
  extensions: [{ ext: "log", kind: "log", bytes: 1000, files: 1 }],
  total_bytes: 1000,
  files: 1,
  truncated: false,
};
const OVERVIEW_RESULTS = {
  caches: { findings: [FINDING], error: null, note: null },
  system: { findings: [], error: null, note: null },
  docker: { findings: [], error: null, note: "Docker is not available." },
  projects: { findings: [], error: null, note: null },
};

interface Case {
  tab: TabId;
  Tab: ComponentType;
  /** What the backend answers when the tab scans. */
  scans: Record<string, unknown>;
  /** The persisted slots a snapshot of this tab holds. */
  slots: Record<string, unknown>;
  /** The slot that holds the results themselves. */
  resultKey: string;
  /** Ticks or picks something through the tab's own selection mechanism. Overview has none. */
  select?: () => Promise<void>;
  /** Whether anything is ticked or picked. */
  anySelected?: () => boolean;
  marker: string | RegExp;
  /** Starts the first scan; tabs that scan on their own have none. */
  start?: () => Promise<void>;
}

const click = async (name: string | RegExp) => {
  const button = await screen.findByRole("button", { name });
  await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(button);
};

const tick = async (name: string | RegExp) => {
  fireEvent.click(await screen.findByRole("checkbox", { name }));
};
const pick = async (name: string) => {
  fireEvent.click(await screen.findByRole("button", { name }));
};
const anyTicked = () => screen.queryAllByRole("checkbox").some((box) => (box as HTMLInputElement).checked);
const anyPicked = () => screen.queryAllByRole("button").some((b) => b.getAttribute("aria-pressed") === "true");

const CASES: Record<string, Case> = {
  overview: {
    tab: "overview",
    Tab: OverviewTab,
    resultKey: "results",
    scans: {
      cleaner_scan_caches: [FINDING],
      cleaner_scan_system: [],
      cleaner_scan_docker: { available: false, message: "Docker is not available.", findings: [] },
      cleaner_scan_projects: [],
    },
    slots: { results: OVERVIEW_RESULTS },
    marker: "Safe: 2.0 KB",
    start: () => click("Scan everything"),
  },
  caches: {
    tab: "caches",
    Tab: CachesTab,
    resultKey: "caches",
    scans: { cleaner_scan_caches: [FINDING], cleaner_scan_system: [] },
    slots: { caches: [FINDING], system: [], error: null, systemError: null },
    select: () => tick("Select npm cache"),
    anySelected: anyTicked,
    marker: "npm cache",
    start: () => click("Scan"),
  },
  docker: {
    tab: "docker",
    Tab: DockerTab,
    resultKey: "result",
    scans: { cleaner_scan_docker: { available: true, message: null, findings: [DOCKER_FINDING] } },
    slots: { result: { available: true, message: null, findings: [DOCKER_FINDING] }, error: null },
    select: () => tick("Select Untagged image 1"),
    anySelected: anyTicked,
    marker: "Untagged image 1",
  },
  "large-files": {
    tab: "large-files",
    Tab: LargeFilesTab,
    resultKey: "files",
    scans: { cleaner_scan_large_files: [FILE] },
    slots: {
      view: "files",
      minSize: String(500 * MB),
      kind: "all",
      age: "0",
      files: [FILE],
      scannedFilters: { min: 500 * MB, age: 0 },
      foldersRoot: "",
      foldersPath: "",
    },
    select: () => tick("Select /home/u/movie.mkv"),
    anySelected: anyTicked,
    marker: FILE.path,
    start: () => click("Scan"),
  },
  "storage-map": {
    tab: "storage-map",
    Tab: StorageMapTab,
    resultKey: "tree",
    scans: { cleaner_scan_tree: TREE },
    slots: { root: "/r", tree: TREE, zoomPath: "/r", expanded: [] },
    select: () => pick("Treemap a.log"),
    anySelected: anyPicked,
    marker: "File types",
    start: () => click("Scan"),
  },
  projects: {
    tab: "projects",
    Tab: ProjectsTab,
    resultKey: "rows",
    scans: { cleaner_scan_projects: [PROJECT] },
    slots: { rows: [PROJECT], error: null },
    select: () => tick(/node_modules/),
    anySelected: anyTicked,
    marker: "old-project",
  },
};

let snapshots: Record<string, string>;
let scans: Record<string, unknown>;

function mount(Tab: ComponentType) {
  return render(
    <MemoryRouter>
      <ConfirmProvider>
        <Tab />
      </ConfirmProvider>
    </MemoryRouter>,
  );
}

const snapshotOf = (c: Case, scannedAt: number, extra: Record<string, unknown> = {}) =>
  JSON.stringify({ v: 1, slots: { ...c.slots, scannedAt, ...extra } });

const scanCalls = () => invoke.mock.calls.filter(([cmd]) => String(cmd).startsWith("cleaner_scan_"));

beforeEach(() => {
  resetTabState();
  snapshots = {};
  scans = {};
  invoke.mockReset();
  invoke.mockImplementation(async (cmd: string, args?: { tab?: string }) => {
    switch (cmd) {
      case "cleaner_snapshot_get":
        return snapshots[args?.tab ?? ""] ?? null;
      case "cleaner_snapshot_set":
      case "cleaner_snapshot_clear":
        return true;
      case "cleaner_settings_get":
        return { scan_roots: ["/r"], project_roots: [], exclusions: [], large_file_min_bytes: 500 * MB, project_idle_days: 30 };
      case "cleaner_history_list":
        return [];
      default:
        if (cmd in scans) return scans[cmd];
        return undefined;
    }
  });
});
afterEach(cleanup);

describe.each(Object.entries(CASES))("%s tab history", (_name, c) => {
  it("keeps its results when the tab is left and entered again", async () => {
    scans = c.scans;
    const first = mount(c.Tab);
    await c.start?.();
    await screen.findByText(c.marker);
    const scansSoFar = scanCalls().length;
    first.unmount();

    mount(c.Tab);
    expect(await screen.findByText(c.marker)).toBeTruthy();
    expect(screen.getByText(/Scanned/)).toBeTruthy();
    // Coming back never scans again.
    await new Promise((r) => setTimeout(r, 20));
    expect(scanCalls().length).toBe(scansSoFar);
  });

  it("restores the saved results after a restart, without scanning", async () => {
    scans = c.scans;
    snapshots[c.tab] = snapshotOf(c, Date.now() - 5 * 60_000);
    mount(c.Tab);
    expect(await screen.findByText(c.marker)).toBeTruthy();
    expect(screen.getByText(/Scanned 5 minutes ago/)).toBeTruthy();
    expect(screen.queryByText(/may be out of date/)).toBeNull();
    await new Promise((r) => setTimeout(r, 20));
    expect(scanCalls()).toEqual([]);
  });

  it("says results older than a day may be out of date", async () => {
    snapshots[c.tab] = snapshotOf(c, Date.now() - STALE_AFTER_MS - 60_000);
    mount(c.Tab);
    await screen.findByText(c.marker);
    expect(screen.getByText(/may be out of date/)).toBeTruthy();
  });

  it("saves its results once a scan has finished", async () => {
    scans = c.scans;
    mount(c.Tab);
    await c.start?.();
    await screen.findByText(c.marker);
    await waitFor(() => expect(screen.getByText(/Scanned/)).toBeTruthy());
    await flushSnapshots();
    const saved = invoke.mock.calls.find(([cmd, args]) => cmd === "cleaner_snapshot_set" && args.tab === c.tab);
    expect(saved).toBeTruthy();
    const doc = JSON.parse(saved?.[1].json);
    expect(doc.slots).toHaveProperty(c.resultKey);
    expect(doc.slots.scannedAt).toBeTypeOf("number");
    expect(doc.slots).not.toHaveProperty("selected");
  });
});

describe("snapshot handling", () => {
  it("keeps a snapshot too big for the backend in memory only", async () => {
    scans = CASES.caches.scans;
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cleaner_snapshot_set") return false;
      if (cmd === "cleaner_snapshot_get") return null;
      return scans[cmd];
    });
    mount(CachesTab);
    await click("Scan");
    await screen.findByText("npm cache");
    await flushSnapshots();
    // The backend refused: the older saved copy is dropped, not left to return after a restart.
    expect(invoke).toHaveBeenCalledWith("cleaner_snapshot_clear", { tab: "caches" });
    expect(screen.getByText("npm cache")).toBeTruthy();
  });

  it("ignores a snapshot it cannot read, and one the backend fails to give", async () => {
    snapshots.caches = "{not json";
    mount(CachesTab);
    expect(await screen.findByRole("button", { name: "Scan" })).toBeTruthy();
    cleanup();
    resetTabState();
    snapshots.caches = JSON.stringify({ v: 1, slots: { caches: "nope", scannedAt: "yesterday" } });
    mount(CachesTab);
    expect(await screen.findByRole("button", { name: "Scan" })).toBeTruthy();
    cleanup();
    resetTabState();
    invoke.mockImplementation(async () => {
      throw new Error("backend down");
    });
    mount(CachesTab);
    expect(await screen.findByRole("button", { name: "Scan" })).toBeTruthy();
  });

  it("restores the filters and position, not just the results", async () => {
    const large = CASES["large-files"];
    snapshots["large-files"] = snapshotOf(large, Date.now() - 60_000, { kind: "video", age: "30", minSize: String(100 * MB) });
    mount(LargeFilesTab);
    await screen.findByText(/Scanned/);
    expect(screen.getAllByRole("button", { name: "Kind" })[0].textContent).toMatch(/Video/);
    expect(screen.getByRole("radio", { name: "100 MB" }).getAttribute("aria-checked")).toBe("true");

    const map = CASES["storage-map"];
    snapshots["storage-map"] = snapshotOf(map, Date.now() - 60_000, { zoomPath: "/r", expanded: ["/r"] });
    mount(StorageMapTab);
    await screen.findByRole("group", { name: "Treemap" });
  });

  it("lets a scan that is still running finish into the store after the tab was left", async () => {
    let finish: (v: unknown) => void = () => {};
    scans = {};
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cleaner_scan_projects") return new Promise((r) => (finish = r));
      return undefined;
    });
    const first = mount(ProjectsTab);
    await screen.findByText("Scanning…");
    first.unmount();

    mount(ProjectsTab);
    expect(await screen.findByText("Scanning…")).toBeTruthy();
    finish([PROJECT]);
    expect(await screen.findByText("old-project")).toBeTruthy();
    expect(scanCalls().length).toBe(1);
  });
});

// The Overview has no selection, so it is left out.
describe.each(Object.entries(CASES).filter(([, c]) => c.select))("%s tab selection", (_name, c) => {
  it("is kept across tab switches but not restored after a restart", async () => {
    scans = c.scans;
    const first = mount(c.Tab);
    await c.start?.();
    await screen.findByText(c.marker);
    await c.select?.();
    expect(c.anySelected?.()).toBe(true);
    first.unmount();
    mount(c.Tab);
    await screen.findByText(c.marker);
    expect(c.anySelected?.()).toBe(true);
    cleanup();

    await flushSnapshots();
    const saved = invoke.mock.calls.filter(([cmd, args]) => cmd === "cleaner_snapshot_set" && args.tab === c.tab).pop();
    expect(saved).toBeTruthy();
    // A restart: memory is gone, only what was saved comes back.
    resetTabState();
    snapshots[c.tab] = saved?.[1].json;
    invoke.mockClear();
    mount(c.Tab);
    await screen.findByText(c.marker);
    expect(c.anySelected?.()).toBe(false);
    expect(scanCalls()).toEqual([]);
  });
});

describe("position survives leaving the tab", () => {
  it("keeps the Storage map zoom and expanded rows", async () => {
    const dir = (path: string, children: StorageTree["root"]["children"]) => ({
      name: path.split("/").pop()!,
      path,
      bytes: 700,
      files: 2,
      is_dir: true,
      children,
    });
    const leaf = (path: string) => ({ name: path.split("/").pop()!, path, bytes: 300, files: 1, is_dir: false, children: [] });
    const tree: StorageTree = {
      ...TREE,
      root: { ...TREE.root, children: [dir("/r/big", [dir("/r/big/sub", [leaf("/r/big/sub/x.bin")]), leaf("/r/big/a.mp4")]), leaf("/r/b.log")] },
    };
    scans = { cleaner_scan_tree: tree };
    const first = mount(StorageMapTab);
    await click("Scan");
    fireEvent.click(await screen.findByRole("button", { name: "Treemap big" }));
    fireEvent.click(await screen.findByRole("button", { name: "Expand sub" }));
    await screen.findByRole("button", { name: "Collapse sub" });
    first.unmount();

    mount(StorageMapTab);
    expect(await screen.findByRole("button", { name: "Collapse sub" })).toBeTruthy();
    expect(screen.getByText("big").tagName).toBe("STRONG");
  });

  it("keeps the Biggest folders path", async () => {
    const entry = (path: string, is_dir: boolean) => ({ path, name: path.split("/").pop(), bytes: 5 * MB, is_dir, entries: 1 });
    invoke.mockImplementation(async (cmd: string, args?: { path?: string }) => {
      if (cmd === "cleaner_settings_get") return { scan_roots: ["/r"], project_roots: [], exclusions: [], large_file_min_bytes: 500 * MB, project_idle_days: 30 };
      if (cmd === "cleaner_folder_sizes") return args?.path === "/r" ? [entry("/r/Videos", true)] : [entry("/r/Videos/a.mp4", false)];
      return undefined;
    });
    const first = mount(LargeFilesTab);
    fireEvent.click(await screen.findByRole("radio", { name: "Biggest folders" }));
    fireEvent.click(await screen.findByRole("button", { name: "Videos" }));
    await screen.findByText("a.mp4");
    first.unmount();

    mount(LargeFilesTab);
    expect(await screen.findByText("a.mp4")).toBeTruthy();
    expect(screen.getByRole("navigation", { name: "Folder path" }).textContent).toContain("Videos");
  });
});

describe("after a clean", () => {
  const lastSaved = (tab: string) => {
    const call = invoke.mock.calls.filter(([cmd, args]) => cmd === "cleaner_snapshot_set" && args.tab === tab).pop();
    return call ? JSON.parse(call[1].json).slots : null;
  };

  it("a trimming tab saves its results without the removed files, with the same scan time", async () => {
    const other: LargeFile = { ...FILE, path: "/home/u/other.iso" };
    const large = CASES["large-files"];
    const scannedAt = Date.now() - 60_000;
    snapshots["large-files"] = JSON.stringify({ v: 1, slots: { ...large.slots, files: [FILE, other], scannedAt } });
    invoke.mockImplementation(async (cmd: string, args?: { tab?: string; paths?: string[] }) => {
      if (cmd === "cleaner_snapshot_get") return snapshots[args?.tab ?? ""] ?? null;
      if (cmd === "cleaner_settings_get") return { scan_roots: ["/r"], project_roots: [], exclusions: [], large_file_min_bytes: 500 * MB, project_idle_days: 30 };
      if (cmd === "cleaner_remove_files") return (args?.paths ?? []).map((id) => ({ id, ok: true, message: "ok", bytes_freed: 1 }));
      return undefined;
    });
    mount(LargeFilesTab);
    await tick("Select /home/u/movie.mkv");
    await click("Move to Trash");
    await click(/^Clean/);
    await waitFor(() => expect(screen.queryByText("/home/u/movie.mkv")).toBeNull());
    await flushSnapshots();
    const saved = lastSaved("large-files");
    expect(saved.files.map((f: LargeFile) => f.path)).toEqual(["/home/u/other.iso"]);
    expect(saved.scannedAt).toBe(scannedAt);
    expect(saved.trimmed).toBe(true);
    expect(scanCalls()).toEqual([]);
    expect(screen.getByText(/Updated after clean/)).toBeTruthy();
  });

  it("a rescanning tab saves the new scan", async () => {
    const other: ProjectJunk = { ...PROJECT, project_path: "/w/b", project_name: "b-project", artifact_path: "/w/b/target", artifact_kind: "target" };
    let rows = [PROJECT, other];
    invoke.mockImplementation(async (cmd: string, args?: { tab?: string; artifactPaths?: string[] }) => {
      if (cmd === "cleaner_snapshot_get") return null;
      if (cmd === "cleaner_scan_projects") return rows;
      if (cmd === "cleaner_clean_projects") {
        rows = [other];
        return (args?.artifactPaths ?? []).map((id) => ({ id, ok: true, message: "Deleted", bytes_freed: 1 }));
      }
      return undefined;
    });
    mount(ProjectsTab);
    await tick(/node_modules/);
    await click(/^Clean 200 MB/);
    const buttons = await screen.findAllByRole("button", { name: /^Clean 200 MB/ });
    fireEvent.click(buttons[buttons.length - 1]);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cleaner_clean_projects", expect.anything()));
    fireEvent.click((await screen.findAllByRole("button", { name: "Close" })).pop() as HTMLElement);
    await waitFor(() => expect(scanCalls().length).toBe(2));
    await waitFor(() => expect(screen.queryByText("old-project")).toBeNull());
    await flushSnapshots();
    expect(lastSaved("projects").rows.map((r: ProjectJunk) => r.project_name)).toEqual(["b-project"]);
  });
});

describe("saving", () => {
  const setCalls = (tab: string) => invoke.mock.calls.filter(([cmd, args]) => cmd === "cleaner_snapshot_set" && args.tab === tab);

  it("saves results as soon as a scan finishes, without waiting for a timer", async () => {
    scans = CASES.caches.scans;
    mount(CachesTab);
    await click("Scan");
    await screen.findByText("npm cache");
    // The filter debounce is 400 ms; this must not need it.
    await waitFor(() => expect(setCalls("caches").length).toBeGreaterThan(0), { timeout: 250 });
    expect(JSON.parse(setCalls("caches")[0][1].json).slots.caches).toHaveLength(1);
  });

  it("flushes pending filter changes when the page is hidden", async () => {
    const { flushOnHide, setSlot } = await import("../tabState");
    const off = flushOnHide();
    setSlot("large-files", "kind", "video", "all");
    await new Promise((r) => setTimeout(r, 20));
    expect(setCalls("large-files")).toEqual([]);
    const state = vi.spyOn(document, "visibilityState", "get");
    state.mockReturnValue("visible");
    document.dispatchEvent(new Event("visibilitychange"));
    await new Promise((r) => setTimeout(r, 20));
    expect(setCalls("large-files")).toEqual([]);
    state.mockReturnValue("hidden");
    document.dispatchEvent(new Event("visibilitychange"));
    await waitFor(() => expect(setCalls("large-files").length).toBe(1), { timeout: 250 });
    expect(JSON.parse(setCalls("large-files")[0][1].json).slots.kind).toBe("video");
    state.mockRestore();
    off();
  });

  it("saves pending filters when the Cleaner page is left, and never hooks the window close", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const { default: CleanerPage } = await import("../pages/CleanerPage");
    const { setSlot } = await import("../tabState");
    const page = render(
      <MemoryRouter>
        <ConfirmProvider>
          <CleanerPage />
        </ConfirmProvider>
      </MemoryRouter>,
    );
    await screen.findByText(/Scan everything to see/);
    setSlot("large-files", "age", "30", "0");
    expect(setCalls("large-files")).toEqual([]);
    page.unmount();
    await waitFor(() => expect(setCalls("large-files").length).toBe(1), { timeout: 250 });
    // A close-requested handler would block the native close and override close-to-tray.
    expect(onCloseRequested).not.toHaveBeenCalled();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
});
