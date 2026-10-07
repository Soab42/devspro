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
  /** Everything a saved selection could name. */
  selection: string[];
  marker: string | RegExp;
  /** Starts the first scan; tabs that scan on their own have none. */
  start?: () => Promise<void>;
}

const click = async (name: string | RegExp) => {
  const button = await screen.findByRole("button", { name });
  await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(button);
};

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
    selection: [],
    marker: "Safe: 2.0 KB",
    start: () => click("Scan everything"),
  },
  caches: {
    tab: "caches",
    Tab: CachesTab,
    resultKey: "caches",
    scans: { cleaner_scan_caches: [FINDING], cleaner_scan_system: [] },
    slots: { caches: [FINDING], system: [], error: null, systemError: null },
    selection: ["npm"],
    marker: "npm cache",
    start: () => click("Scan"),
  },
  docker: {
    tab: "docker",
    Tab: DockerTab,
    resultKey: "result",
    scans: { cleaner_scan_docker: { available: true, message: null, findings: [DOCKER_FINDING] } },
    slots: { result: { available: true, message: null, findings: [DOCKER_FINDING] }, error: null },
    selection: ["image:1"],
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
    selection: [FILE.path],
    marker: FILE.path,
    start: () => click("Scan"),
  },
  "storage-map": {
    tab: "storage-map",
    Tab: StorageMapTab,
    resultKey: "tree",
    scans: { cleaner_scan_tree: TREE },
    slots: { root: "/r", tree: TREE, zoomPath: "/r", expanded: [] },
    selection: ["/r/a.log"],
    marker: "File types",
    start: () => click("Scan"),
  },
  projects: {
    tab: "projects",
    Tab: ProjectsTab,
    resultKey: "rows",
    scans: { cleaner_scan_projects: [PROJECT] },
    slots: { rows: [PROJECT], error: null },
    selection: [PROJECT.artifact_path],
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

  it("restores the saved results after a restart, without scanning, and ticks nothing", async () => {
    scans = c.scans;
    snapshots[c.tab] = snapshotOf(c, Date.now() - 5 * 60_000, { selected: c.selection });
    mount(c.Tab);
    expect(await screen.findByText(c.marker)).toBeTruthy();
    expect(screen.getByText(/Scanned 5 minutes ago/)).toBeTruthy();
    expect(screen.queryByText(/may be out of date/)).toBeNull();
    await new Promise((r) => setTimeout(r, 20));
    expect(scanCalls()).toEqual([]);
    expect(screen.queryAllByRole("checkbox").filter((box) => (box as HTMLInputElement).checked)).toEqual([]);
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
