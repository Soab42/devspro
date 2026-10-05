import { describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { search, SearchQuery, setSearchQuery } from "@codemirror/search";
import { findMatches, matchLabel } from "./searchMatchCount";

function label(
  doc: string,
  query: ConstructorParameters<typeof SearchQuery>[0],
  selection = { from: 0, to: 0 },
) {
  const state = EditorState.create({ doc, extensions: [search()] }).update({
    effects: setSearchQuery.of(new SearchQuery(query)),
  }).state;
  return matchLabel(findMatches(state), selection);
}

describe("search match count", () => {
  it("says nothing until there is something to find", () => {
    expect(label("A=1\n", { search: "" })).toBe("");
  });

  it("gives the total while the selection is off any match", () => {
    expect(label("A=1\nB=1\nC=1\n", { search: "=1" })).toBe("3 matches");
    expect(label("A=1\nB=2\n", { search: "B=" })).toBe("1 match");
    expect(label("A=1\n", { search: "zzz" })).toBe("No matches");
  });

  it("gives the position once the selection sits on a match", () => {
    // "=1" at 1–3, 5–7, 9–11.
    expect(label("A=1\nB=1\nC=1\n", { search: "=1" }, { from: 5, to: 7 })).toBe("2 of 3");
  });

  it("counts by the query's own rules", () => {
    expect(label("key KEY Key\n", { search: "key" })).toBe("3 matches");
    expect(label("key KEY Key\n", { search: "key", caseSensitive: true })).toBe("1 match");
    expect(label("A_KEY=1\nKEY=2\n", { search: "KEY", wholeWord: true })).toBe("1 match");
    expect(label("A=1\nB=22\n", { search: "=\\d+$", regexp: true })).toBe("2 matches");
  });

  it("says so when the pattern is not a valid regex", () => {
    expect(label("A=1\n", { search: "(", regexp: true })).toBe("Invalid pattern");
  });

  it("stops counting at a thousand", () => {
    const doc = "a".repeat(1500);
    expect(label(doc, { search: "a" })).toBe("1000+ matches");
    expect(label(doc, { search: "a" }, { from: 2, to: 3 })).toBe("3 of 1000+");
  });
});
