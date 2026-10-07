import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import type { CleanerSettings, FolderEntry, LargeFile } from "../api";
import LargeFilesTab from "./LargeFilesTab";
import { resetTabState } from "../tabState";
import { filterFiles, nearestSize } from "./large-files/model";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

const MB = 1024 * 1024;
const SETTINGS: CleanerSettings = {
  scan_roots: ["/home/u"],
  project_roots: [],
  exclusions: [],
  large_file_min_bytes: 500 * MB,
  project_idle_days: 30,
};
const NOW = Math.floor(Date.now() / 1000);
const FILES: LargeFile[] = [
  { path: "/home/u/movie.mkv", bytes: 900 * MB, modified: NOW - 400 * 86_400, accessed: NOW, kind: "video" },
  { path: "/home/u/dump.sql", bytes: 600 * MB, modified: NOW - 5 * 86_400, accessed: NOW, kind: "db-dump" },
];
const FOLDERS: FolderEntry[] = [
  { path: "/home/u/Videos", name: "Videos", bytes: 2000 * MB, is_dir: true, entries: 12 },
  { path: "/home/u/a.iso", name: "a.iso", bytes: 700 * MB, is_dir: false, entries: 0 },
];

function renderTab() {
  return render(
    <MemoryRouter>
      <ConfirmProvider>
        <LargeFilesTab />
      </ConfirmProvider>
    </MemoryRouter>,
  );
}

beforeEach(() => {
  resetTabState();
  invoke.mockReset();
  invoke.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
    switch (cmd) {
      case "cleaner_settings_get":
        return SETTINGS;
      case "cleaner_scan_large_files":
        return FILES;
      case "cleaner_folder_sizes":
        return args?.path === "/home/u" ? FOLDERS : [];
      case "cleaner_remove_files":
        return (args?.paths as string[]).map((id) => ({ id, ok: true, message: "ok", bytes_freed: 1 }));
      default:
        throw new Error(`unexpected ${cmd}`);
    }
  });
});
afterEach(cleanup);

async function scan() {
  renderTab();
  fireEvent.click(await screen.findByRole("button", { name: "Scan" }));
  await screen.findByText("/home/u/movie.mkv");
}

describe("filters", () => {
  it("picks the threshold nearest the setting", () => {
    expect(nearestSize(500 * MB)).toBe(String(500 * MB));
    expect(nearestSize(700 * MB)).toBe(String(500 * MB));
    expect(nearestSize(4096 * MB)).toBe(String(5 * 1024 * MB));
  });

  it("filters by size, kind and age", () => {
    expect(filterFiles(FILES, 700 * MB, "all", 0, NOW).map((f) => f.kind)).toEqual(["video"]);
    expect(filterFiles(FILES, 0, "db-dump", 0, NOW).map((f) => f.kind)).toEqual(["db-dump"]);
    expect(filterFiles(FILES, 0, "all", 90, NOW).map((f) => f.kind)).toEqual(["video"]);
  });
});

describe("LargeFilesTab", () => {
  it("scans with the settings threshold and lists the files", async () => {
    await scan();
    const call = invoke.mock.calls.find((c) => c[0] === "cleaner_scan_large_files");
    expect(call?.[1]).toMatchObject({ minBytes: 500 * MB, olderThanDays: null });
    expect(screen.getByText("/home/u/dump.sql")).toBeTruthy();
  });

  it("moves the selected files to the Trash through the review modal", async () => {
    await scan();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select /home/u/movie.mkv" }));
    fireEvent.click(screen.getByRole("button", { name: "Move to Trash" }));
    fireEvent.click(await screen.findByRole("button", { name: /^Clean/ }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cleaner_remove_files", { paths: ["/home/u/movie.mkv"], permanent: false }),
    );
  });

  it("says trashed files still take space until the Trash is emptied", async () => {
    await scan();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select /home/u/movie.mkv" }));
    fireEvent.click(screen.getByRole("button", { name: "Move to Trash" }));
    fireEvent.click(await screen.findByRole("button", { name: /^Clean/ }));
    expect(await screen.findByText(/space is freed when the Trash is emptied/)).toBeTruthy();
  });

  it("does not say that after a permanent delete", async () => {
    await scan();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select /home/u/dump.sql" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    fireEvent.click(await screen.findByRole("button", { name: "Continue" }));
    fireEvent.click(await screen.findByRole("button", { name: /^Clean/ }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cleaner_remove_files", expect.anything()));
    expect(screen.queryByText(/Trash is emptied/)).toBeNull();
  });

  it("asks again before deleting permanently", async () => {
    await scan();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select /home/u/dump.sql" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    expect(await screen.findByText(/cannot be restored/)).toBeTruthy();
    expect(invoke).not.toHaveBeenCalledWith("cleaner_remove_files", expect.anything());
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(await screen.findByRole("button", { name: /^Clean/ }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cleaner_remove_files", { paths: ["/home/u/dump.sql"], permanent: true }),
    );
  });

  it("does not delete when the confirmation is cancelled", async () => {
    await scan();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select /home/u/dump.sql" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    await screen.findByText(/cannot be restored/);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByText(/cannot be restored/)).toBeNull());
    expect(screen.queryByRole("button", { name: /^Clean/ })).toBeNull();
    expect(invoke).not.toHaveBeenCalledWith("cleaner_remove_files", expect.anything());
  });

  it("drills into a folder from the biggest folders view", async () => {
    renderTab();
    fireEvent.click(await screen.findByRole("radio", { name: "Biggest folders" }));
    fireEvent.click(await screen.findByRole("button", { name: "Videos" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cleaner_folder_sizes", expect.objectContaining({ path: "/home/u/Videos" })),
    );
    // The breadcrumb leads back to the root.
    fireEvent.click(await screen.findByRole("button", { name: "/home/u" }));
    expect(await screen.findByText("a.iso")).toBeTruthy();
  });
});
