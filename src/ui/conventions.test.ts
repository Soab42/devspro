import { describe, expect, it } from "vitest";

/**
 * Structural guards for the design system.
 *
 * These encode decisions that are cheap to violate by accident and expensive to
 * notice later — each one corresponds to a bug that actually existed in this
 * codebase before the redesign. They are source-level checks, so they hold for
 * every file regardless of whether it has a component test of its own.
 *
 * Sources are pulled in with import.meta.glob rather than node:fs so this file
 * typechecks under the app's own tsconfig, which has no Node types.
 */

// The options must be an inline object literal — import.meta.glob is rewritten
// at build time and cannot read a variable.
const CODE_FILES = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const STYLE_FILES = import.meta.glob("../**/*.css", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/**
 * Normalises glob keys to src-relative paths.
 *
 * Keys come back relative to THIS file (src/ui), and Vite collapses paths that
 * resolve back into the same directory — so a sibling arrives as
 * "./DataTable.tsx" rather than "../ui/DataTable.tsx".
 */
function clean(path: string): string {
  if (path.startsWith("./")) return `ui/${path.slice(2)}`;
  return path.replace(/^\.\.\//, "");
}

const isTest = (p: string) => /\.test\.tsx?$/.test(p);

const SOURCES = Object.entries(CODE_FILES)
  .filter(([p]) => !isTest(p))
  .map(([p, source]) => ({ path: clean(p), source }));

const MODULES = SOURCES.filter((f) => f.path.startsWith("modules/"));

const STYLES = Object.entries(STYLE_FILES).map(([p, source]) => ({
  path: clean(p),
  source,
}));

/**
 * Strips comments before scanning.
 *
 * Needed because several of these rules are documented in prose right beside
 * the code upholding them — "Was the last native <select> in the app…" — and a
 * naive match flags the explanation as the violation.
 */
function code(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, "") // block and JSDoc comments
    .replace(/(^|[^:])\/\/.*$/gm, "$1"); // line comments, but not http://
}

function offenders(
  files: { path: string; source: string }[],
  predicate: (source: string, path: string) => boolean,
): string[] {
  return files.filter((f) => predicate(f.source, f.path)).map((f) => f.path);
}

describe("no emoji", () => {
  // The original UI used emoji as its icon language: 11 on the dashboard alone,
  // plus glyph buttons for the menu and theme toggles. They render
  // inconsistently across platforms and fonts — the rule is SVG only.
  const EMOJI = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}\u{2B00}-\u{2BFF}\u{FE0F}]/u;

  it("no source or stylesheet contains a pictographic glyph", () => {
    const all = [...SOURCES, ...STYLES];
    expect(offenders(all, (s) => EMOJI.test(s))).toEqual([]);
  });
});

describe("page structure", () => {
  it("no module renders its own <h1>", () => {
    // Pages used to early-return a whole shell per state, so DockerPage and
    // MailCatcherPage shipped three <h1> elements each. <Page> owns the
    // heading now, and this keeps it that way.
    expect(offenders(MODULES, (s) => /<h1[\s>]/.test(code(s)))).toEqual([]);
  });
});

describe("native select ban", () => {
  it("nothing uses a native <select> or <option>", () => {
    // src/ui/Select.tsx exists because WebKitGTK renders native <select>
    // popups with GTK's own colours, ignoring our custom properties, which
    // makes the option list unreadable in dark mode.
    expect(
      offenders(SOURCES, (s) => /<select[\s>]|<option[\s>]/.test(code(s))),
    ).toEqual([]);
  });
});

describe("styling discipline", () => {
  // Inline styles are allowed only where the value IS the data and therefore
  // cannot be expressed as a static class.
  const DYNAMIC_STYLE_OK = new Set([
    "ui/Meter.tsx", // fill width is the measurement itself
    "ui/Feedback.tsx", // skeleton dimensions vary per instance
    "ui/Mono.tsx", // truncation width supplied by the caller
    "ui/Menu.tsx", // portal position, measured at runtime
    "ui/Select.tsx", // portal position, measured at runtime
    "ui/DataTable.tsx", // per-column width from the column config
    "ui/Layout.tsx", // Stack align/justify pass-through
    "ui/Field.tsx", // SearchInput width supplied by the caller
    "ui/CodeEditor.tsx", // editor height derives from the caller's `rows`
  ]);

  it("inline styles appear only where the value is dynamic", () => {
    expect(
      offenders(
        SOURCES,
        (s, path) => code(s).includes("style={{") && !DYNAMIC_STYLE_OK.has(path),
      ),
    ).toEqual([]);
  });

  it("only src/ui/icons.ts imports lucide-react", () => {
    expect(
      offenders(
        SOURCES,
        (s, path) => path !== "ui/icons.ts" && /from "lucide-react"/.test(code(s)),
      ),
    ).toEqual([]);
  });

  it("no stylesheet hardcodes the overlay colour", () => {
    // Was duplicated as rgba(10,12,16,0.4) in Shell.css and 0.45 in Modal.css;
    // both now read var(--overlay).
    expect(offenders(STYLES, (s) => /rgba\(10,\s*12,\s*16/.test(s))).toEqual([]);
  });

  it("module stylesheets stay the documented exception", () => {
    // Only two page-level stylesheets are sanctioned: the Dashboard's bespoke
    // grid and the Logs page's full-height layout. Everything else composes
    // primitives, which is what keeps styling traceable to one owner.
    const moduleStyles = STYLES.filter((f) => f.path.startsWith("modules/")).map(
      (f) => f.path,
    );
    expect(moduleStyles.sort()).toEqual([
      "modules/browser/pages/BrowserPage.css",
      "modules/dashboard/pages/DashboardPage.css",
      "modules/file-manager/pages/FileManagerPage.css",
      "modules/logs/pages/LogsPage.css",
    ]);
  });
});
