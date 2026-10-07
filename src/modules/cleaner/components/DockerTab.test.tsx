import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { DockerScan, Finding } from "../api";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import DockerTab from "./DockerTab";
import { resetTabState } from "../tabState";

const f = (over: Partial<Finding>): Finding => ({
  id: "x",
  tab: "docker",
  group: "Dangling images",
  label: "X",
  reason: "",
  method: "docker image rm",
  paths: [],
  bytes: 1024 * 1024,
  risk: "safe",
  suggested: false,
  blocked_by: null,
  needs_confirm_text: null,
  ...over,
});

const FINDINGS = [
  f({ id: "image:111111111111", label: "Untagged image 111111111111", suggested: true }),
  f({ id: "container:bd830e46b3c6", label: "web-1 (web)", group: "Stopped containers", risk: "review" }),
  f({
    id: "volume:devspro_pgdata",
    label: "devspro_pgdata",
    group: "Volumes",
    risk: "danger",
    needs_confirm_text: "devspro_pgdata",
    method: "docker volume rm",
  }),
];

function mockBackend(scan: DockerScan, detection?: unknown) {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === "cleaner_scan_docker") return scan;
    if (cmd === "docker_detect") return detection;
    if (cmd === "cleaner_clean_docker") return [{ id: "volume:devspro_pgdata", ok: true, message: "Removed", bytes_freed: 5 }];
    throw new Error(`unexpected ${cmd}`);
  });
}

beforeEach(() => {
  resetTabState();
  invoke.mockReset();
});
afterEach(cleanup);

describe("DockerTab", () => {
  it("shows the Docker page's callout when the daemon is unreachable", async () => {
    mockBackend(
      { available: false, message: "permission denied", findings: [] },
      { found: true, daemon_reachable: false, version: null, error: "permission denied" },
    );
    render(<DockerTab />);
    await waitFor(() => expect(screen.getByText("Docker daemon unreachable")).toBeTruthy());
    expect(screen.getAllByText(/permission denied/).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Recheck" })).toBeTruthy();
    expect(screen.queryByRole("table")).toBeNull();
    expect((screen.getByRole("button", { name: "Select suggested" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("says so when Docker is not installed", async () => {
    mockBackend(
      { available: false, message: "Docker isn't installed.", findings: [] },
      { found: false, daemon_reachable: false, version: null, error: null },
    );
    render(<DockerTab />);
    await waitFor(() => expect(screen.getByText("Docker isn't installed")).toBeTruthy());
  });

  it("selects only the suggested findings", async () => {
    mockBackend({ available: true, message: null, findings: FINDINGS });
    render(<DockerTab />);
    await waitFor(() => expect(screen.getByText("web-1 (web)")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Select suggested" }));
    expect((screen.getByLabelText("Select Untagged image 111111111111") as HTMLInputElement).checked).toBe(true);
    expect((screen.getByLabelText("Select web-1 (web)") as HTMLInputElement).checked).toBe(false);
    expect((screen.getByLabelText("Select devspro_pgdata") as HTMLInputElement).checked).toBe(false);
  });

  it("makes the user type a protected volume's name, then reports it as confirmed", async () => {
    mockBackend({ available: true, message: null, findings: FINDINGS });
    render(<DockerTab />);
    await waitFor(() => expect(screen.getByText("devspro_pgdata")).toBeTruthy());
    fireEvent.click(screen.getByLabelText("Select devspro_pgdata"));
    fireEvent.click(screen.getByRole("button", { name: /^Review and clean/ }));

    const clean = () => screen.getByRole("button", { name: /^Clean/ }) as HTMLButtonElement;
    expect(clean().disabled).toBe(true);
    const input = screen.getByPlaceholderText("devspro_pgdata");
    fireEvent.change(input, { target: { value: "devspro_pg" } });
    expect(clean().disabled).toBe(true);
    fireEvent.change(input, { target: { value: "devspro_pgdata" } });
    expect(clean().disabled).toBe(false);

    fireEvent.click(clean());
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cleaner_clean_docker", {
        ids: ["volume:devspro_pgdata"],
        confirmed: ["volume:devspro_pgdata"],
      }),
    );
    await waitFor(() => expect(screen.getByText(/devspro_pgdata: Removed/)).toBeTruthy());
  });

  it("rescans after a clean once the modal is closed", async () => {
    mockBackend({ available: true, message: null, findings: FINDINGS });
    render(<DockerTab />);
    await waitFor(() => expect(screen.getByText("web-1 (web)")).toBeTruthy());
    fireEvent.click(screen.getByLabelText("Select web-1 (web)"));
    fireEvent.click(screen.getByRole("button", { name: /^Review and clean/ }));
    fireEvent.click(screen.getByRole("button", { name: /^Clean/ }));
    await waitFor(() => expect(screen.getByText(/Freed/)).toBeTruthy());
    fireEvent.click(screen.getAllByRole("button", { name: "Close" }).pop() as HTMLElement);
    await waitFor(() => expect(invoke.mock.calls.filter((c) => c[0] === "cleaner_scan_docker").length).toBe(2));
  });
});
