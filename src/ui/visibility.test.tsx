import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { Select } from "./Select";

/**
 * Regressions for three ways controls went missing after the redesign.
 *
 * All three shared a root cause: a container quietly clipping or hiding its
 * children. They are grouped here because the failure mode is identical from
 * the user's side — the button is in the DOM, passes every other test, and
 * cannot be seen or clicked.
 */

const STYLES = import.meta.glob("./*.css", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/**
 * Strips CSS comments before matching.
 *
 * Each of these rules is documented in prose directly above the declaration it
 * governs — and that prose quotes the very thing being banned ("pinned at
 * flex: 0 0 auto…"), so an unfiltered match flags the explanation as the bug.
 */
function css(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "");
}

const dataTableCss = css(STYLES["./DataTable.css"]);
const monoCss = css(STYLES["./Mono.css"]);
const selectCss = css(STYLES["./Select.css"]);
const segmentedCss = css(STYLES["./SegmentedControl.css"]);
const sparklineCss = css(STYLES["./Sparkline.css"]);

const TOKENS = import.meta.glob("../styles/tokens.css", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const tokensCss = css(Object.values(TOKENS)[0]);

function zIndex(name: string): number {
  const match = tokensCss.match(new RegExp(`--z-${name}:\\s*(\\d+)`));
  if (!match) throw new Error(`--z-${name} is not defined`);
  return Number(match[1]);
}

describe("Select escapes its clipping context", () => {
  // .card sets overflow:hidden for its rounded corners and .dt-scroll combines
  // overflow-x:auto with container-type — which contains even absolutely
  // positioned descendants. Anchored inline, the listbox was cut off at the
  // card or table edge, so the Apps version dropdown opened into nothing.
  it("renders its listbox in a portal outside the trigger", () => {
    const { container } = render(
      <Select
        id="t"
        value="a"
        onChange={() => {}}
        options={[
          { value: "a", label: "Alpha" },
          { value: "b", label: "Beta" },
        ]}
      />,
    );

    // fireEvent, not .click(): it wraps the dispatch in act() so React has
    // flushed the open state before the assertion runs.
    fireEvent.click(screen.getByRole("button"));

    const listbox = screen.getByRole("listbox");
    expect(listbox).toBeDefined();
    // The point of the fix: it must NOT be a descendant of the trigger's box.
    expect(container.contains(listbox)).toBe(false);
    expect(document.body.contains(listbox)).toBe(true);
  });

  it("is positioned in viewport coordinates, not relative to its parent", () => {
    // A portalled menu anchored with position:absolute would land relative to
    // <body> rather than the trigger.
    expect(selectCss).toMatch(/\.custom-select-menu\s*\{[^}]*position:\s*fixed/);
  });

  it("still refuses to use a native select", () => {
    // The whole reason this component exists — WebKitGTK renders native
    // <select> popups with GTK's colours, ignoring the theme.
    const { container } = render(
      <Select id="t" value="a" onChange={() => {}} options={[{ value: "a", label: "A" }]} />,
    );
    expect(container.querySelector("select")).toBeNull();
  });
});

describe("table cells do not clip their controls", () => {
  // .dt-td uses the max-width:0 + overflow:hidden ellipsis hack, which works by
  // forcing a column below its min-content width. Applied to every cell it
  // clipped action buttons, checkboxes and dropdowns down to a sliver.
  it("exempts cells containing interactive elements from the truncation hack", () => {
    const rule = dataTableCss.match(
      /\.dt-td:has\(([^)]*)\)\s*\{([^}]*)\}/,
    );
    expect(rule).not.toBeNull();

    const [, selectors, body] = rule!;
    for (const control of ["button", "input", "a", ".action-group"]) {
      expect(selectors).toContain(control);
    }
    expect(body).toMatch(/max-width:\s*none/);
    expect(body).toMatch(/overflow:\s*visible/);
  });

  it("keeps truncation for ordinary text cells", () => {
    // The exemption must stay an exemption — the base rule still truncates.
    expect(dataTableCss).toMatch(/\.dt-td\s*\{[^}]*max-width:\s*0/);
  });
});

describe("stacking order", () => {
  // Every one of these pairs, inverted, hides something behind something else.
  it("layers overlays from back to front", () => {
    expect(zIndex("backdrop")).toBeLessThan(zIndex("sidebar"));
    expect(zIndex("sidebar")).toBeLessThan(zIndex("modal"));
    expect(zIndex("modal")).toBeLessThan(zIndex("popover"));
  });

  it("puts the drawer above the scrim that dims it", () => {
    // Inverted, the open drawer sits behind its own overlay — dimmed, and
    // every click lands on the backdrop and closes it again.
    expect(zIndex("sidebar")).toBeGreaterThan(zIndex("backdrop"));
  });

  it("puts portalled popovers above modals", () => {
    // Select is portalled to <body>, leaving the modal's stacking context, and
    // is used inside five modals.
    expect(zIndex("popover")).toBeGreaterThan(zIndex("modal"));
  });
});

describe("nothing forces the page wider than the window", () => {
  it("lets the sidebar nav scroll instead of clipping", () => {
    // overflow-y:auto alone does nothing on a flex item: min-height defaults to
    // auto, so it refuses to shrink below its content and .shell's
    // overflow:hidden clips the tail off instead.
    const styles = import.meta.glob("../app/Shell.css", {
      query: "?raw",
      import: "default",
      eager: true,
    }) as Record<string, string>;
    const shellCss = css(Object.values(styles)[0]);
    const rule = shellCss.match(/\.sidebar-nav\s*\{([^}]*)\}/)?.[1] ?? "";
    expect(rule).toMatch(/overflow-y:\s*auto/);
    expect(rule).toMatch(/min-height:\s*0/);
  });

  it("keeps the segmented control shrinkable", () => {
    // Pinned at flex:0 0 auto, the Dashboard's six-facet filter pushed the
    // content area wider than the window and scrolled the whole page.
    const rule = segmentedCss.match(/^\.segmented\s*\{([^}]*)\}/m)?.[1] ?? "";
    expect(rule).not.toMatch(/flex:\s*0\s+0\s+auto/);
    expect(rule).toMatch(/max-width:\s*100%/);
  });

  it("clips sparkline ink so it cannot extend the scroll area", () => {
    // Visual overflow still counts toward the nearest scroll container's
    // scrollable region, and a 2px stroke sits right on the viewBox edge.
    const rule = sparklineCss.match(/\.sparkline\s*\{([^}]*)\}/)?.[1] ?? "";
    expect(rule).not.toMatch(/overflow:\s*visible/);
  });
});

describe("CopyButton", () => {
  // The dim-until-hover treatment was written unscoped, so a standalone copy
  // button — the Logs toolbar, the log viewer and command modal footers — sat
  // at opacity 0 permanently.
  it("is not hidden by default", () => {
    const base = monoCss.match(/^\.copy-button\s*\{([^}]*)\}/m);
    expect(base).not.toBeNull();
    expect(base![1]).not.toMatch(/opacity:\s*0/);
  });

  it("dims only inside table cells", () => {
    expect(monoCss).toMatch(/\.dt-td\s+\.copy-button\s*\{[^}]*opacity:\s*0/);
  });
});
