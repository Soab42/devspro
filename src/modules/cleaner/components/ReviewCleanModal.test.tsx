import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { CleanOutcome, Finding } from "../api";
import ReviewCleanModal from "./ReviewCleanModal";

const f = (over: Partial<Finding>): Finding => ({
  id: "x",
  tab: "docker",
  group: "g",
  label: "X",
  reason: "",
  method: "docker prune",
  paths: ["/some/path"],
  bytes: 1024 * 1024,
  risk: "review",
  suggested: false,
  blocked_by: null,
  needs_confirm_text: null,
  ...over,
});

const ITEMS = [
  f({ id: "a", label: "Build cache", paths: ["/var/lib/docker/buildkit"] }),
  f({ id: "b", label: "Volumes", risk: "danger", bytes: 3 * 1024 * 1024, needs_confirm_text: "DELETE VOLUMES" }),
];

afterEach(cleanup);

const confirmButton = () => screen.getByRole("button", { name: /^Clean/ }) as HTMLButtonElement;

describe("ReviewCleanModal", () => {
  it("lists label, paths, risk, method and the total", () => {
    render(<ReviewCleanModal items={ITEMS} onConfirm={vi.fn()} onClose={() => {}} />);
    expect(screen.getByText("Build cache")).toBeTruthy();
    expect(screen.getByText("/var/lib/docker/buildkit")).toBeTruthy();
    expect(screen.getByText("Danger")).toBeTruthy();
    expect(screen.getAllByText("docker prune").length).toBe(2);
    expect(screen.getByText("Total:").parentElement?.textContent).toContain("4.0 MB");
  });

  it("keeps Confirm disabled until every typed text matches", () => {
    render(<ReviewCleanModal items={ITEMS} onConfirm={vi.fn()} onClose={() => {}} />);
    expect(confirmButton().disabled).toBe(true);
    const input = screen.getByPlaceholderText("DELETE VOLUMES");
    fireEvent.change(input, { target: { value: "delete volumes" } });
    expect(confirmButton().disabled).toBe(true);
    fireEvent.change(input, { target: { value: "DELETE VOLUMES" } });
    expect(confirmButton().disabled).toBe(false);
  });

  it("is enabled at once when nothing needs typing", () => {
    render(<ReviewCleanModal items={[ITEMS[0]]} onConfirm={vi.fn()} onClose={() => {}} />);
    expect(confirmButton().disabled).toBe(false);
  });

  it("shows running, then the outcomes and bytes freed", async () => {
    let finish: (o: CleanOutcome[]) => void = () => {};
    const onConfirm = vi.fn(() => new Promise<CleanOutcome[]>((r) => (finish = r)));
    render(<ReviewCleanModal items={[ITEMS[0]]} onConfirm={onConfirm} onClose={() => {}} />);
    fireEvent.click(confirmButton());
    expect(onConfirm).toHaveBeenCalledWith(["a"], []);
    expect(screen.getByText(/Cleaning 1 items/)).toBeTruthy();
    await act(async () => finish([{ id: "a", ok: true, message: "pruned", bytes_freed: 2 * 1024 * 1024 }]));
    await waitFor(() => expect(screen.getByText(/Freed/).textContent).toContain("2.0 MB"));
    expect(screen.getByText(/Build cache: pruned/)).toBeTruthy();
  });

  it("passes the ids whose text matched as confirmed", async () => {
    const onConfirm = vi.fn(async () => [] as CleanOutcome[]);
    render(<ReviewCleanModal items={ITEMS} onConfirm={onConfirm} onClose={() => {}} />);
    fireEvent.change(screen.getByPlaceholderText("DELETE VOLUMES"), { target: { value: "DELETE VOLUMES" } });
    await act(async () => fireEvent.click(confirmButton()));
    expect(onConfirm).toHaveBeenCalledWith(["a", "b"], ["b"]);
  });

  it("shows a failure from onConfirm", async () => {
    const onConfirm = vi.fn(async () => {
      throw new Error("daemon gone");
    });
    render(<ReviewCleanModal items={[ITEMS[0]]} onConfirm={onConfirm} onClose={() => {}} />);
    await act(async () => fireEvent.click(confirmButton()));
    expect(screen.getByText("daemon gone")).toBeTruthy();
  });
});
