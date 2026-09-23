import { describe, expect, it } from "vitest";

// Read with import.meta.glob rather than node:fs, so this file typechecks
// under the app's own tsconfig, which has no Node types — same reason as
// ui/conventions.test.ts.
const raw = (glob: Record<string, unknown>) =>
  Object.values(glob as Record<string, string>)[0];

const css = raw(
  import.meta.glob("./browser.css", {
    query: "?raw",
    import: "default",
    eager: true,
  }),
);

const pageCss = raw(
  import.meta.glob("../../../ui/page/Page.css", {
    query: "?raw",
    import: "default",
    eager: true,
  }),
);

const fieldCss = raw(
  import.meta.glob("../../../ui/Field.css", {
    query: "?raw",
    import: "default",
    eager: true,
  }),
);

const sidebar = raw(
  import.meta.glob("./ObjectSidebar.tsx", {
    query: "?raw",
    import: "default",
    eager: true,
  }),
);

const browserPage = raw(
  import.meta.glob("../pages/DataBrowserPage.tsx", {
    query: "?raw",
    import: "default",
    eager: true,
  }),
);

/** The rule body for a selector, so a declaration can be asserted within it. */
function block(selector: string, sheet = css): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`${escaped}[^{]*\\{([^}]*)\\}`).exec(sheet);
  if (!match) throw new Error(`no rule for ${selector}`);
  return match[1];
}

/**
 * The browser fills the window and scrolls its panes rather than the page.
 *
 * That rests on an unbroken chain of flex parents that are each allowed to
 * shrink. A flex child defaults to `min-height: auto` and refuses to go below
 * its content, so dropping `min-height: 0` anywhere along the chain silently
 * reverts the whole thing to "the page scrolls" — which is exactly the bug
 * this layout was added to fix, and it is invisible until a table is long
 * enough to overflow.
 */
describe("the browser's fill-height chain", () => {
  it("lets every link in the chain shrink", () => {
    for (const selector of [
      ".db-page-fill",
      ".db-browser-root",
      ".db-tab-scroll",
      ".db-sidebar-list",
    ]) {
      expect(block(selector)).toMatch(/min-height:\s*0/);
    }
  });

  /**
   * `.page` is `min-height: 100%`, which computes to `auto` — and a percentage
   * height against an auto-height ancestor resolves to auto too. So the page's
   * own bound has to be `height`, not `min-height`, or nothing below it works.
   */
  it("starts from a page bounded to the window, not merely at least as tall", () => {
    expect(block(".page-fill", pageCss)).toMatch(/(^|[^-])height:\s*100%/);
    expect(block(".page-fill", pageCss)).not.toMatch(/min-height:\s*100%/);
  });

  /**
   * That percentage is the only one, and everything below it is plain flex.
   *
   * The first attempt chained `height: 100%` down through `.page-body`, which
   * is a *block* — whether a percentage resolves against a flex item's used
   * height is exactly the kind of thing that works in one window and not in
   * another, and it did: the popped-out window came out unbounded. Making
   * `.page-body` a flex column means every level below asks for `flex: 1`,
   * which needs nothing resolved.
   */
  it("hands the rest of the chain over to flex, with no percentage left in it", () => {
    const body = block(".page-fill > .page-body", pageCss);
    expect(body).toMatch(/display:\s*flex/);
    expect(body).toMatch(/flex-direction:\s*column/);
    expect(body).toMatch(/min-height:\s*0/);

    expect(block(".db-page-fill")).toMatch(/flex:\s*1/);
    expect(block(".db-page-fill")).not.toMatch(/height:\s*100%/);
  });

  it("is actually switched on by the page", () => {
    // The CSS above does nothing unless the page opts in.
    expect(browserPage).toMatch(/\bfill\b/);
  });

  it("gives the sections-only tabs their own scrollbar", () => {
    expect(block(".db-tab-scroll")).toMatch(/overflow-y:\s*auto/);
  });

  /**
   * `SearchInput` is `flex: 1`, which is right in the toolbars it mostly lives
   * in and wrong here: this is a column, so growing means growing *taller*.
   * The box stretched to the leftover height and `align-items: center` left
   * the input floating in the middle of an empty panel.
   */
  it("keeps the filter box its own height rather than letting it stretch", () => {
    expect(sidebar).toMatch(/grow=\{false\}/);
    expect(block(".search-input-fixed", fieldCss)).toMatch(/flex:\s*0 0 auto/);
  });

  /* The list scrolls, not the aside — otherwise the filter box scrolls away,
     in exactly the database that has too many tables to read without it. */
  it("scrolls the table list under a filter box that stays put", () => {
    expect(block(".db-sidebar-list")).toMatch(/overflow-y:\s*auto/);
    expect(block(".db-sidebar-list")).toMatch(/min-height:\s*0/);
    const aside = block(".db-browser .split-view-aside");
    expect(aside).toMatch(/overflow:\s*hidden/);
    // A column, so the list's `flex: 1` means height rather than width.
    expect(aside).toMatch(/flex-direction:\s*column/);
  });

  /* The Data tab deliberately has no scrollbar of its own: its filter bar and
     pager stay put while the grid scrolls between them, which is the
     DataTable's `fillHeight` doing the work. */
  it("does not give the data tab one, so the grid scrolls instead", () => {
    expect(block(".db-tab-fill")).not.toMatch(/overflow/);
  });
});
