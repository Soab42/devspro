import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import LiveVersionPicker from "./LiveVersionPicker";

const VERSIONS = ["4.97.0", "4.96.3", "4.96.2", "4.96.1", "4.96.0", "4.95.0", "4.92.0"];

// Each test gets its own app id, so the module's per-session cache of the
// fetched list never carries one test's answer into the next.
let n = 0;
function setup() {
  const onChange = vi.fn();
  const appId = `docker-${n++}`;
  render(<LiveVersionPicker appId={appId} appName="Docker" onChange={onChange} />);
  return { onChange, appId };
}

const openMenu = () => fireEvent.click(screen.getByLabelText("Version for Docker"));
const optionLabels = () => screen.queryAllByRole("option").map((o) => o.textContent);

describe("LiveVersionPicker", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("starts on Latest and fetches nothing until opened", () => {
    const { onChange } = setup();
    expect(invoke).not.toHaveBeenCalled();
    expect(onChange).toHaveBeenLastCalledWith({ version: null, valid: true });
  });

  it("lists Latest, the five newest, and Other version… once opened", async () => {
    invoke.mockResolvedValue(VERSIONS);
    const { appId } = setup();
    openMenu();
    expect(invoke).toHaveBeenCalledWith("apps_live_versions", { id: appId });
    await waitFor(() =>
      expect(optionLabels()).toEqual([
        "Latest",
        "4.97.0",
        "4.96.3",
        "4.96.2",
        "4.96.1",
        "4.96.0",
        "Other version…",
      ]),
    );
  });

  it("ticks a typed version that's published and crosses one that isn't", async () => {
    invoke.mockResolvedValue(VERSIONS);
    const { onChange } = setup();
    openMenu();
    await waitFor(() => expect(optionLabels()).toContain("4.97.0"));
    fireEvent.click(screen.getByRole("option", { name: "Other version…" }));

    const input = screen.getByLabelText("Docker version to install");
    fireEvent.change(input, { target: { value: "4.92.0" } });
    expect(screen.getByRole("img", { name: "Docker 4.92.0 is available" })).toBeTruthy();
    expect(onChange).toHaveBeenLastCalledWith({ version: "4.92.0", valid: true });

    fireEvent.change(input, { target: { value: "4.10.9" } });
    expect(screen.getByRole("img", { name: "Docker 4.10.9 isn't available" })).toBeTruthy();
    expect(onChange).toHaveBeenLastCalledWith({ version: "4.10.9", valid: false });
  });

  it("says when the list couldn't load instead of claiming a version is missing", async () => {
    invoke.mockRejectedValue(new Error("no network"));
    setup();
    openMenu();
    await waitFor(() => expect(optionLabels()).toContain("Couldn't load versions"));
    fireEvent.click(screen.getByRole("option", { name: "Other version…" }));
    fireEvent.change(screen.getByLabelText("Docker version to install"), {
      target: { value: "4.92.0" },
    });
    await waitFor(() => expect(screen.getByRole("img", { name: /Couldn't check 4\.92\.0/ })).toBeTruthy());
  });
});
