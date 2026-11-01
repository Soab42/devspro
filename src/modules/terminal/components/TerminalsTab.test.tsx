import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import type { SshHost } from "../../ssh/api";
import TerminalsTab, { type WorkspaceHandle } from "./TerminalsTab";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));

// xterm needs a real canvas; a pane only has to say which command it would
// run, and hand over what it would do with a pasted image.
type PasteImage = (local: string) => Promise<string | null>;
const pasteImage = new Map<string, PasteImage>();
vi.mock("../../../ui/Terminal", () => ({
  default: ({ command, onPasteImage }: { command: unknown; onPasteImage?: PasteImage }) => {
    if (onPasteImage) pasteImage.set(JSON.stringify(command), onPasteImage);
    return <div data-testid="pty">{JSON.stringify(command)}</div>;
  },
  TerminalWorkspace: ({ children, empty, tabs, actions }: { children: ReactNode; empty: ReactNode; tabs?: ReactNode; actions?: ReactNode }) => (
    <div>
      {tabs}
      {actions}
      {empty}
      {children}
    </div>
  ),
  clampAside: (width: number) => width,
}));
vi.mock("./HostStatsBar", () => ({ default: () => null }));
vi.mock("./TerminalSidebar", () => ({ default: () => null }));
// PaneLayout measures itself; jsdom has no layout to observe.
vi.stubGlobal(
  "ResizeObserver",
  class {
    observe() {}
    unobserve() {}
    disconnect() {}
  },
);

const KEY = "/home/me/.ssh/a.pem";
const host: SshHost = {
  id: "keyed",
  name: "web",
  host: "web.example",
  port: 22,
  username: "me",
  auth: "key",
  key_path: KEY,
  created_at: "",
};

beforeEach(() => {
  cleanup();
  invoke.mockReset();
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === "ssh_hosts_list") return [host];
    if (cmd === "ssh_key_permissions") return { mode: 0o664, too_open: true };
    return undefined;
  });
});

const calls = (command: string) => invoke.mock.calls.filter(([cmd]) => cmd === command).map(([, args]) => args);
const panes = () => screen.queryAllByTestId("pty").map((el) => el.textContent);

function renderTab() {
  let handle: WorkspaceHandle | null = null;
  render(
    <ConfirmProvider>
      <TerminalsTab
        hosts={[host]}
        hidden={false}
        onReady={(h) => {
          handle = h;
        }}
        onError={() => {}}
      />
    </ConfirmProvider>,
  );
  return () => handle as unknown as WorkspaceHandle;
}

describe("opening an ssh terminal", () => {
  it("restricts a too-open key before ssh starts, once confirmed", async () => {
    const workspace = renderTab();
    act(() => workspace().openWindow({ ssh: "keyed" }));

    await screen.findByRole("dialog", { name: "Private key is too open" });
    expect(panes()).toEqual([]);

    fireEvent.click(screen.getByRole("button", { name: "Restrict and connect" }));
    expect(await screen.findByTestId("pty")).toBeDefined();
    expect(panes()).toEqual(['{"ssh":"keyed"}']);
    expect(calls("ssh_key_restrict")).toEqual([{ path: KEY }]);
  });

  it("opens nothing when the user cancels", async () => {
    const workspace = renderTab();
    act(() => workspace().openWindow({ ssh: "keyed" }));

    await screen.findByRole("dialog", { name: "Private key is too open" });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await act(async () => {});
    expect(panes()).toEqual([]);
    expect(calls("ssh_key_restrict")).toEqual([]);
  });

  it("checks a split onto the host too, and leaves local shells alone", async () => {
    const workspace = renderTab();
    act(() => workspace().openWindow("local"));
    expect(panes()).toEqual(['"local"']);
    expect(calls("ssh_key_permissions")).toEqual([]);

    act(() => workspace().split({ ssh: "keyed" }, "row"));
    await screen.findByRole("dialog", { name: "Private key is too open" });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await act(async () => {});
    expect(panes()).toEqual(['"local"']);
  });

  it("logs in to a host from the empty page, file browser and all", async () => {
    localStorage.setItem("devspro.terminal.aside", "0");
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "ssh_hosts_list") return [host];
      if (cmd === "ssh_key_permissions") return { mode: 0o600, too_open: false };
      return undefined;
    });
    renderTab();
    const hosts = screen.getByRole("list", { name: "Saved SSH hosts" });
    expect(hosts.textContent).toContain("me@web.example");
    expect(screen.getByRole("button", { name: "Files" }).getAttribute("aria-pressed")).toBe("false");

    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(await screen.findByTestId("pty")).toBeDefined();
    expect(panes()).toEqual(['{"ssh":"keyed"}']);
    expect(screen.getByRole("button", { name: "Files" }).getAttribute("aria-pressed")).toBe("true");
    localStorage.removeItem("devspro.terminal.aside");
  });
});

describe("closing terminals", () => {
  it("removes a pane, a tab and a window, and none of them come back", async () => {
    const workspace = renderTab();
    act(() => workspace().openWindow("local"));
    act(() => workspace().openWindow("local"));
    act(() => workspace().split("local", "row"));
    expect(panes()).toHaveLength(3);

    // Only the active window is exposed to the accessibility tree.
    fireEvent.click(document.querySelectorAll<HTMLElement>('[aria-label="Close pane"]')[1]);
    expect(panes()).toHaveLength(2);

    fireEvent(screen.getAllByRole("tab")[0], new MouseEvent("auxclick", { bubbles: true, button: 1 }));
    expect(screen.getAllByRole("tab")).toHaveLength(1);

    fireEvent.click(screen.getAllByRole("button", { name: "Close" })[0]);
    await act(async () => {});
    expect(panes()).toEqual([]);
    expect(screen.queryAllByRole("tab")).toHaveLength(0);
  });
});

describe("pasting an image", () => {
  it("types a local shell's image where it was saved", async () => {
    const workspace = renderTab();
    act(() => workspace().openWindow("local"));
    await expect(pasteImage.get('"local"')?.("/home/me/.cache/p.png")).resolves.toBe("/home/me/.cache/p.png");
    expect(calls("sftp_upload_temp")).toEqual([]);
  });

  it("uploads a remote shell's image to the host first, connecting if need be", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "ssh_hosts_list") return [host];
      if (cmd === "ssh_key_permissions") return { mode: 0o600, too_open: false };
      if (cmd === "sftp_connect") return "sftp-1";
      if (cmd === "sftp_upload_temp") return "/tmp/p.png";
      return undefined;
    });
    const workspace = renderTab();
    act(() => workspace().openWindow({ ssh: "keyed" }));
    await screen.findByTestId("pty");

    let typed: string | null = null;
    await act(async () => {
      typed = (await pasteImage.get('{"ssh":"keyed"}')?.("/home/me/.cache/p.png")) ?? null;
    });
    expect(typed).toBe("/tmp/p.png");
    expect(calls("sftp_connect")).toHaveLength(1);
    expect(calls("sftp_upload_temp")).toEqual([{ session: "sftp-1", local: "/home/me/.cache/p.png" }]);

    // The connection is kept for the file browser and the next paste.
    await act(async () => {
      await pasteImage.get('{"ssh":"keyed"}')?.("/home/me/.cache/q.png");
    });
    expect(calls("sftp_connect")).toHaveLength(1);
  });
});
