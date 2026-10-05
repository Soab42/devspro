import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { SshKey } from "../api";
import ImportKeyForm from "./ImportKeyForm";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

const key = (name: string): SshKey => ({
  name,
  type: "ED25519",
  bits: 256,
  fingerprint: "SHA256:abc",
  comment: "",
  public_key: "ssh-ed25519 AAAA",
  has_private: true,
  path: `/home/me/.ssh/${name}`,
});

const PRIVATE = "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----";

function renderForm(existing: SshKey[] = []) {
  const onImported = vi.fn();
  render(<ImportKeyForm existing={existing} onImported={onImported} onCancel={() => {}} />);
  return onImported;
}
const type = (label: string, value: string) =>
  fireEvent.change(screen.getByLabelText(label, { exact: false }), { target: { value } });
const submit = () =>
  act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Import key" }));
  });

beforeEach(() => {
  cleanup();
  invoke.mockReset();
});

describe("ImportKeyForm", () => {
  it("sends the pasted pair under its name, and hands back the saved key", async () => {
    invoke.mockResolvedValue(key("brokercore-live"));
    const onImported = renderForm();
    type("Name", " brokercore-live ");
    type("Private key", PRIVATE);
    type("Public key", "ssh-ed25519 AAAA me@box");
    await submit();

    expect(invoke).toHaveBeenCalledWith("ssh_keys_import", {
      args: {
        name: "brokercore-live",
        private_key: PRIVATE,
        public_key: "ssh-ed25519 AAAA me@box",
        passphrase: "",
      },
    });
    expect(onImported).toHaveBeenCalledWith(key("brokercore-live"));
  });

  it("shows why a key was refused, and doesn't send one without a private key or a free name", async () => {
    invoke.mockRejectedValue("the public key doesn't belong to this private key");
    renderForm([key("taken")]);
    type("Name", "taken");
    await submit();
    expect(screen.getByText('A key named "taken" already exists.')).toBeDefined();

    type("Name", "fresh");
    await submit();
    expect(screen.getByText("Paste the private key.")).toBeDefined();
    expect(invoke).not.toHaveBeenCalled();

    type("Private key", PRIVATE);
    await submit();
    expect(screen.getByText("the public key doesn't belong to this private key")).toBeDefined();
  });
});
