import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import CleanerSettings from "./CleanerSettings";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
const open = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...args: unknown[]) => open(...args) }));

const MB = 1024 * 1024;
const SETTINGS = {
  scan_roots: ["/home/me"],
  project_roots: ["/home/me/code"],
  exclusions: [],
  large_file_min_bytes: 100 * MB,
  project_idle_days: 30,
};

beforeEach(() => {
  invoke.mockReset();
  open.mockReset();
  invoke.mockImplementation(async (cmd: string, args?: { settings?: unknown }) => {
    if (cmd === "cleaner_settings_get") return SETTINGS;
    if (cmd === "cleaner_settings_set") return args?.settings;
    throw new Error(`unexpected ${cmd}`);
  });
});
afterEach(cleanup);

describe("CleanerSettings", () => {
  it("saves added and removed folders and the edited numbers", async () => {
    open.mockResolvedValue("/mnt/data");
    render(<CleanerSettings />);
    await screen.findByText("/home/me");

    fireEvent.click(screen.getAllByRole("button", { name: "Add folder" })[2]);
    await screen.findByText("/mnt/data");
    fireEvent.click(screen.getByRole("button", { name: "Remove /home/me/code" }));
    fireEvent.change(screen.getByLabelText("Large file threshold (MB)"), { target: { value: "250" } });
    fireEvent.change(screen.getByLabelText("Project idle days"), { target: { value: "45" } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));

    await screen.findByText("Settings saved.");
    expect(invoke).toHaveBeenCalledWith("cleaner_settings_set", {
      settings: {
        scan_roots: ["/home/me"],
        project_roots: [],
        exclusions: ["/mnt/data"],
        large_file_min_bytes: 250 * MB,
        project_idle_days: 45,
      },
    });
  });

  it("does not add when the folder dialog is cancelled", async () => {
    open.mockResolvedValue(null);
    render(<CleanerSettings />);
    await screen.findByText("/home/me");
    fireEvent.click(screen.getAllByRole("button", { name: "Add folder" })[0]);
    await waitFor(() => expect(open).toHaveBeenCalled());
    expect(screen.getAllByText("/home/me").length).toBe(1);
  });

  it("shows a save failure and blocks a bad number", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "cleaner_settings_get") return SETTINGS;
      throw "disk is read-only";
    });
    render(<CleanerSettings />);
    await screen.findByText("/home/me");
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await screen.findByText("disk is read-only");

    fireEvent.change(screen.getByLabelText("Project idle days"), { target: { value: "abc" } });
    expect((screen.getByRole("button", { name: "Save settings" }) as HTMLButtonElement).disabled).toBe(true);
  });
});
