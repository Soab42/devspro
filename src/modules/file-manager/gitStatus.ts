import type { GitCode, GitStatus } from "./api";
import { isRootPath, parentPath } from "./utils";

/** What a row shows for git: the file's letter, or for a folder the most pressing change inside it. */
export interface GitMark {
  code: GitCode;
  /** A folder holding changes rather than a changed file: drawn as a dot. */
  inside: boolean;
}

export type GitMarkFor = (path: string, isDir: boolean) => GitMark | null;

/* Which change colours a folder when it holds several kinds, VS Code's order. */
const RANK: Record<GitCode, number> = { C: 6, M: 5, D: 4, R: 3, A: 2, U: 1 };

/**
 * A lookup from a listing's paths to their git marks. An untracked folder
 * comes from git as one entry, so everything under it is untracked too; a
 * folder with changes anywhere below it gets a dot in the colour of the most
 * pressing one, up to the repository root.
 */
export function gitMarks(status: GitStatus | null): GitMarkFor {
  if (!status || status.files.length === 0) return () => null;
  const files = new Map<string, GitCode>();
  const folders = new Map<string, GitCode>();
  const root = isRootPath(status.root) ? status.root : status.root.replace(/[\\/]+$/, "");
  for (const f of status.files) {
    files.set(f.path, f.code);
    for (let dir = parentPath(f.path); dir.length >= root.length && dir !== f.path; dir = parentPath(dir)) {
      const had = folders.get(dir);
      if (!had || RANK[f.code] > RANK[had]) folders.set(dir, f.code);
      if (dir === root || isRootPath(dir)) break;
    }
  }
  return (path, isDir) => {
    const own = files.get(path);
    if (own) return { code: own, inside: false };
    for (let dir = parentPath(path); dir.length >= root.length; dir = parentPath(dir)) {
      if (files.get(dir) === "U") return { code: "U", inside: false };
      if (dir === root || isRootPath(dir)) break;
    }
    const below = isDir ? folders.get(path) : undefined;
    return below ? { code: below, inside: true } : null;
  };
}

/** "M", "U", … as VS Code names them, for tooltips. */
export const GIT_CODE_LABEL: Record<GitCode, string> = {
  M: "Modified",
  A: "Added",
  D: "Deleted",
  R: "Renamed",
  U: "Untracked",
  C: "Conflict",
};
