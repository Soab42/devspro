import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { CleanOutcome, CleanerSettings, ProjectJunk } from "../api";

const invoke = vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>();
const open = vi.fn<() => Promise<string | null>>();

vi.mock("@tauri-apps/api/core", () => ({ invoke: (c: string, a?: Record<string, unknown>) => invoke(c, a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: () => open() }));

import ProjectsTab from "./ProjectsTab";
import { resetTabState } from "../tabState";

const MB = 1024 * 1024;
const row = (over: Partial<ProjectJunk>): ProjectJunk => ({
  project_path: "/w/old",
  project_name: "old",
  artifact_path: "/w/old/node_modules",
  artifact_kind: "node_modules",
  bytes: 200 * MB,
  last_activity: 1,
  idle_days: 90,
  suggested: true,
  ...over,
});

const ROWS = [
  row({}),
  row({ artifact_path: "/w/old/.next", artifact_kind: ".next", bytes: 50 * MB }),
  row({
    project_path: "/w/new",
    project_name: "new",
    artifact_path: "/w/new/target",
    artifact_kind: "target",
    bytes: 10 * MB,
    idle_days: 2,
    suggested: false,
  }),
];

const SETTINGS: CleanerSettings = {
  scan_roots: [],
  project_roots: ["/w"],
  exclusions: [],
  large_file_min_bytes: 1,
  project_idle_days: 30,
};

function respond(rows: ProjectJunk[]) {
  invoke.mockImplementation(async (cmd, args) => {
    if (cmd === "cleaner_scan_projects") return rows;
    if (cmd === "cleaner_settings_get") return SETTINGS;
    if (cmd === "cleaner_settings_set") return args?.settings;
    if (cmd === "cleaner_clean_projects") {
      return (args?.artifactPaths as string[]).map<CleanOutcome>((id) => ({ id, ok: true, message: "Deleted", bytes_freed: MB }));
    }
    return undefined;
  });
}

beforeEach(() => {
  resetTabState();
  invoke.mockReset();
  open.mockReset();
});
afterEach(cleanup);

describe("ProjectsTab", () => {
  it("groups artifacts by project with total and idle days", async () => {
    respond(ROWS);
    render(<ProjectsTab />);
    expect(await screen.findByText("old")).toBeTruthy();
    expect(screen.getByText("250 MB")).toBeTruthy();
    expect(screen.getByText("90 days")).toBeTruthy();
    expect(screen.getByLabelText("node_modules 200 MB")).toBeTruthy();
    expect(screen.getByLabelText("target 10.0 MB")).toBeTruthy();
  });

  it("selects the suggested artifacts, reviews, cleans and rescans", async () => {
    respond(ROWS);
    render(<ProjectsTab />);
    await screen.findByText("old");
    fireEvent.click(screen.getByRole("button", { name: "Select suggested" }));
    expect((screen.getByLabelText("node_modules 200 MB") as HTMLInputElement).checked).toBe(true);
    expect((screen.getByLabelText("target 10.0 MB") as HTMLInputElement).checked).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: /^Clean 250 MB/ }));
    // The modal's confirm button is the later of the two with this name.
    await waitFor(() => expect(screen.getAllByRole("button", { name: /^Clean 250 MB/ })).toHaveLength(2));
    fireEvent.click(screen.getAllByRole("button", { name: /^Clean 250 MB/ })[1]);
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cleaner_clean_projects", {
        artifactPaths: ["/w/old/node_modules", "/w/old/.next"],
      }),
    );
    const scans = () => invoke.mock.calls.filter((c) => c[0] === "cleaner_scan_projects").length;
    expect(scans()).toBe(1);
    await waitFor(() => expect(screen.getAllByRole("button", { name: "Close" }).length).toBeGreaterThan(0));
    const closes = screen.getAllByRole("button", { name: "Close" });
    fireEvent.click(closes[closes.length - 1]);
    await waitFor(() => expect(scans()).toBe(2));
  });

  it("offers Add project folder when nothing is found and saves the choice", async () => {
    respond([]);
    open.mockResolvedValue("/home/me/code");
    render(<ProjectsTab />);
    fireEvent.click(await screen.findByRole("button", { name: "Add project folder" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cleaner_settings_set", {
        settings: { ...SETTINGS, project_roots: ["/w", "/home/me/code"] },
      }),
    );
    await waitFor(() => expect(invoke.mock.calls.filter((c) => c[0] === "cleaner_scan_projects").length).toBe(2));
  });

  it("cancels a running scan", async () => {
    invoke.mockImplementation(async (cmd) => (cmd === "cleaner_scan_projects" ? new Promise(() => {}) : undefined));
    render(<ProjectsTab />);
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cleaner_cancel_scan", { scanId: expect.stringMatching(/^scan-/) }),
    );
  });

  it("still offers Add project folder when the first scan fails", async () => {
    invoke.mockImplementation(async (cmd) => {
      if (cmd === "cleaner_scan_projects") throw new Error("scan broke");
      return undefined;
    });
    render(<ProjectsTab />);
    expect(await screen.findByText("scan broke")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Add project folder" })).toBeTruthy();
  });
});
