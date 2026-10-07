import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import type { Finding } from "../api";
import { LAST_SCAN_KEY } from "./overview/lastScan";
import OverviewTab from "./OverviewTab";
import { resetTabState } from "../tabState";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => null) }));

const MB = 1024 * 1024;
const f = (over: Partial<Finding>): Finding => ({
  id: "x",
  tab: "caches",
  group: "g",
  label: "X",
  reason: "",
  method: "delete",
  paths: ["/p"],
  bytes: MB,
  risk: "safe",
  suggested: true,
  blocked_by: null,
  needs_confirm_text: null,
  ...over,
});

const SNAPSHOT = {
  disks: [{ device: "/dev/sda1", mount_point: "/", fs_type: "ext4", total_bytes: 100 * MB, used_bytes: 50 * MB, available_bytes: 50 * MB }],
};
const SETTINGS = { scan_roots: [], project_roots: [], exclusions: [], large_file_min_bytes: 100 * MB, project_idle_days: 30 };

let scans: Record<string, () => unknown>;
let history: unknown[];

beforeEach(() => {
  resetTabState();
  localStorage.removeItem(LAST_SCAN_KEY);
  history = [];
  scans = {};
  invoke.mockReset();
  invoke.mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case "system_info_snapshot":
        return SNAPSHOT;
      case "cleaner_settings_get":
        return SETTINGS;
      case "cleaner_history_list":
        return history;
      case "cleaner_history_clear":
        history = [];
        return undefined;
      default:
        if (scans[cmd]) return scans[cmd]();
        throw "not implemented";
    }
  });
});
afterEach(cleanup);

function renderTab() {
  render(
    <ConfirmProvider>
      <OverviewTab />
    </ConfirmProvider>,
  );
}

const scanButton = () => screen.findByRole("button", { name: "Scan everything" });

describe("OverviewTab", () => {
  it("shows each failed scan on its own and keeps the disks", async () => {
    renderTab();
    expect(await screen.findByRole("progressbar")).toBeTruthy();
    fireEvent.click(await scanButton());
    for (const label of ["Caches", "System", "Docker", "Project junk"]) {
      await screen.findByText(`${label} scan failed: not implemented`);
    }
    expect(screen.getByRole("progressbar")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Clean suggested" }) as HTMLButtonElement).disabled).toBe(true);
    expect(localStorage.getItem(LAST_SCAN_KEY)).toBeNull();
  });

  it("totals what worked, reports what did not, and saves the scan", async () => {
    scans = {
      cleaner_scan_caches: () => [f({ id: "c1", bytes: 2 * MB }), f({ id: "c2", risk: "review", suggested: false, bytes: 3 * MB })],
      cleaner_scan_docker: () => ({ available: false, message: "Docker is not running", findings: [] }),
      cleaner_scan_projects: () => [
        { project_path: "/p", project_name: "app", artifact_path: "/p/node_modules", artifact_kind: "node_modules", bytes: 4 * MB, last_activity: null, idle_days: 90, suggested: true },
      ],
    };
    renderTab();
    fireEvent.click(await scanButton());
    await screen.findByText("System scan failed: not implemented");
    await screen.findByText("Docker: Docker is not running");
    await screen.findByText("Safe: 2.0 MB");
    expect(screen.getByText("Review: 7.0 MB")).toBeTruthy();
    expect(screen.getByText("Danger: 0 B")).toBeTruthy();
    expect(screen.getByText("Caches: 5.0 MB")).toBeTruthy();
    expect(screen.getByText("Project junk: 4.0 MB")).toBeTruthy();
    await waitFor(() => expect(localStorage.getItem(LAST_SCAN_KEY)).not.toBeNull());
    const saved = JSON.parse(localStorage.getItem(LAST_SCAN_KEY) as string);
    expect(saved.totals).toEqual({ safe: 2 * MB, review: 7 * MB, danger: 0 });
  });

  it("sends each suggested finding to the clean command for its tab", async () => {
    const out = (ids: string[]) => ids.map((id) => ({ id, ok: true, message: "", bytes_freed: MB }));
    scans = {
      cleaner_scan_caches: () => [f({ id: "c1" }), f({ id: "c2", suggested: false })],
      cleaner_scan_system: () => [f({ id: "s1", tab: "system" })],
      cleaner_scan_docker: () => ({
        available: true,
        message: null,
        findings: [f({ id: "d1", tab: "docker", risk: "danger", needs_confirm_text: "DELETE" })],
      }),
      cleaner_scan_projects: () => [
        { project_path: "/p", project_name: "app", artifact_path: "/p/target", artifact_kind: "target", bytes: MB, last_activity: null, idle_days: 90, suggested: true },
      ],
      cleaner_clean_caches: () => out(["c1"]),
      cleaner_clean_system: () => out(["s1"]),
      cleaner_clean_docker: () => out(["d1"]),
      cleaner_clean_projects: () => out(["/p/target"]),
    };
    renderTab();
    fireEvent.click(await scanButton());
    await waitFor(() => expect((screen.getByRole("button", { name: "Clean suggested" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "Clean suggested" }));

    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByRole("textbox"), { target: { value: "DELETE" } });
    fireEvent.click(within(dialog).getByRole("button", { name: /^Clean/ }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cleaner_clean_projects", { artifactPaths: ["/p/target"] }));
    expect(invoke).toHaveBeenCalledWith("cleaner_clean_caches", { ids: ["c1"] });
    expect(invoke).toHaveBeenCalledWith("cleaner_clean_system", { ids: ["s1"] });
    expect(invoke).toHaveBeenCalledWith("cleaner_clean_docker", { ids: ["d1"], confirmed: ["d1"] });
  });

  it("lists history and clears it after confirming", async () => {
    history = [{ id: 1, at: new Date().toISOString(), tab: "caches", label: "npm cache", bytes_freed: 5 * MB, ok: true, message: "" }];
    renderTab();
    await screen.findByText("npm cache");
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    fireEvent.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Clear" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cleaner_history_clear"));
    await screen.findByText("Nothing cleaned yet");
  });
});
