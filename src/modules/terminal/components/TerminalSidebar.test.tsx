import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import type { SshHost } from "../../ssh/api";
import type { TerminalSession } from "../workspace";
import TerminalSidebar, { type RemoteBrowser } from "./TerminalSidebar";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));

const host: SshHost = {
  id: "web",
  name: "web",
  host: "web.example",
  port: 22,
  username: "dev",
  auth: "password",
  key_path: null,
  created_at: "",
  has_saved_password: true,
};
const local: TerminalSession = { id: "local-1", title: "Local", command: "local", exited: false, cwd: "/home/me" };
const ssh: TerminalSession = { id: "ssh-1", title: "web", command: { ssh: "web" }, exited: false, cwd: null };
const remote: RemoteBrowser = { sessionId: "sftp-1", connecting: false, failure: null, connect: () => {}, disconnect: () => {} };

const folder = (dir: string, name: string) => ({
  name,
  path: `${dir}/${name}`,
  is_dir: true,
  size: 0,
  modified: 0,
  permissions: "drwxr-xr-x",
  is_hidden: false,
});

let projectsGone = false;

beforeEach(() => {
  cleanup();
  projectsGone = false;
  invoke.mockReset();
  invoke.mockImplementation(async (cmd: string, args?: { path?: string }) => {
    switch (cmd) {
      case "fm_places":
        return { home: "/home/me" };
      case "fm_list":
        return [folder(args?.path ?? "/", "notes")];
      case "sftp_home":
        return "/home/dev";
      case "sftp_list":
        if (args?.path === "/home/dev") return [folder("/home/dev", "projects")];
        if (args?.path === "/home/dev/projects" && !projectsGone) return [folder("/home/dev/projects", "src")];
        // The local folder isn't on the host, and the server takes a moment to say so.
        await new Promise((resolve) => setTimeout(resolve, 50));
        throw `could not list ${args?.path}: no such file`;
      default:
        return null;
    }
  });
});

function sidebar(session: TerminalSession) {
  const isLocal = session.command === "local";
  return (
    <ConfirmProvider>
      <TerminalSidebar
        session={session}
        host={isLocal ? null : host}
        remote={isLocal ? null : remote}
        onCwd={() => {}}
        onSend={() => {}}
        onOpenFile={() => {}}
        onClose={() => {}}
      />
    </ConfirmProvider>
  );
}

describe("moving focus from a local shell to a remote one", () => {
  it("shows the host's folder, not an error about the local one", async () => {
    const { rerender } = render(sidebar(local));
    expect(await screen.findByText("notes")).toBeTruthy();

    rerender(sidebar(ssh));
    expect(await screen.findByText("projects")).toBeTruthy();
    // Let the listing of the local folder on the host fail, as it does.
    await act(() => new Promise((resolve) => setTimeout(resolve, 120)));

    expect(screen.queryByText(/could not list/)).toBeNull();
    expect(screen.getByText("projects")).toBeTruthy();
  });

  it("never asks the host to list the local folder", async () => {
    const { rerender } = render(sidebar(local));
    expect(await screen.findByText("notes")).toBeTruthy();
    rerender(sidebar(ssh));
    expect(await screen.findByText("projects")).toBeTruthy();

    const remoteLists = invoke.mock.calls.filter(([cmd]) => cmd === "sftp_list").map(([, args]) => args.path);
    expect(remoteLists).toEqual(["/home/dev"]);
  });
});

describe("each machine keeps its own folder (#3)", () => {
  it("comes back to where the host was browsed, and the local shell to its own", async () => {
    const { rerender } = render(sidebar(ssh));
    fireEvent.click(await screen.findByText("projects"));
    expect(await screen.findByText("src")).toBeTruthy();

    rerender(sidebar(local));
    expect(await screen.findByText("notes")).toBeTruthy();
    expect(screen.getByTitle(/^\/home\/me /)).toBeTruthy();

    rerender(sidebar(ssh));
    expect(await screen.findByText("src")).toBeTruthy();
    expect(screen.getByTitle(/^\/home\/dev\/projects /)).toBeTruthy();
    expect(screen.queryByText(/could not list/)).toBeNull();
  });

  it("falls back to home when the remembered folder is gone", async () => {
    const { rerender } = render(sidebar(ssh));
    fireEvent.click(await screen.findByText("projects"));
    expect(await screen.findByText("src")).toBeTruthy();
    rerender(sidebar(local));
    expect(await screen.findByText("notes")).toBeTruthy();

    projectsGone = true;
    rerender(sidebar(ssh));
    expect(await screen.findByText("projects", {}, { timeout: 2000 })).toBeTruthy();
    expect(screen.getByTitle(/^\/home\/dev /)).toBeTruthy();
    expect(screen.queryByText(/could not list/)).toBeNull();
  });
});
