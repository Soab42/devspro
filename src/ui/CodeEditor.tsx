import { PointerEvent as ReactPointerEvent, useEffect, useImperativeHandle, useRef, useState } from "react";
import { forwardRef } from "react";
import { EditorState, Compartment, type Extension } from "@codemirror/state";
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  placeholder as placeholderText,
} from "@codemirror/view";
import {
  indentUnit,
  syntaxHighlighting,
  HighlightStyle,
  LanguageDescription,
  StreamLanguage,
} from "@codemirror/language";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
  redo,
  redoDepth,
  undo,
  undoDepth,
} from "@codemirror/commands";
import {
  searchKeymap,
  highlightSelectionMatches,
  openSearchPanel,
  gotoLine,
  search,
} from "@codemirror/search";
import { autocompletion, closeBrackets, completionKeymap } from "@codemirror/autocomplete";
import { tags } from "@lezer/highlight";
import { yaml } from "@codemirror/lang-yaml";
import { MySQL, PostgreSQL, sql } from "@codemirror/lang-sql";
import { searchMatchCount } from "./searchMatchCount";
import "./CodeEditor.css";

/**
 * A real code editor for the one place a plain `<textarea>` genuinely isn't
 * enough: editing a config file whose meaning is carried by indentation.
 *
 * Built on CodeMirror 6 rather than hand-rolled, for the things a textarea has
 * no answer to — Tab that indents instead of leaving the field, Enter that keeps
 * the current indent, line numbers to match the error messages `docker compose
 * config` reports, and undo history scoped to the editor.
 *
 * Assembled from CodeMirror's individual extensions rather than its `basicSetup`
 * bundle: that bundle brings autocomplete, code folding, bracket matching and a
 * lint gutter, none of which have anything to offer a 30-line YAML file, and all
 * of which ship their own un-themed CSS.
 */
/**
 * Tables and their columns, for SQL completion. Passing it turns the editor
 * from "highlights keywords" into something that knows this database — which
 * is the difference between a syntax-coloured textarea and an editor.
 */
export type SqlSchema = Record<string, string[]>;

export interface CodeEditorProps {
  value: string;
  onChange: (value: string) => void;
  language?: "yaml" | "sql" | "env";
  /**
   * Highlight by the file's name — `main.rs`, `Dockerfile`, `.env.local` —
   * when no `language` is given. The grammar loads on demand, so a page that
   * never opens a Rust file never downloads the Rust parser.
   */
  fileName?: string;
  /** SQL only: the dialect to highlight and complete against. */
  dialect?: "mysql" | "postgres";
  /** SQL only: tables and columns offered as completions. */
  schema?: SqlSchema;
  readOnly?: boolean;
  /** Shown while the document is empty. */
  placeholder?: string;
  /** Visible height in rows; the editor scrolls internally beyond it. */
  rows?: number;
  /** Fill the parent's height instead of sizing by rows (a full-page editor). */
  fill?: boolean;
  id?: string;
  ariaLabel?: string;
  /** Cmd/Ctrl+Enter — "run this", wherever the editor is embedded. */
  onSubmit?: () => void;
  /** Fires as the user scrolls the document, for keeping two editors in step. */
  onScroll?: (pos: ScrollPos) => void;
  /** Whether undo/redo would do anything — fires only when that changes. */
  onHistoryChange?: (history: { canUndo: boolean; canRedo: boolean }) => void;
  /** Current selection, so a caller can run just the highlighted statement. */
  onSelectionChange?: (selected: string) => void;
}

/** Smallest height the drag handle allows — a few lines, never a sliver. */
const MIN_HEIGHT_PX = 96;

export interface ScrollPos {
  top: number;
  left: number;
}

export interface CodeEditorHandle {
  /** Selects the first line matching `text` exactly (trimmed) and scrolls to it. */
  revealLine: (text: string) => void;
  focus: () => void;
  /** Opens the find/replace panel — the same one Ctrl/Cmd+F opens. */
  openSearch: () => void;
  undo: () => void;
  redo: () => void;
  /** Scrolls without firing `onScroll`, so two synced editors don't ping-pong. */
  scrollTo: (pos: ScrollPos) => void;
  /** Opens the "go to line" prompt — the same one Ctrl/Cmd+Alt+G opens. */
  openGotoLine: () => void;
  /**
   * Selects line `n` (1-based, clamped) and scrolls it into view, bringing the
   * editor itself on screen too. `focus: false` leaves the keyboard where it is,
   * for moving a second editor in step with the one being focused.
   */
  goToLine: (n: number, options?: { focus?: boolean }) => void;
}

/* Only the tags YAML actually produces. Mapped to tokens so both themes and any
   future re-theme are a CSS change, not a rebuild of this table. */
const highlightStyle = HighlightStyle.define([
  { tag: tags.comment, color: "var(--color-syntax-comment)", fontStyle: "italic" },
  // In YAML a "property name" is a mapping key — the structure of the file, so
  // it gets the weight as well as the colour.
  { tag: tags.propertyName, color: "var(--color-syntax-key)", fontWeight: "500" },
  { tag: tags.string, color: "var(--color-syntax-string)" },
  { tag: tags.number, color: "var(--color-syntax-number)" },
  { tag: [tags.bool, tags.null, tags.atom], color: "var(--color-syntax-atom)" },
  { tag: [tags.punctuation, tags.separator], color: "var(--color-syntax-punct)" },
  { tag: tags.invalid, color: "var(--color-syntax-invalid)" },
  // SQL's own tags. Keywords carry the weight here for the same reason mapping
  // keys do in YAML: they are the structure of the statement.
  { tag: tags.keyword, color: "var(--color-syntax-key)", fontWeight: "500" },
  { tag: [tags.typeName, tags.standard(tags.name)], color: "var(--color-syntax-atom)" },
  { tag: tags.operator, color: "var(--color-syntax-punct)" },
  { tag: tags.variableName, color: "var(--color-text)" },
  // What source files in general add: markup, functions, classes, prose.
  { tag: [tags.tagName, tags.heading], color: "var(--color-syntax-key)", fontWeight: "500" },
  { tag: [tags.attributeName, tags.className, tags.self], color: "var(--color-syntax-atom)" },
  { tag: [tags.regexp, tags.escape, tags.link, tags.url], color: "var(--color-syntax-string)" },
  { tag: [tags.meta, tags.processingInstruction], color: "var(--color-syntax-comment)" },
  { tag: tags.strong, fontWeight: "600" },
  { tag: tags.emphasis, fontStyle: "italic" },
]);

/* Layout and chrome only — every colour comes from a token, so light/dark is
   handled by the same variables as the rest of the app and there is no second
   theme to keep in sync. */
const editorTheme = EditorView.theme({
  "&": {
    fontSize: "0.8125rem",
    backgroundColor: "var(--color-surface)",
    color: "var(--color-text)",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily: "var(--font-mono)",
    lineHeight: "1.6",
    // Ligatures turn `->` in a port mapping into a glyph that isn't what's in
    // the file. Same reasoning as `Mono`.
    fontFeatureSettings: '"calt" 0',
  },
  ".cm-content": { padding: "var(--space-2) 0" },
  ".cm-gutters": {
    backgroundColor: "var(--color-surface-2)",
    color: "var(--color-text-muted)",
    borderRight: "1px solid var(--color-border-subtle)",
  },
  ".cm-activeLine": { backgroundColor: "var(--color-hover)" },
  ".cm-activeLineGutter": {
    backgroundColor: "var(--color-surface-3)",
    color: "var(--color-text-secondary)",
  },
  ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection": {
    backgroundColor: "var(--color-accent-subtle)",
  },
  ".cm-cursor": { borderLeftColor: "var(--color-text)" },
  ".cm-selectionMatch": { backgroundColor: "var(--color-surface-3)" },
  // The find/replace panel ships with its own light-only colours.
  ".cm-panels": {
    backgroundColor: "var(--color-surface-2)",
    color: "var(--color-text)",
  },
  ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--color-border)" },
  ".cm-panels.cm-panels-bottom": { borderTop: "1px solid var(--color-border)" },
  ".cm-search": { fontSize: "var(--text-sm)", padding: "var(--space-2)" },
  ".cm-search label": { color: "var(--color-text-secondary)" },
  ".cm-search input[type=checkbox]": {
    appearance: "auto",
    accentColor: "var(--color-accent)",
    verticalAlign: "middle",
  },
  ".cm-textfield": {
    backgroundColor: "var(--color-surface)",
    color: "var(--color-text)",
    border: "1px solid var(--color-border)",
    borderRadius: "var(--radius-sm)",
    padding: "0.15rem var(--space-2)",
  },
  ".cm-textfield:focus": { outline: "none", borderColor: "var(--color-accent)" },
  ".cm-button": {
    backgroundImage: "none",
    backgroundColor: "var(--color-surface)",
    color: "var(--color-text)",
    border: "1px solid var(--color-border)",
    borderRadius: "var(--radius-sm)",
  },
  ".cm-button:hover": { backgroundColor: "var(--color-hover)" },
  ".cm-panel.cm-search [name=close]": { color: "var(--color-text-muted)" },
  ".cm-searchMatch": {
    backgroundColor: "color-mix(in srgb, var(--color-warning) 30%, transparent)",
  },
  ".cm-searchMatch.cm-searchMatch-selected": {
    backgroundColor: "color-mix(in srgb, var(--color-warning) 60%, transparent)",
  },
});

/* A `.env` file: `[export ]KEY=value  # comment`. Hand-written because there is
   no CodeMirror package for it, and a line-at-a-time stream parser is all the
   format needs. Follows dotenv's rule that an inline `#` is a comment only when
   the value is unquoted and whitespace precedes it. */
type EnvPhase = "start" | "key" | "value" | "tail";
const envLanguage = StreamLanguage.define<{ phase: EnvPhase }>({
  name: "env",
  startState: () => ({ phase: "start" }),
  token(stream, state) {
    if (stream.sol()) state.phase = "start";
    if (stream.eatSpace()) return null;

    switch (state.phase) {
      case "start":
        if (stream.peek() === "#") {
          stream.skipToEnd();
          return "comment";
        }
        if (stream.match(/^export(?=\s)/)) return "keyword";
        if (stream.match(/^[^=\s#]+/)) {
          state.phase = "key";
          return "propertyName";
        }
        break;
      case "key":
        if (stream.eat("=")) {
          state.phase = "value";
          return "punctuation";
        }
        break;
      case "value": {
        state.phase = "tail";
        const quote = stream.peek();
        if (quote === '"' || quote === "'" || quote === "`") {
          stream.next();
          let ch;
          while ((ch = stream.next()) !== undefined) {
            if (ch === "\\" && quote === '"') stream.next();
            else if (ch === quote) break;
          }
          return "string";
        }
        if (stream.match(/^.*?(?=\s+#|$)/) && stream.current()) return "string";
        break;
      }
      case "tail":
        if (stream.peek() === "#") {
          stream.skipToEnd();
          return "comment";
        }
        break;
    }
    stream.skipToEnd();
    return state.phase === "start" ? "invalid" : null;
  },
  languageData: { commentTokens: { line: "#" } },
});

/** The grammar for a file name, from CodeMirror's language catalogue. */
async function languageForFile(name: string): Promise<Extension> {
  if (name === ".env" || name.startsWith(".env.")) return envLanguage;
  const { languages } = await import("@codemirror/language-data");
  const found = LanguageDescription.matchFilename(languages, name);
  return found ? found.load() : [];
}

function languageExtension(
  language: "yaml" | "sql" | "env",
  dialect: "mysql" | "postgres",
  schema: SqlSchema | undefined,
) {
  if (language === "env") return envLanguage;
  if (language !== "sql") return yaml();
  return sql({
    dialect: dialect === "postgres" ? PostgreSQL : MySQL,
    schema,
    upperCaseKeywords: true,
  });
}

export const CodeEditor = forwardRef<CodeEditorHandle, CodeEditorProps>(function CodeEditor(
  {
    value,
    onChange,
    language: languageProp,
    fileName,
    dialect = "mysql",
    schema,
    readOnly = false,
    placeholder,
    rows = 20,
    fill = false,
    id,
    ariaLabel,
    onSubmit,
    onSelectionChange,
    onHistoryChange,
    onScroll,
  },
  ref,
) {
  // A file name picks its own grammar; without one the editor stays the YAML
  // editor it was built as.
  const language = languageProp ?? (fileName ? null : "yaml");
  const host = useRef<HTMLDivElement>(null);
  // Pixel height once the user has dragged the bottom edge; until then `rows`.
  const [dragHeight, setDragHeight] = useState<number | null>(null);
  const view = useRef<EditorView | null>(null);
  // Kept in a ref so the editor is created once: rebuilding it on every render
  // would drop selection, scroll position and undo history on each keystroke.
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onSubmitRef = useRef(onSubmit);
  onSubmitRef.current = onSubmit;
  const onSelectionRef = useRef(onSelectionChange);
  onSelectionRef.current = onSelectionChange;
  const onHistoryRef = useRef(onHistoryChange);
  onHistoryRef.current = onHistoryChange;
  const lastHistory = useRef("");
  const onScrollRef = useRef(onScroll);
  onScrollRef.current = onScroll;
  // Where `scrollTo` last put the scroller; the scroll event that answers it is
  // ours, not the user's, and must not be reported back.
  const programmatic = useRef<ScrollPos | null>(null);
  const readOnlyComp = useRef(new Compartment());
  // The schema arrives after the table list loads, and can change with the
  // database, so completion is reconfigured rather than baked in at creation.
  const languageComp = useRef(new Compartment());

  useEffect(() => {
    if (!host.current) return;

    const editor = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          highlightActiveLine(),
          ...(placeholder ? [placeholderText(placeholder)] : []),
          highlightSelectionMatches(),
          // Top, next to the toolbar that opens it, not under a tall document.
          search({ top: true }),
          searchMatchCount,
          history(),
          closeBrackets(),
          languageComp.current.of(language ? languageExtension(language, dialect, schema) : []),
          syntaxHighlighting(highlightStyle),
          ...(language === "sql" ? [autocompletion()] : []),
          // YAML's own convention, and what compose files are written in.
          indentUnit.of("  "),
          EditorState.tabSize.of(2),
          // `indentWithTab` must precede defaultKeymap: without it Tab moves
          // focus out of the editor, which in a modal means landing on Cancel.
          // The submit binding precedes both so Mod-Enter is not swallowed by
          // defaultKeymap's "insert newline and indent".
          keymap.of([
            {
              key: "Mod-Enter",
              run: () => {
                if (!onSubmitRef.current) return false;
                onSubmitRef.current();
                return true;
              },
            },
            indentWithTab,
            ...completionKeymap,
            ...defaultKeymap,
            ...historyKeymap,
            ...searchKeymap,
          ]),
          editorTheme,
          readOnlyComp.current.of(EditorState.readOnly.of(readOnly)),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) onChangeRef.current(update.state.doc.toString());
            if (update.transactions.length > 0) {
              const canUndo = undoDepth(update.state) > 0;
              const canRedo = redoDepth(update.state) > 0;
              const key = `${canUndo}${canRedo}`;
              if (key !== lastHistory.current) {
                lastHistory.current = key;
                onHistoryRef.current?.({ canUndo, canRedo });
              }
            }
            if (update.docChanged || update.selectionSet) {
              const { from, to } = update.state.selection.main;
              onSelectionRef.current?.(from === to ? "" : update.state.sliceDoc(from, to));
            }
          }),
          EditorView.contentAttributes.of({
            "aria-label": ariaLabel ?? "Code editor",
            ...(id ? { id } : {}),
          }),
        ],
      }),
    });
    view.current = editor;
    const scroller = editor.scrollDOM;
    const reportScroll = () => {
      const pos = { top: scroller.scrollTop, left: scroller.scrollLeft };
      const echo = programmatic.current;
      programmatic.current = null;
      if (echo && Math.abs(echo.top - pos.top) < 1 && Math.abs(echo.left - pos.left) < 1) return;
      onScrollRef.current?.(pos);
    };
    scroller.addEventListener("scroll", reportScroll, { passive: true });
    return () => {
      scroller.removeEventListener("scroll", reportScroll);
      editor.destroy();
      view.current = null;
    };
    // Created once for the editor's lifetime; `value` is only the initial doc,
    // and later changes are reconciled by the effect below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /* Controlled-component reconciliation. The guard is what stops a feedback
     loop: every keystroke calls `onChange`, the parent re-renders with the new
     value, and dispatching that identical value back would reset the cursor to
     the end of the document on every character typed. */
  useEffect(() => {
    const editor = view.current;
    if (!editor || value === editor.state.doc.toString()) return;
    editor.dispatch({
      changes: { from: 0, to: editor.state.doc.length, insert: value },
    });
  }, [value]);

  useEffect(() => {
    view.current?.dispatch({
      effects: readOnlyComp.current.reconfigure(EditorState.readOnly.of(readOnly)),
    });
  }, [readOnly]);

  // Serialized because a Compartment reconfigure needs a stable dependency and
  // `schema` is a fresh object on every render of the caller.
  const schemaKey = JSON.stringify(schema ?? null);
  useEffect(() => {
    if (!language) return;
    view.current?.dispatch({
      effects: languageComp.current.reconfigure(
        languageExtension(language, dialect, schemaKey === "null" ? undefined : JSON.parse(schemaKey)),
      ),
    });
  }, [language, dialect, schemaKey]);

  useEffect(() => {
    if (language || !fileName) return;
    let cancelled = false;
    languageForFile(fileName)
      .then((ext) => {
        if (!cancelled) view.current?.dispatch({ effects: languageComp.current.reconfigure(ext) });
      })
      // No grammar is a plain-text editor, which is still an editor.
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [language, fileName]);

  useImperativeHandle(ref, () => ({
    focus: () => view.current?.focus(),
    openSearch: () => {
      if (view.current) openSearchPanel(view.current);
    },
    undo: () => {
      if (view.current) undo(view.current);
    },
    redo: () => {
      if (view.current) redo(view.current);
    },
    scrollTo: ({ top, left }) => {
      const scroller = view.current?.scrollDOM;
      if (!scroller) return;
      // Clamp first, so the echo check compares against where it will really land.
      const target = {
        top: Math.min(top, scroller.scrollHeight - scroller.clientHeight),
        left: Math.min(left, scroller.scrollWidth - scroller.clientWidth),
      };
      if (
        Math.abs(scroller.scrollTop - target.top) < 1 &&
        Math.abs(scroller.scrollLeft - target.left) < 1
      )
        return;
      programmatic.current = target;
      scroller.scrollTop = target.top;
      scroller.scrollLeft = target.left;
    },
    openGotoLine: () => {
      if (view.current) gotoLine(view.current);
    },
    goToLine: (n: number, { focus = true } = {}) => {
      const editor = view.current;
      if (!editor) return;
      const doc = editor.state.doc;
      const line = doc.line(Math.min(Math.max(1, Math.round(n)), doc.lines));
      editor.dispatch({
        selection: { anchor: line.from, head: line.to },
        effects: EditorView.scrollIntoView(line.from, { y: "center" }),
      });
      // CodeMirror focuses without scrolling the page, so a caller further down
      // it (a table row, an issue link) would otherwise move an editor off screen.
      editor.dom.scrollIntoView?.({ block: "nearest" });
      if (focus) editor.focus();
    },
    revealLine: (text: string) => {
      const editor = view.current;
      if (!editor) return;
      const target = text.trim();
      for (let n = 1; n <= editor.state.doc.lines; n++) {
        const line = editor.state.doc.line(n);
        if (line.text.trim() !== target) continue;
        editor.dispatch({
          selection: { anchor: line.from, head: line.to },
          // "center" rather than the default "nearest": a match near the bottom
          // of a long file would otherwise sit on the last visible row with no
          // context under it.
          effects: EditorView.scrollIntoView(line.from, { y: "center" }),
        });
        editor.focus();
        return;
      }
    },
  }));

  if (fill) return <div className="code-editor code-editor-fill" ref={host} />;

  /* A hand-rolled handle rather than CSS `resize`: WebKitGTK doesn't offer it on
     a div, and CodeMirror's scroller would cover the corner grip if it did. */
  function startResize(e: ReactPointerEvent<HTMLDivElement>) {
    const box = host.current;
    if (!box || e.button !== 0) return;
    e.preventDefault();
    const handle = e.currentTarget;
    handle.setPointerCapture(e.pointerId);
    const startY = e.clientY;
    const startHeight = box.getBoundingClientRect().height;
    const move = (ev: PointerEvent) =>
      setDragHeight(Math.max(MIN_HEIGHT_PX, startHeight + ev.clientY - startY));
    const stop = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", stop);
      handle.removeEventListener("pointercancel", stop);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", stop);
    handle.addEventListener("pointercancel", stop);
  }

  return (
    <div className="code-editor-resizable">
      <div
        className="code-editor"
        /* Sized in rows to match the `rows` prop on the TextArea this replaces,
           computed from the same line-height the theme sets above. */
        style={{
          height:
            dragHeight !== null
              ? `${dragHeight}px`
              : `calc(${rows} * 1.6 * 0.8125rem + 2 * var(--space-2))`,
        }}
        ref={host}
      />
      <div
        className="code-editor-grip"
        aria-hidden
        title="Drag to resize"
        onPointerDown={startResize}
        onDoubleClick={() => setDragHeight(null)}
      />
    </div>
  );
});
