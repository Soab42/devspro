import { Chunk, getChunks, unifiedMergeView } from "@codemirror/merge";
import { EditorState, Extension, Prec, RangeSet, RangeSetBuilder, RangeValue, StateField, Text } from "@codemirror/state";
import { Decoration, EditorView, GutterMarker, WidgetType, gutter, keymap } from "@codemirror/view";
import type { GitBlame, GitCommit } from "./api";
import { relativeTime } from "./utils";

/**
 * Git in the file editor, the way VS Code and GitLens show it:
 *
 * - `changeGutter`: a bar beside every line that differs from the staged
 *   copy (green added, blue changed, a red notch where lines were deleted),
 *   kept current as you type. A click on a bar opens the inline view.
 * - `changesView`: the same changes inline, deleted lines shown in place,
 *   with Keep and Revert on each change.
 * - `inlineBlame`: who last changed the line the cursor is on, when, and
 *   why, faint at the end of the line. A click opens that commit.
 */

const DIFF = { scanLimit: 500 };

const toText = (s: string) => Text.of(s.split(/\r\n?|\n/));

type Kind = "added" | "modified" | "deleted";

class ChangeMarker extends GutterMarker {
  constructor(readonly kind: Kind) {
    super();
  }
  eq(other: GutterMarker) {
    return other instanceof ChangeMarker && other.kind === this.kind;
  }
  toDOM() {
    const el = document.createElement("div");
    el.className = `cm-git-mark cm-git-${this.kind}`;
    return el;
  }
}

const MARKS: Record<Kind, ChangeMarker> = {
  added: new ChangeMarker("added"),
  modified: new ChangeMarker("modified"),
  deleted: new ChangeMarker("deleted"),
};

/** One marker per changed line of the editor's document. */
function markersFor(doc: Text, chunks: readonly Chunk[]): RangeSet<GutterMarker> {
  const out = new RangeSetBuilder<GutterMarker>();
  let lastLine = -1;
  const add = (pos: number, kind: Kind) => {
    const line = doc.lineAt(Math.min(pos, doc.length));
    if (line.number <= lastLine) return;
    lastLine = line.number;
    out.add(line.from, line.from, MARKS[kind]);
  };
  for (const c of chunks) {
    if (c.fromB === c.toB) {
      add(c.fromB, "deleted");
      continue;
    }
    const kind = c.fromA === c.toA ? "added" : "modified";
    const end = Math.min(c.endB, doc.length);
    for (let pos = c.fromB; pos <= end; pos = doc.lineAt(pos).to + 1) add(pos, kind);
  }
  return out.finish();
}

interface Changes {
  base: Text;
  chunks: readonly Chunk[];
  marks: RangeSet<GutterMarker>;
}

const changesField = StateField.define<Changes | null>({
  create: () => null,
  update(value, tr) {
    if (!value || !tr.docChanged) return value;
    // Incremental: only the edited region is diffed again.
    const chunks = Chunk.updateB(value.chunks, value.base, tr.state.doc, tr.changes, DIFF);
    return { base: value.base, chunks, marks: markersFor(tr.state.doc, chunks) };
  },
});

/** The changed chunks, whether the bars or the inline view is showing them. */
function chunksOf(state: EditorState): readonly Chunk[] {
  return state.field(changesField, false)?.chunks ?? getChunks(state)?.chunks ?? [];
}

export function changeCount(state: EditorState): number {
  return chunksOf(state).length;
}

/** Moves to the next (or previous) change after the cursor's line, wrapping round. */
export function goToChange(view: EditorView, dir: 1 | -1): boolean {
  const chunks = chunksOf(view.state);
  if (chunks.length === 0) return false;
  const { doc } = view.state;
  const line = doc.lineAt(view.state.selection.main.head);
  const target =
    dir > 0
      ? (chunks.find((c) => c.fromB > line.to) ?? chunks[0])
      : ([...chunks].reverse().find((c) => Math.min(c.endB, doc.length) < line.from) ?? chunks[chunks.length - 1]);
  const pos = Math.min(target.fromB, doc.length);
  view.dispatch({ selection: { anchor: pos }, effects: EditorView.scrollIntoView(pos, { y: "center" }) });
  view.focus();
  return true;
}

/* VS Code's keys for stepping through changes. */
const changeKeys = Prec.high(
  keymap.of([
    { key: "Alt-F5", run: (v) => goToChange(v, 1) },
    { key: "Shift-Alt-F5", run: (v) => goToChange(v, -1) },
  ]),
);

const gitTheme = EditorView.theme({
  ".cm-git-gutter .cm-gutterElement": { padding: "0 2px 0 1px" },
  ".cm-git-mark": { width: "3px", height: "100%" },
  ".cm-git-added": { backgroundColor: "var(--color-success)" },
  ".cm-git-modified": { backgroundColor: "var(--color-info)" },
  ".cm-git-deleted": {
    width: "0",
    height: "0",
    borderTop: "4px solid transparent",
    borderBottom: "4px solid transparent",
    borderLeft: "5px solid var(--color-danger)",
    transform: "translateY(-4px)",
  },
  ".cm-git-gutter .cm-gutterElement:has(.cm-git-mark)": { cursor: "pointer" },
  ".cm-blame": {
    marginLeft: "3em",
    color: "var(--color-text-muted)",
    fontStyle: "italic",
    opacity: "0.8",
    cursor: "pointer",
    whiteSpace: "pre",
  },
  ".cm-blame-uncommitted": { cursor: "default" },
});

/**
 * Change bars against `base` — the file as staged in git. `onOpen` runs on a
 * click on a bar (the page opens the inline view there).
 */
export function changeGutter(base: string, onOpen: () => void): Extension {
  const baseText = toText(base);
  return [
    changesField.init((state) => {
      const chunks = Chunk.build(baseText, state.doc, DIFF);
      return { base: baseText, chunks, marks: markersFor(state.doc, chunks) };
    }),
    gutter({
      class: "cm-git-gutter",
      markers: (view) => view.state.field(changesField)?.marks ?? RangeSet.empty,
      domEventHandlers: {
        mousedown: (view, line) => {
          const marks = view.state.field(changesField)?.marks;
          let hit = false;
          marks?.between(line.from, line.from, () => {
            hit = true;
            return false;
          });
          if (hit) onOpen();
          return hit;
        },
      },
    }),
    changeKeys,
    gitTheme,
  ];
}

/** The changes inline, each with Keep (stage nothing, just hide it) and Revert. */
export function changesView(base: string): Extension {
  return [
    unifiedMergeView({ original: base, mergeControls: true, highlightChanges: true, gutter: true, syntaxHighlightDeletions: true }),
    EditorState.phrases.of({ Accept: "Keep", Reject: "Revert" }),
    changeKeys,
    gitTheme,
  ];
}

/* ---- inline blame --------------------------------------------------------- */

/**
 * Which commit a line comes from, placed at the line's start. It travels with
 * the text after it, so a line pushed down by Enter keeps its commit and the
 * new empty line above it reads as the user's.
 */
class LineCommit extends RangeValue {
  constructor(readonly commit: number) {
    super();
    // A zero-width mark needs both sides equal, or range queries skip it.
    this.startSide = 1;
    this.endSide = 1;
    this.point = true;
  }
  eq(other: RangeValue) {
    return other instanceof LineCommit && other.commit === this.commit;
  }
}

const YOU: GitCommit = { sha: "", author: "You", email: "", time: 0, summary: "Uncommitted changes", uncommitted: true };

class BlameWidget extends WidgetType {
  constructor(
    readonly commit: GitCommit,
    readonly onOpen: (commit: GitCommit) => void,
  ) {
    super();
  }
  eq(other: WidgetType) {
    return other instanceof BlameWidget && other.commit.sha === this.commit.sha && other.commit.uncommitted === this.commit.uncommitted;
  }
  toDOM() {
    const c = this.commit;
    const el = document.createElement("span");
    el.className = c.uncommitted ? "cm-blame cm-blame-uncommitted" : "cm-blame";
    el.textContent = c.uncommitted ? "You · Uncommitted changes" : `${c.author}, ${relativeTime(c.time)} · ${c.summary}`;
    if (!c.uncommitted) {
      el.title = `${c.summary}\n\n${c.author} <${c.email}>\n${new Date(c.time * 1000).toLocaleString()}\n${c.sha}\n\nClick to see this commit`;
      el.addEventListener("mousedown", (e) => {
        e.preventDefault();
        this.onOpen(c);
      });
    }
    return el;
  }
  ignoreEvent() {
    return false;
  }
}

/** The commit the cursor's line was last changed in; null when there is no blame. */
export function commitAtCursor(state: EditorState, blame: GitBlame): GitCommit | null {
  const field = state.field(blameField, false);
  if (!field || blame.commits.length === 0) return null;
  const line = state.doc.lineAt(state.selection.main.head);
  // Typing at the start of a line moves its marker along the line; the first
  // marker anywhere on it is still that line's.
  let commit: GitCommit = YOU;
  field.between(line.from, line.to, (_from, _to, value) => {
    commit = blame.commits[value.commit] ?? YOU;
    return false;
  });
  return commit;
}

const blameField = StateField.define<RangeSet<LineCommit>>({
  create: () => RangeSet.empty,
  update: (value, tr) => (tr.docChanged ? value.map(tr.changes) : value),
});

/**
 * GitLens' current-line blame. `blame.lines` was worked out for the buffer as
 * it was when asked; edits since then keep their line's old answer until the
 * next blame, except new lines, which are the user's own.
 */
export function inlineBlame(blame: GitBlame, onOpen: (commit: GitCommit) => void): Extension {
  if (blame.commits.length === 0) return [];
  return [
    blameField.init((state) => {
      const out = new RangeSetBuilder<LineCommit>();
      const n = Math.min(blame.lines.length, state.doc.lines);
      for (let i = 0; i < n; i++) {
        const from = state.doc.line(i + 1).from;
        out.add(from, from, new LineCommit(blame.lines[i]));
      }
      return out.finish();
    }),
    EditorView.decorations.compute([blameField, "selection"], (state) => {
      const commit = commitAtCursor(state, blame);
      if (!commit) return Decoration.none;
      const line = state.doc.lineAt(state.selection.main.head);
      return Decoration.set([Decoration.widget({ widget: new BlameWidget(commit, onOpen), side: 1 }).range(line.to)]);
    }),
    gitTheme,
  ];
}
