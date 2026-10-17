import { describe, expect, it } from "vitest";
import type { GitStatus } from "./api";
import { gitMarks } from "./gitStatus";

const status: GitStatus = {
  root: "/r",
  branch: "main",
  upstream: "origin/main",
  ahead: 0,
  behind: 0,
  files: [
    { path: "/r/src/app.ts", code: "M", staged: false },
    { path: "/r/src/new.ts", code: "A", staged: true },
    { path: "/r/notes", code: "U", staged: false },
    { path: "/r/lib/deep/conflict.rs", code: "C", staged: false },
  ],
};

describe("git marks", () => {
  const mark = gitMarks(status);

  it("letters changed files", () => {
    expect(mark("/r/src/app.ts", false)).toEqual({ code: "M", inside: false });
    expect(mark("/r/src/clean.ts", false)).toBeNull();
  });

  it("dots folders in the colour of their most pressing change, up to the root", () => {
    expect(mark("/r/src", true)).toEqual({ code: "M", inside: true });
    expect(mark("/r/lib", true)).toEqual({ code: "C", inside: true });
    expect(mark("/r", true)).toEqual({ code: "C", inside: true });
    expect(mark("/", true)).toBeNull();
  });

  it("marks everything inside an untracked folder as untracked", () => {
    expect(mark("/r/notes", true)).toEqual({ code: "U", inside: false });
    expect(mark("/r/notes/todo.md", false)).toEqual({ code: "U", inside: false });
    expect(mark("/r/notesx", false)).toBeNull();
  });

  it("is silent outside a repository", () => {
    expect(gitMarks(null)("/r/src/app.ts", false)).toBeNull();
  });
});
