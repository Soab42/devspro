import { EditorState } from "@codemirror/state";
import { EditorView, ViewPlugin, ViewUpdate } from "@codemirror/view";
import { getSearchQuery, searchPanelOpen } from "@codemirror/search";

/**
 * A "3 of 12" beside the find field. CodeMirror's find/replace panel highlights
 * matches but never says how many there are, which is the first thing you want
 * to know before pressing "replace all".
 *
 * The panel is CodeMirror's own DOM, built afresh each time it opens, so the
 * count is a node slotted in after its search field rather than a rebuilt
 * panel. Should an upgrade rename that field, the count just stops showing.
 */

/** Counting stops here: past it the number is no longer what anyone reads. */
const CAP = 1000;

interface Range {
  from: number;
  to: number;
}

export type MatchList = { ranges: Range[]; capped: boolean } | "invalid" | null;

/** Every match of the current query in document order, or null with no query. */
export function findMatches(state: EditorState): MatchList {
  const query = getSearchQuery(state);
  if (!query.search) return null;
  if (!query.valid) return "invalid";
  const ranges: Range[] = [];
  const cursor = query.getCursor(state);
  for (let m = cursor.next(); !m.done; m = cursor.next()) {
    if (ranges.length === CAP) return { ranges, capped: true };
    ranges.push(m.value);
  }
  return { ranges, capped: false };
}

/** "2 of 12" when the selection is on a match — as it is after Enter — else the total. */
export function matchLabel(found: MatchList, selection: Range): string {
  if (found === null) return "";
  if (found === "invalid") return "Invalid pattern";
  const { ranges, capped } = found;
  if (ranges.length === 0) return "No matches";
  const total = capped ? `${CAP}+` : String(ranges.length);
  const at = ranges.findIndex((r) => r.from === selection.from && r.to === selection.to);
  if (at !== -1) return `${at + 1} of ${total}`;
  return `${total} ${ranges.length === 1 && !capped ? "match" : "matches"}`;
}

const countPlugin = ViewPlugin.fromClass(
  class {
    found: MatchList = null;

    constructor(readonly view: EditorView) {}

    update(update: ViewUpdate) {
      if (!searchPanelOpen(update.state)) return;
      if (
        !searchPanelOpen(update.startState) ||
        update.docChanged ||
        !getSearchQuery(update.state).eq(getSearchQuery(update.startState))
      ) {
        this.found = findMatches(update.state);
      } else if (!update.selectionSet) {
        return;
      }
      const label = matchLabel(this.found, update.state.selection.main);
      // The panel is mounted during this same update, so the write waits for
      // the measure phase, by which point its field is in the DOM.
      this.view.requestMeasure({
        key: this,
        read: () => null,
        write: () => {
          const field = this.view.dom.querySelector(".cm-search input[name=search]");
          if (!field) return;
          let count = field.parentElement?.querySelector(".cm-search-count");
          if (!count) {
            count = document.createElement("span");
            count.className = "cm-search-count";
            count.setAttribute("aria-live", "polite");
            field.after(count);
          }
          count.textContent = label;
        },
      });
    }
  },
);

export const searchMatchCount = [
  countPlugin,
  EditorView.baseTheme({
    ".cm-search-count": {
      display: "inline-block",
      // Room for "12 of 345", so the buttons after it don't shift as you step.
      minWidth: "6em",
      margin: "0 var(--space-2)",
      color: "var(--color-text-muted)",
      fontVariantNumeric: "tabular-nums",
      whiteSpace: "nowrap",
    },
    ".cm-search-count:empty": { display: "none" },
  }),
];
