import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { Page, PageStatus } from "./Page";

/**
 * The regression this file exists for: pages used to early-return a full page
 * shell per state, so Docker and MailCatcher each rendered three <h1> elements
 * and Dashboard two. Page emits the heading above the status branch, making
 * that structurally impossible — these tests hold that line.
 */
const STATUSES: { name: string; status: PageStatus }[] = [
  { name: "ready", status: { kind: "ready" } },
  { name: "loading", status: { kind: "loading" } },
  { name: "error", status: { kind: "error", message: "boom" } },
  {
    name: "blocked",
    status: { kind: "blocked", title: "Not installed", message: "Install it" },
  },
];

describe("Page", () => {
  it.each(STATUSES)("renders exactly one <h1> when $name", ({ status }) => {
    render(
      <Page title="Docker" status={status}>
        <p>body</p>
      </Page>,
    );
    const headings = screen.getAllByRole("heading", { level: 1 });
    expect(headings).toHaveLength(1);
    expect(headings[0].textContent).toBe("Docker");
  });

  it("renders children only when ready", () => {
    const { rerender } = render(
      <Page title="Docker" status={{ kind: "ready" }}>
        <p>body</p>
      </Page>,
    );
    expect(screen.getByText("body")).toBeDefined();

    rerender(
      <Page title="Docker" status={{ kind: "loading" }}>
        <p>body</p>
      </Page>,
    );
    expect(screen.queryByText("body")).toBeNull();
  });

  it("keeps header actions visible while loading so the header does not jump", () => {
    render(
      <Page title="Docker" status={{ kind: "loading" }} actions={<button>Refresh</button>}>
        <p>body</p>
      </Page>,
    );
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDefined();
  });

  it("hides header actions when blocked", () => {
    // A blocked page's own call-to-action lives in the empty state; showing the
    // page-level actions too would offer two competing next steps.
    render(
      <Page
        title="Docker"
        status={{ kind: "blocked", title: "Not installed", message: "x" }}
        actions={<button>Refresh</button>}
      >
        <p>body</p>
      </Page>,
    );
    expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull();
  });

  it("surfaces the blocked action and message", () => {
    render(
      <Page
        title="Docker"
        status={{
          kind: "blocked",
          title: "Docker isn't installed",
          message: "Install it, then recheck.",
          action: { label: "Recheck", onClick: () => {} },
        }}
      >
        <p>body</p>
      </Page>,
    );
    expect(screen.getByText("Docker isn't installed")).toBeDefined();
    expect(screen.getByRole("button", { name: "Recheck" })).toBeDefined();
  });
});
