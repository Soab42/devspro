import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { AuthProvider, useAuth } from "./AuthContext";
import CreateProfile from "./CreateProfile";
import LockScreen from "./LockScreen";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

const me = { id: "p1", display_name: "Talha", created_at: "", has_password: false };

/** What App does with the auth status, minus the app itself. */
function Gate() {
  const { status } = useAuth();
  if (status === "no-profile") return <CreateProfile />;
  if (status === "locked") return <LockScreen />;
  return <span>{status}</span>;
}

function start(commands: Record<string, unknown>) {
  invoke.mockImplementation(async (cmd: string) => commands[cmd]);
  render(
    <AuthProvider>
      <Gate />
    </AuthProvider>,
  );
}

beforeEach(() => {
  cleanup();
  invoke.mockReset();
});

describe("first launch (#1)", () => {
  it("can be skipped: a profile with no name or password, and the app opens", async () => {
    start({ auth_has_profile: false, auth_create_profile: me });
    fireEvent.click(await screen.findByText(/Skip/));
    expect(await screen.findByText("unlocked")).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("auth_create_profile", { displayName: "", password: "" });
  });

  it("needs no password, and asks to confirm only one that is typed", async () => {
    start({ auth_has_profile: false, auth_create_profile: me });
    expect(await screen.findByText("Continue without a password")).toBeTruthy();
    expect(screen.queryByLabelText(/Confirm password/)).toBeNull();

    fireEvent.change(screen.getByLabelText(/^Password/), { target: { value: "s3cret" } });
    expect(screen.getByLabelText(/Confirm password/)).toBeTruthy();
    expect(screen.getByText("Create profile")).toBeTruthy();
  });
});

describe("lock screen", () => {
  it("opens a profile without a password with a plain Continue", async () => {
    start({ auth_has_profile: true, auth_current_session: null, auth_list_profiles: [me], auth_unlock: me });
    fireEvent.click(await screen.findByText("Continue"));
    expect(await screen.findByText("unlocked")).toBeTruthy();
    expect(screen.queryByLabelText("Password")).toBeNull();
    expect(invoke).toHaveBeenCalledWith("auth_unlock", { profileId: "p1", password: "" });
  });

  it("still asks for the password of a profile that has one", async () => {
    start({
      auth_has_profile: true,
      auth_current_session: null,
      auth_list_profiles: [{ ...me, has_password: true }],
    });
    expect(await screen.findByText("Unlock")).toBeTruthy();
    expect(screen.getByLabelText("Password")).toBeTruthy();
  });
});
