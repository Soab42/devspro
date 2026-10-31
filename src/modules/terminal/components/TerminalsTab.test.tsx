import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import type { SshHost } from "../../ssh/api";
import TerminalsTab, { type WorkspaceHandle } from "./TerminalsTab";
import { keptWorkspace } from "../workspaceStore";

const invoke = vi.fn();
const endTerminalSession = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));

// xterm needs a real canvas; a pane only has to say which command it would run.
vi.mock("../../../ui/Terminal", () => ({
  default: ({ command, onPrompt }: { command: unknown; onPrompt?: () => unknown }) => (
    <div data-testid="pty">
      {JSON.stringify(command)}
      <button onClick={() => onPrompt?.()}>shell prompt</button>
    </div>
  ),
  TerminalWorkspace: ({ children, empty, tabs, actions }: { children: ReactNode; empty: ReactNode; tabs?: ReactNode; actions?: ReactNode }) => (
    <div>
      {tabs}
      {actions}
      {/* Like the real one: the empty state replaces the children. */}
      {empty || children}
    </div>
  ),
  clampAside: (width: number) => width,
  hideNextHook: () => {},
  endTerminalSession: (id: string) => endTerminalSession(id),
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
  has_saved_password: false,
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
const panes = () => screen.queryAllByTestId("pty").map((el) => el.firstChild?.textContent);

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

describe("with no terminal open", () => {
  it("opens a local shell from its card", () => {
    renderTab();
    expect(screen.getByRole("button", { name: /SSH server/ }).textContent).toContain("1 saved host");
    fireEvent.click(screen.getByRole("button", { name: /Local terminal/ }));
    expect(panes()).toEqual(['"local"']);
  });

  it("picks the host for an SSH terminal from the host dialog", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "ssh_hosts_list") return [host];
      if (cmd === "ssh_key_permissions") return { mode: 0o600, too_open: false };
      return undefined;
    });
    renderTab();
    fireEvent.click(screen.getByRole("button", { name: /SSH server/ }));
    const dialog = screen.getByRole("dialog", { name: "Connect to a host" });
    fireEvent.click(within(dialog).getByRole("button", { name: /web/ }));
    expect(screen.queryByRole("dialog", { name: "Connect to a host" })).toBeNull();
    expect(await screen.findByTestId("pty")).toBeDefined();
    expect(panes()).toEqual(['{"ssh":"keyed"}']);
  });

  it("keeps the header buttons, the SSH host picker among them, in the page top bar", () => {
    const slot = document.createElement("div");
    document.body.appendChild(slot);
    render(
      <ConfirmProvider>
        <TerminalsTab hosts={[host]} hidden={false} actionsPortal={slot} onError={() => {}} />
      </ConfirmProvider>,
    );
    expect(within(slot).getByRole("button", { name: "Open a terminal on a saved SSH host" })).toBeDefined();
    slot.remove();
  });

  it("points to adding a host when none is saved", () => {
    const onAddHost = vi.fn();
    render(
      <ConfirmProvider>
        <TerminalsTab hosts={[]} hidden={false} onError={() => {}} onAddHost={onAddHost} />
      </ConfirmProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: /SSH server/ }));
    expect(onAddHost).toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("a kept workspace", () => {
  function renderKept(key: string) {
    let handle: WorkspaceHandle | null = null;
    const view = render(
      <ConfirmProvider>
        <TerminalsTab hosts={[host]} hidden={false} keep={key}
          onReady={(h) => {
            handle = h;
          }}
          onError={() => {}}
        />
      </ConfirmProvider>,
    );
    return { view, workspace: () => handle as unknown as WorkspaceHandle };
  }

  it("is still there, shells and all, when its page comes back", () => {
    endTerminalSession.mockReset();
    const first = renderKept("kept-a");
    act(() => first.workspace().openWindow("local"));
    expect(panes()).toEqual(['"local"']);

    first.view.unmount();
    expect(panes()).toEqual([]);

    renderKept("kept-a");
    expect(panes()).toEqual(['"local"']);
    expect(endTerminalSession).not.toHaveBeenCalled();
  });

  it("ends a shell when its window is closed", async () => {
    endTerminalSession.mockReset();
    const kept = renderKept("kept-b");
    act(() => kept.workspace().openWindow("local"));
    fireEvent.click(screen.getByRole("button", { name: "Close window" }));
    await waitFor(() => expect(endTerminalSession).toHaveBeenCalledTimes(1));
    expect(panes()).toEqual([]);
  });
});

describe("following a remote shell's folder", () => {
  async function openOnHost(key: string, remoteCwd: string | null) {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "ssh_hosts_list") return [host];
      if (cmd === "ssh_key_permissions") return { mode: 0o600, too_open: false };
      if (cmd === "term_remote_cwd") return remoteCwd;
      return undefined;
    });
    // The file browser's SSH connection to the host is already up.
    keptWorkspace(key, "terminal").sftp = { keyed: "sftp-1" };
    let handle: WorkspaceHandle | null = null;
    render(
      <ConfirmProvider>
        <TerminalsTab
          hosts={[host]}
          hidden={false}
          keep={key}
          onReady={(h) => {
            handle = h;
          }}
          onError={() => {}}
        />
      </ConfirmProvider>,
    );
    act(() => (handle as unknown as WorkspaceHandle).openWindow({ ssh: "keyed" }));
    fireEvent.click(await screen.findByText("shell prompt"));
  }

  it("reads it in the background, typing nothing into the terminal", async () => {
    await openOnHost("follow-a", "/srv/app");
    await waitFor(() => expect(calls("term_remote_cwd")).toHaveLength(1));
    expect(calls("term_remote_cwd")[0]).toMatchObject({ session: "sftp-1" });
    await act(async () => {});
    expect(calls("pty_write")).toEqual([]);
  });

  it("falls back to the hook when the server can't say", async () => {
    await openOnHost("follow-b", null);
    await waitFor(() => expect(calls("pty_write")).toHaveLength(1));
    expect(String((calls("pty_write")[0] as { data: string }).data)).toContain("PROMPT_COMMAND");
  });
});
