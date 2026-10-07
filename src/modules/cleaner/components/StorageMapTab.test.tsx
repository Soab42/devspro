import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import type { StorageTree, TreeNode } from "../api";
import StorageMapTab from "./StorageMapTab";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const file = (path: string, bytes: number): TreeNode => ({
  name: path.split("/").pop()!,
  path,
  bytes,
  files: 1,
  is_dir: false,
  children: [],
});

const TREE: StorageTree = {
  root: {
    name: "r",
    path: "/r",
    bytes: 1000,
    files: 3,
    is_dir: true,
    children: [
      {
        name: "big",
        path: "/r/big",
        bytes: 700,
        files: 2,
        is_dir: true,
        children: [file("/r/big/a.mp4", 500), file("/r/big/b.txt", 200)],
      },
      file("/r/small.log", 300),
    ],
  },
  extensions: [
    { ext: "mp4", kind: "video", bytes: 500, files: 1 },
    { ext: "log", kind: "log", bytes: 300, files: 1 },
    { ext: "txt", kind: "document", bytes: 200, files: 1 },
  ],
  total_bytes: 1000,
  files: 3,
  truncated: false,
};

function mockBackend(tree: StorageTree = TREE) {
  invoke.mockImplementation((cmd: string) => {
    switch (cmd) {
      case "cleaner_settings_get":
        return Promise.resolve({ scan_roots: ["/r"], project_roots: [], exclusions: [], large_file_min_bytes: 0, project_idle_days: 0 });
      case "cleaner_scan_tree":
        return Promise.resolve(tree);
      case "cleaner_remove_files":
        return Promise.resolve([{ id: "/r/big/a.mp4", ok: true, message: "Moved to Trash", bytes_freed: 500 }]);
      default:
        return Promise.resolve(undefined);
    }
  });
}

async function scanned(tree?: StorageTree) {
  mockBackend(tree);
  render(
    <MemoryRouter>
      <StorageMapTab />
    </MemoryRouter>,
  );
  const scan = screen.getByRole("button", { name: "Scan" });
  await waitFor(() => expect((scan as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(scan);
  await screen.findByRole("group", { name: "Treemap" });
}

beforeEach(() => invoke.mockReset());
afterEach(cleanup);

describe("StorageMapTab", () => {
  it("scans the chosen root and lists the file types", async () => {
    await scanned();
    expect(invoke).toHaveBeenCalledWith("cleaner_scan_tree", expect.objectContaining({ root: "/r" }));
    expect(screen.getByRole("table", { name: "Size by file extension" })).toBeTruthy();
    expect(screen.getByText("mp4")).toBeTruthy();
  });

  it("zooms into a folder and back up through the breadcrumb", async () => {
    await scanned();
    const crumbs = () => screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(within(crumbs()).queryByRole("button")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Treemap big" }));
    expect(within(crumbs()).getByText("big").tagName).toBe("STRONG");
    expect(screen.queryByRole("button", { name: "Treemap small.log" })).toBeNull();
    expect(screen.getByRole("button", { name: "Treemap a.mp4" })).toBeTruthy();

    fireEvent.click(within(crumbs()).getByRole("button", { name: "r" }));
    expect(screen.getByRole("button", { name: "Treemap small.log" })).toBeTruthy();
  });

  it("removes the selected file through the review dialog, to Trash by default", async () => {
    await scanned();
    const remove = screen.getByRole("button", { name: "Remove" }) as HTMLButtonElement;
    expect(remove.disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Treemap a.mp4" }));
    expect(remove.disabled).toBe(false);
    fireEvent.click(remove);
    expect(await screen.findByText("Review before cleaning")).toBeTruthy();
    expect(screen.getAllByText("/r/big/a.mp4").length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole("button", { name: /^Clean/ }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cleaner_remove_files", { paths: ["/r/big/a.mp4"], permanent: false }),
    );
    // The removed file leaves the map.
    await waitFor(() => expect(screen.queryByRole("button", { name: "Treemap a.mp4" })).toBeNull());
  });

  it("shows a nested cell's row in the tree and highlights the cell from a row", async () => {
    await scanned();
    expect(screen.queryByRole("button", { name: "a.mp4" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Treemap a.mp4" }));
    const row = screen.getByRole("button", { name: "a.mp4" });
    expect(row.getAttribute("aria-pressed")).toBe("true");

    fireEvent.click(screen.getByRole("button", { name: "small.log" }));
    expect(screen.getByRole("button", { name: "Treemap small.log" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("button", { name: "Treemap a.mp4" }).getAttribute("aria-pressed")).toBe("false");
  });

  it("expands and collapses folders in the tree, and gives each cell a tooltip", async () => {
    await scanned();
    fireEvent.click(screen.getByRole("button", { name: "Expand big" }));
    expect(screen.getByRole("button", { name: "b.txt" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Collapse big" }));
    expect(screen.queryByRole("button", { name: "b.txt" })).toBeNull();
    const tip = screen.getByRole("button", { name: "Treemap small.log" }).querySelector("title");
    expect(tip?.textContent).toContain("/r/small.log");
    expect(tip?.textContent).toContain("30%");
  });

  it("says so when the scan was cut short", async () => {
    await scanned({ ...TREE, truncated: true });
    expect(screen.getByText(/lower bound/)).toBeTruthy();
  });
});
