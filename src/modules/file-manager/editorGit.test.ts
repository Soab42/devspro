import { describe, expect, it } from "vitest";
import { EditorState, Extension } from "@codemirror/state";
import type { GitBlame } from "./api";
import { changeCount, changeGutter, commitAtCursor, inlineBlame } from "./editorGit";

const BASE = "one\ntwo\nthree\nfour\n";

function stateWith(doc: string, ...extensions: Extension[]) {
  return EditorState.create({ doc, extensions });
}

describe("change gutter", () => {
  it("counts changes against the staged copy and keeps counting as the buffer is edited", () => {
    let state = stateWith(BASE, changeGutter(BASE, () => undefined));
    expect(changeCount(state)).toBe(0);
    // Modify line 2 and append a line: two separate changes.
    state = state.update({ changes: { from: 4, to: 7, insert: "TWO" } }).state;
    expect(changeCount(state)).toBe(1);
    state = state.update({ changes: { from: state.doc.length, insert: "five\n" } }).state;
    expect(changeCount(state)).toBe(2);
    // Undoing the first edit by hand leaves only the appended line.
    state = state.update({ changes: { from: 4, to: 7, insert: "two" } }).state;
    expect(changeCount(state)).toBe(1);
  });

  it("sees deleted lines as a change", () => {
    const state = stateWith("one\nfour\n", changeGutter(BASE, () => undefined));
    expect(changeCount(state)).toBe(1);
  });
});

describe("inline blame", () => {
  const blame: GitBlame = {
    commits: [
      { sha: "a".repeat(40), author: "Ada", email: "ada@x", time: 1, summary: "first", uncommitted: false },
      { sha: "b".repeat(40), author: "Bo", email: "bo@x", time: 2, summary: "second", uncommitted: false },
    ],
    lines: [0, 1, 0, 1],
  };

  it("names the commit of the cursor's line, and follows lines as text is inserted above", () => {
    let state = stateWith(BASE, inlineBlame(blame, () => undefined));
    state = state.update({ selection: { anchor: 5 } }).state; // line 2
    expect(commitAtCursor(state, blame)?.author).toBe("Bo");
    // A new first line pushes "two" down; it keeps its commit, the new line is the user's.
    state = state.update({ changes: { from: 0, insert: "zero\n" }, selection: { anchor: 0 } }).state;
    expect(commitAtCursor(state, blame)?.uncommitted).toBe(true);
    state = state.update({ selection: { anchor: state.doc.line(3).from } }).state;
    expect(commitAtCursor(state, blame)?.author).toBe("Bo");
  });

  it("shows nothing for a file git has no history of", () => {
    const state = stateWith(BASE, inlineBlame({ commits: [], lines: [] }, () => undefined));
    expect(commitAtCursor(state, { commits: [], lines: [] })).toBeNull();
  });
});
