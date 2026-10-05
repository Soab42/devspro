import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import type { SshHost } from "../../ssh/api";
import TerminalsTab, { type WorkspaceHandle } from "./TerminalsTab";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));

// xterm needs a real canvas; a pane only has to say which command it would run.
vi.mock("../../../ui/Terminal", () => ({
  default: ({ command }: { command: unknown }) => <div data-testid="pty">{JSON.stringify(command)}</div>,
  TerminalWorkspace: ({ children, empty }: { children: ReactNode; empty: ReactNode }) => (
    <div>
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
});
