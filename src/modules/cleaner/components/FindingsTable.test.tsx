import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { Finding } from "../api";
import { FindingsTable } from "./FindingsTable";

const f = (over: Partial<Finding>): Finding => ({
  id: "x",
  tab: "caches",
  group: "npm",
  label: "X",
  reason: "rebuilds itself",
  method: "trash",
  paths: ["/p"],
  bytes: 2048,
  risk: "safe",
  suggested: false,
  blocked_by: null,
  needs_confirm_text: null,
  ...over,
});

const FINDINGS = [
  f({ id: "a", label: "npm cache", group: "Node", bytes: 1024 * 1024 }),
  f({ id: "b", label: "yarn cache", group: "Node", bytes: 2048, risk: "review" }),
  f({ id: "c", label: "docker volumes", group: "Docker", bytes: 10, blocked_by: "docker is running" }),
];

afterEach(cleanup);

describe("FindingsTable", () => {
  it("shows group headings, risk badges and sizes", () => {
    render(<FindingsTable findings={FINDINGS} selected={new Set()} onSelectionChange={() => {}} />);
    expect(screen.getByText(/^Node ·/)).toBeTruthy();
    expect(screen.getByText(/^Docker ·/)).toBeTruthy();
    expect(screen.getAllByText("Safe").length).toBeTruthy();
    expect(screen.getByText("Review")).toBeTruthy();
    expect(screen.getByText("1.0 MB")).toBeTruthy();
  });

  it("reports a ticked row to the parent without owning the state", () => {
    const onChange = vi.fn();
    render(<FindingsTable findings={FINDINGS} selected={new Set(["b"])} onSelectionChange={onChange} />);
    expect((screen.getByLabelText("Select yarn cache") as HTMLInputElement).checked).toBe(true);
    fireEvent.click(screen.getByLabelText("Select npm cache"));
    expect([...onChange.mock.calls[0][0]].sort()).toEqual(["a", "b"]);
  });

  it("unticks a selected row", () => {
    const onChange = vi.fn();
    render(<FindingsTable findings={FINDINGS} selected={new Set(["a"])} onSelectionChange={onChange} />);
    fireEvent.click(screen.getByLabelText("Select npm cache"));
    expect([...onChange.mock.calls[0][0]]).toEqual([]);
  });

  it("shows why a row is blocked and won't select it", () => {
    render(<FindingsTable findings={FINDINGS} selected={new Set()} onSelectionChange={() => {}} />);
    expect(screen.getByText(/docker is running/)).toBeTruthy();
    expect((screen.getByLabelText("Select docker volumes") as HTMLInputElement).disabled).toBe(true);
  });

  it("a group checkbox selects only what is selectable in it", () => {
    const onChange = vi.fn();
    render(<FindingsTable findings={FINDINGS} selected={new Set()} onSelectionChange={onChange} />);
    fireEvent.click(screen.getByLabelText(/^Node ·/));
    expect([...onChange.mock.calls[0][0]].sort()).toEqual(["a", "b"]);
  });

  it("renders the empty state", () => {
    render(
      <FindingsTable findings={[]} selected={new Set()} onSelectionChange={() => {}} empty={{ title: "All clean" }} />,
    );
    expect(screen.getByText("All clean")).toBeTruthy();
  });
});
