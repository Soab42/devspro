import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, renderHook, screen, within } from "@testing-library/react";
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
  has_saved_password: false,
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

describe("run with a password host", () => {
  /** Renders the hook's prompt, which renderHook alone would not. */
  function setupWithPrompt() {
    const flow = { current: null as unknown as ReturnType<typeof useConnectFlow> };
    function Harness() {
      flow.current = useConnectFlow();
      return <>{flow.current.element}</>;
    }
    render(
      <ConfirmProvider>
        <Harness />
      </ConfirmProvider>,
    );
    return flow;
  }

  async function answer(password: string, remember: boolean) {
    const dialog = await screen.findByRole("dialog", { name: "Password required" });
    const box = within(dialog).getByRole("checkbox", { name: "Remember this password" }) as HTMLInputElement;
    expect(box.checked).toBe(false);
    if (remember) fireEvent.click(box);
    fireEvent.change(within(dialog).getByLabelText("Password"), { target: { value: password } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Connect" }));
  }

  it("uses a typed password once unless the user ticks Remember", async () => {
    const flow = setupWithPrompt();
    const attempt = vi.fn(async () => "session");
    let result: Promise<string | null> = Promise.resolve(null);
    act(() => {
      result = flow.current.run(host("pw", "password", null), attempt);
    });
    await answer("hunter2", false);
    await expect(result).resolves.toBe("session");
    expect(attempt).toHaveBeenCalledWith({ password: "hunter2", remember: false });

    act(() => {
      result = flow.current.run(host("pw", "password", null), attempt);
    });
    await answer("hunter2", true);
    await expect(result).resolves.toBe("session");
    expect(attempt).toHaveBeenLastCalledWith({ password: "hunter2", remember: true });
  });

  it("doesn't ask when a password is saved, and asks again when the server rejects it", async () => {
    const flow = setupWithPrompt();
    const saved = { ...host("pw", "password", null), has_saved_password: true };
    const attempt = vi
      .fn<(creds: object) => Promise<string>>()
      .mockRejectedValueOnce("SAVED_PASSWORD_REJECTED")
      .mockResolvedValueOnce("session");
    let result: Promise<string | null> = Promise.resolve(null);
    act(() => {
      result = flow.current.run(saved, attempt);
    });
    const dialog = await screen.findByRole("dialog", { name: "Password required" });
    expect(attempt).toHaveBeenNthCalledWith(1, {});
    expect(dialog.textContent).toContain("The saved password was rejected, so it has been forgotten.");
    await answer("new-one", true);
    await expect(result).resolves.toBe("session");
    expect(attempt).toHaveBeenNthCalledWith(2, { password: "new-one", remember: true });
  });
});
