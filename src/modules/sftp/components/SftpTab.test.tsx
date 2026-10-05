import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import type { SftpEntry, SshHost } from "../../ssh/api";
import { FAVOURITES_KEY } from "../favourites";
import SftpTab from "./SftpTab";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
const save = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: (...args: unknown[]) => save(...args) }));

const host: SshHost = {
  id: "h1",
  name: "web",
  host: "web.example",
  port: 22,
  username: "me",
  auth: "agent",
  key_path: null,
  created_at: "",
};

const file = (dir: string, name: string, is_dir = false): SftpEntry => ({
  name,
  path: `${dir}/${name}`,
  is_dir,
  size: 1,
  modified: 0,
  permissions: "644",
});

const REMOTE: Record<string, SftpEntry[]> = {
  "/srv": [file("/srv", "app", true), file("/srv", "a.txt"), file("/srv", "b.txt"), file("/srv", "c.txt")],
  "/srv/app": [file("/srv/app", "inner.txt")],
};

/** The settings table, and every command the tab sends, by name. */
let stored: Record<string, string>;
function route(cmd: string, args: Record<string, string>) {
  switch (cmd) {
    case "settings_get":
      return stored[args.key] ?? null;
    case "settings_set":
      stored[args.key] = args.value;
      return undefined;
    case "sftp_home":
      return "/srv";
    case "sftp_list":
      return REMOTE[args.path] ?? [];
    case "fm_places":
      return { home: "/home/me", places: [] };
    case "fm_list":
      return [];
    case "sftp_delete":
      if (args.path === "/srv/c.txt") throw "permission denied";
      return undefined;
    case "sftp_download":
      if (args.remote === "/srv/b.txt") throw "disk full";
      return 1;
    case "sftp_download_zip":
      return { bytes: 2, skipped: ["/srv/c.txt: permission denied"] };
    default:
      return undefined;
  }
}

function renderTab() {
  return render(
    <ConfirmProvider>
      <SftpTab sessions={[{ id: "s1", host }]} activeId="s1" hosts={[host]} layout="side" />
    </ConfirmProvider>,
  );
}

const remoteTable = () => screen.getByRole("table", { name: "Files in /srv on web" });
const rowOf = (name: string) => within(remoteTable()).getByText(name).closest("tr") as HTMLElement;

const calls = (command: string) =>
  invoke.mock.calls.filter(([cmd]) => cmd === command).map(([, args]) => args);

beforeEach(() => {
  cleanup();
  stored = {};
  save.mockReset();
  invoke.mockReset();
  invoke.mockImplementation(async (cmd: string, args: Record<string, string>) => route(cmd, args));
});

describe("favourite folders", () => {
  it("stars the folder for this host, and opens it again from the menu", async () => {
    renderTab();
    await screen.findByText("a.txt");

    fireEvent.click(screen.getByRole("button", { name: "Add this folder to favourites" }));
    await waitFor(() => expect(JSON.parse(stored[FAVOURITES_KEY])).toEqual({ h1: ["/srv"] }));
    expect(screen.getByRole("button", { name: "Remove this folder from favourites" })).toBeDefined();

    fireEvent.click(within(remoteTable()).getByRole("button", { name: "app" }));
    await screen.findByText("inner.txt");
    expect(screen.getByRole("button", { name: "Add this folder to favourites" })).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: "Favourite folders" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "/srv" }));
    await screen.findByText("a.txt");
  });

  it("lists only this host's favourites, and removes one from the dialog", async () => {
    stored[FAVOURITES_KEY] = JSON.stringify({ h1: ["/srv/app", "/var/log"], h2: ["/etc"] });
    renderTab();
    await screen.findByText("a.txt");

    fireEvent.click(screen.getByRole("button", { name: "Favourite folders" }));
    expect(screen.getByRole("menuitem", { name: "/srv/app" })).toBeDefined();
    expect(screen.queryByRole("menuitem", { name: "/etc" })).toBeNull();

    fireEvent.click(screen.getByRole("menuitem", { name: "Edit favourites…" }));
    const dialog = screen.getByRole("dialog", { name: "Favourite folders on web" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove /var/log from favourites" }));
    await waitFor(() =>
      expect(JSON.parse(stored[FAVOURITES_KEY])).toEqual({ h1: ["/srv/app"], h2: ["/etc"] }),
    );
    expect(within(dialog).queryByText("/var/log")).toBeNull();
  });
});

describe("multi-select", () => {
  it("selects with click, shift and ctrl, and with the select-all box", async () => {
    renderTab();
    await screen.findByText("a.txt");

    fireEvent.click(rowOf("a.txt"));
    fireEvent.click(rowOf("c.txt"), { shiftKey: true });
    expect(screen.getByText("3 items selected")).toBeDefined();
    fireEvent.click(rowOf("b.txt"), { ctrlKey: true });
    expect(screen.getByText("2 items selected")).toBeDefined();

    fireEvent.click(within(remoteTable()).getByRole("checkbox", { name: "Select all" }));
    expect(screen.getByText("4 items selected")).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Clear selection" }));
    expect(screen.queryByText(/selected$/)).toBeNull();
  });

  it("deletes the selection after one confirm, and reports each one that failed", async () => {
    renderTab();
    await screen.findByText("a.txt");

    fireEvent.click(within(rowOf("a.txt")).getByRole("checkbox", { name: "Select a.txt" }));
    fireEvent.click(within(rowOf("c.txt")).getByRole("checkbox", { name: "Select c.txt" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    const confirm = await screen.findByRole("dialog", { name: "Delete 2 items?" });
    fireEvent.click(within(confirm).getByRole("button", { name: "Delete" }));

    await screen.findByText("Couldn't delete 1 of 2 items: c.txt (permission denied)");
    expect(screen.getByText("Deleted 1 item on web.")).toBeDefined();
    expect(calls("sftp_delete").map((a) => a.path)).toEqual(["/srv/a.txt", "/srv/c.txt"]);
  });

  it("moves to local by deleting only what downloaded, and keeps folders", async () => {
    renderTab();
    await screen.findByText("a.txt");

    fireEvent.click(rowOf("app"));
    fireEvent.click(rowOf("b.txt"), { shiftKey: true });
    fireEvent.click(screen.getByRole("button", { name: "Move to local" }));
    const confirm = await screen.findByRole("dialog", { name: "Move 3 items to /home/me?" });
    fireEvent.click(within(confirm).getByRole("button", { name: "Move" }));

    await waitFor(() => expect(calls("sftp_download")).toHaveLength(3));
    await screen.findByText("disk full");
    expect(calls("sftp_download").map((a) => [a.remote, a.local])).toEqual([
      ["/srv/app", "/home/me/app"],
      ["/srv/a.txt", "/home/me/a.txt"],
      ["/srv/b.txt", "/home/me/b.txt"],
    ]);
    // The folder stays (told up front), b.txt never arrived, so only a.txt goes.
    expect(calls("sftp_delete").map((a) => a.path)).toEqual(["/srv/a.txt"]);
  });

  it("zips the selection where the user saves it, and says what was left out", async () => {
    save.mockResolvedValue("/home/me/picked.zip");
    renderTab();
    await screen.findByText("a.txt");

    fireEvent.click(within(remoteTable()).getByRole("checkbox", { name: "Select all" }));
    fireEvent.click(screen.getByRole("button", { name: "Download as zip" }));

    await screen.findByText("Saved without 1 item: /srv/c.txt: permission denied");
    expect(save.mock.calls[0][0]).toMatchObject({ defaultPath: "/home/me/srv.zip" });
    expect(calls("sftp_download_zip")).toEqual([
      {
        session: "s1",
        remotes: ["/srv/app", "/srv/a.txt", "/srv/b.txt", "/srv/c.txt"],
        local: "/home/me/picked.zip",
        transfer: expect.any(String),
      },
    ]);
    expect(screen.queryByText(/selected$/)).toBeNull();
  });
});

describe("after a transfer", () => {
  it("refreshes the folder on screen, not the one the transfer started in", async () => {
    let finish = () => {};
    invoke.mockImplementation(async (cmd: string, args: Record<string, string>) =>
      cmd === "sftp_download"
        ? new Promise((resolve) => (finish = () => resolve(1)))
        : route(cmd, args),
    );
    renderTab();
    await screen.findByText("a.txt");

    fireEvent.click(rowOf("a.txt"));
    fireEvent.click(screen.getByRole("button", { name: "Move to local" }));
    await waitFor(() => expect(calls("sftp_download")).toHaveLength(1));

    // Off into another folder while the download is still going.
    fireEvent.click(within(remoteTable()).getByRole("button", { name: "app" }));
    await screen.findByText("inner.txt");
    const listed = calls("sftp_list").length;
    await act(async () => finish());
    await waitFor(() => expect(calls("sftp_list").length).toBeGreaterThan(listed));

    expect(calls("sftp_delete").map((a) => a.path)).toEqual(["/srv/a.txt"]);
    const lists = calls("sftp_list");
    expect(lists[lists.length - 1].path).toBe("/srv/app");
    const table = screen.getByRole("table", { name: "Files in /srv/app on web" });
    await waitFor(() => expect(within(table).getByText("inner.txt")).toBeDefined());
    expect(within(table).queryByText("b.txt")).toBeNull();
  });

  it("ignores a listing that arrives after the user has moved on", async () => {
    let finishSlow = () => {};
    invoke.mockImplementation(async (cmd: string, args: Record<string, string>) =>
      cmd === "sftp_list" && args.path === "/srv/app"
        ? new Promise((resolve) => (finishSlow = () => resolve(REMOTE["/srv/app"])))
        : route(cmd, args),
    );
    renderTab();
    await screen.findByText("a.txt");

    // Into the slow folder, and back out before it answers.
    fireEvent.click(within(remoteTable()).getByRole("button", { name: "app" }));
    await waitFor(() => expect(calls("sftp_list").some((a) => a.path === "/srv/app")).toBe(true));
    // Favourites are on the remote pane only, so they find its toolbar.
    const pane = screen.getByRole("button", { name: "Favourite folders" }).closest("section") as HTMLElement;
    fireEvent.click(within(pane).getByRole("button", { name: "Up one level" }));
    await screen.findByRole("table", { name: "Files in /srv on web" });
    await act(async () => finishSlow());

    expect(within(remoteTable()).getByText("a.txt")).toBeDefined();
    expect(within(remoteTable()).queryByText("inner.txt")).toBeNull();
  });
});
