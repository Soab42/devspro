import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { AuthProvider, useAuth } from "./AuthContext";
import CreateProfile from "./CreateProfile";
import LockScreen from "./LockScreen";
import PasswordSection from "../modules/settings/components/PasswordSection";

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

  it("offers no reset for a profile without a password", async () => {
    start({ auth_has_profile: true, auth_current_session: null, auth_list_profiles: [me] });
    expect(await screen.findByText("Continue")).toBeTruthy();
    expect(screen.queryByText("Forgot password?")).toBeNull();
  });
});

describe("forgotten password", () => {
  const locked = { ...me, has_password: true };

  it("resets with a new password after the OS prompt, and opens the profile", async () => {
    start({
      auth_has_profile: true,
      auth_current_session: null,
      auth_list_profiles: [locked],
      auth_reset_password: locked,
    });
    fireEvent.click(await screen.findByText("Forgot password?"));
    expect(screen.getByText("Reset password", { selector: "h1" })).toBeTruthy();

    fireEvent.change(screen.getByLabelText(/^New password/), { target: { value: "fresh" } });
    fireEvent.change(screen.getByLabelText(/Confirm new password/), { target: { value: "fresh" } });
    fireEvent.click(screen.getByText("Reset password", { selector: "button *, button" }));

    expect(await screen.findByText("unlocked")).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("auth_reset_password", { profileId: "p1", newPassword: "fresh" });
  });

  it("stays locked and says why when the OS prompt is declined", async () => {
    const commands: Record<string, unknown> = {
      auth_has_profile: true,
      auth_current_session: null,
      auth_list_profiles: [locked],
    };
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "auth_reset_password") throw "the administrator check didn't go through, so the password is unchanged";
      return commands[cmd];
    });
    render(
      <AuthProvider>
        <Gate />
      </AuthProvider>,
    );
    fireEvent.click(await screen.findByText("Forgot password?"));
    fireEvent.click(screen.getByText("Remove password", { selector: "button *, button" }));

    expect(await screen.findByText(/password is unchanged/)).toBeTruthy();
    expect(screen.queryByText("unlocked")).toBeNull();
    fireEvent.click(screen.getByText("Back to unlock"));
    expect(screen.getByText("Unlock")).toBeTruthy();
  });
});

describe("Settings → Password", () => {
  function settings(session: typeof me, changed: typeof me) {
    invoke.mockImplementation(async (cmd: string) =>
      ({ auth_has_profile: true, auth_current_session: session, auth_change_password: changed })[cmd],
    );
    render(
      <AuthProvider>
        <PasswordSection />
      </AuthProvider>,
    );
  }

  it("changes a password given the current one", async () => {
    settings({ ...me, has_password: true }, { ...me, has_password: true });
    fireEvent.change(await screen.findByLabelText("Current password"), { target: { value: "old1" } });
    fireEvent.change(screen.getByLabelText(/^New password/), { target: { value: "new1" } });
    fireEvent.change(screen.getByLabelText(/Confirm new password/), { target: { value: "new1" } });
    fireEvent.click(screen.getByText("Change password", { selector: "button *, button" }));

    expect(await screen.findByText("Password changed.")).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("auth_change_password", {
      profileId: "p1",
      currentPassword: "old1",
      newPassword: "new1",
    });
  });

  it("removes it when the new one is left empty, then offers to set one", async () => {
    settings({ ...me, has_password: true }, me);
    fireEvent.change(await screen.findByLabelText("Current password"), { target: { value: "old1" } });
    fireEvent.click(screen.getByText("Remove password", { selector: "button *, button" }));

    expect(await screen.findByText(/Password removed/)).toBeTruthy();
    expect(screen.queryByLabelText("Current password")).toBeNull();
    expect(screen.getByText("Set password", { selector: "button *, button" })).toBeTruthy();
  });
});
