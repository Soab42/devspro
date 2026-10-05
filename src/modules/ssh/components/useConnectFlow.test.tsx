import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, renderHook, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import type { KeyPermissions, SshHost } from "../api";
import { useConnectFlow } from "./useConnectFlow";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));

const KEY = "/home/me/.ssh/a.pem";

const host = (id: string, auth: SshHost["auth"], key_path: string | null): SshHost => ({
  id,
  name: id,
  host: "web.example",
  port: 22,
  username: "me",
  auth,
  key_path,
  created_at: "",
});

const HOSTS = [host("keyed", "key", KEY), host("agent", "agent", null)];

let perms: KeyPermissions | Error;
let restrictFails: string | null;

beforeEach(() => {
  cleanup();
  perms = { mode: 0o664, too_open: true };
  restrictFails = null;
  invoke.mockReset();
  invoke.mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case "ssh_hosts_list":
        return HOSTS;
      case "ssh_key_permissions":
        if (perms instanceof Error) throw perms.message;
        return perms;
      case "ssh_key_restrict":
        if (restrictFails) throw restrictFails;
        return undefined;
      default:
        return undefined;
    }
  });
});

const calls = (command: string) => invoke.mock.calls.filter(([cmd]) => cmd === command).map(([, args]) => args);

function setup() {
  const wrapper = ({ children }: { children: ReactNode }) => <ConfirmProvider>{children}</ConfirmProvider>;
  return renderHook(() => useConnectFlow(), { wrapper }).result;
}

describe("prepareOpenSsh", () => {
  it("asks before restricting a key ssh would refuse, then lets ssh start", async () => {
    const flow = setup();
    let ready: Promise<boolean> = Promise.resolve(false);
    act(() => {
      ready = flow.current.prepareOpenSsh("keyed");
    });

    const dialog = await screen.findByRole("dialog", { name: "Private key is too open" });
    expect(dialog.textContent).toContain(`${KEY} can be read by other users (mode 664), so ssh will refuse it.`);
    expect(calls("ssh_key_restrict")).toEqual([]);

    fireEvent.click(screen.getByRole("button", { name: "Restrict and connect" }));
    await expect(ready).resolves.toBe(true);
    expect(calls("ssh_key_restrict")).toEqual([{ path: KEY }]);
  });

  it("changes nothing and doesn't connect when the user cancels", async () => {
    const flow = setup();
    let ready: Promise<boolean> = Promise.resolve(true);
    act(() => {
      ready = flow.current.prepareOpenSsh("keyed");
    });

    await screen.findByRole("dialog", { name: "Private key is too open" });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(ready).resolves.toBe(false);
    expect(calls("ssh_key_restrict")).toEqual([]);
  });

  it("starts straight away when the key is already private", async () => {
    perms = { mode: 0o600, too_open: false };
    const flow = setup();
    await expect(flow.current.prepareOpenSsh("keyed")).resolves.toBe(true);
    expect(calls("ssh_key_permissions")).toEqual([{ path: KEY }]);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(calls("ssh_key_restrict")).toEqual([]);
  });

  it("leaves hosts without a key file, and keys it can't read, to ssh", async () => {
    const flow = setup();
    await expect(flow.current.prepareOpenSsh("agent")).resolves.toBe(true);
    expect(calls("ssh_key_permissions")).toEqual([]);

    perms = new Error("cannot read /home/me/.ssh/a.pem: No such file or directory");
    await expect(flow.current.prepareOpenSsh("keyed")).resolves.toBe(true);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("doesn't connect when the chmod is refused, and passes on why", async () => {
    restrictFails = `${KEY} belongs to another user, so it was left as it is.`;
    const flow = setup();
    let ready: Promise<boolean> = Promise.resolve(true);
    act(() => {
      ready = flow.current.prepareOpenSsh("keyed");
    });

    await screen.findByRole("dialog", { name: "Private key is too open" });
    fireEvent.click(screen.getByRole("button", { name: "Restrict and connect" }));
    await expect(ready).rejects.toBe(restrictFails);
  });
});
