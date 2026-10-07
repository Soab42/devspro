import { beforeEach, describe, expect, it } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { ZoomControl } from "../../../app/ZoomControl";
import { getZoom, setZoom } from "../../../app/zoom";

describe("Interface size control", () => {
  beforeEach(() => {
    localStorage.clear();
    act(() => setZoom(100));
  });

  it("shows the current percentage", () => {
    act(() => setZoom(125));
    render(<ZoomControl />);
    expect(screen.getByLabelText("Interface size").textContent).toContain("125%");
  });

  it("resets to 100%", () => {
    act(() => setZoom(150));
    render(<ZoomControl />);
    fireEvent.click(screen.getByRole("button", { name: "Reset" }));
    expect(getZoom()).toBe(100);
  });

  it("disables Reset at the default", () => {
    render(<ZoomControl />);
    expect((screen.getByRole("button", { name: "Reset" }) as HTMLButtonElement).disabled).toBe(true);
  });
});
