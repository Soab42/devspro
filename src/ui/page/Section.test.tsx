import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { act } from "react";
import { Section } from "./Section";

describe("Section collapsible", () => {
  it("has no toggle unless asked for", () => {
    render(<Section title="Standalone">body</Section>);
    expect(
      screen.queryByRole("button", { name: /collapse|expand/i }),
    ).toBeNull();
    expect(screen.getByText("body")).toBeDefined();
  });

  it("folds the body away but keeps the header and its meta", () => {
    render(
      <Section title="singnerpro" collapsible meta={<span>0/1 running</span>}>
        body
      </Section>,
    );
    const toggle = screen.getByRole("button", { name: "Collapse section" });
    act(() => toggle.click());

    expect(screen.queryByText("body")).toBeNull();
    expect(screen.getByText("singnerpro")).toBeDefined();
    expect(screen.getByText("0/1 running")).toBeDefined();

    const expand = screen.getByRole("button", { name: "Expand section" });
    expect(expand.getAttribute("aria-expanded")).toBe("false");
    act(() => expand.click());
    expect(screen.getByText("body")).toBeDefined();
  });

  it("honours defaultCollapsed", () => {
    render(
      <Section title="singnerpro" collapsible defaultCollapsed>
        body
      </Section>,
    );
    expect(screen.queryByText("body")).toBeNull();
  });

  it("lets a parent drive the fold", () => {
    const seen: boolean[] = [];
    const { rerender } = render(
      <Section
        title="singnerpro"
        collapsible
        collapsed
        onCollapsedChange={(c) => seen.push(c)}
      >
        body
      </Section>,
    );
    expect(screen.queryByText("body")).toBeNull();

    // The toggle reports the intent; it does not fold on its own.
    act(() => screen.getByRole("button", { name: "Expand section" }).click());
    expect(seen).toEqual([false]);
    expect(screen.queryByText("body")).toBeNull();

    rerender(
      <Section
        title="singnerpro"
        collapsible
        collapsed={false}
        onCollapsedChange={(c) => seen.push(c)}
      >
        body
      </Section>,
    );
    expect(screen.getByText("body")).toBeDefined();
  });
});
