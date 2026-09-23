import { useEffect, useImperativeHandle, useRef } from "react";
import { forwardRef } from "react";
import { EditorState, Compartment } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine } from "@codemirror/view";
import { indentUnit, syntaxHighlighting, HighlightStyle } from "@codemirror/language";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { autocompletion, closeBrackets, completionKeymap } from "@codemirror/autocomplete";
import { tags } from "@lezer/highlight";
import { yaml } from "@codemirror/lang-yaml";
import { MySQL, PostgreSQL, sql } from "@codemirror/lang-sql";
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
  language?: "yaml" | "sql";
  /** SQL only: the dialect to highlight and complete against. */
  dialect?: "mysql" | "postgres";
  /** SQL only: tables and columns offered as completions. */
  schema?: SqlSchema;
  readOnly?: boolean;
  /** Visible height in rows; the editor scrolls internally beyond it. */
  rows?: number;
  /** Fill the parent's height instead of sizing by rows (a full-page editor). */
  fill?: boolean;
  id?: string;
  ariaLabel?: string;
  /** Cmd/Ctrl+Enter — "run this", wherever the editor is embedded. */
  onSubmit?: () => void;
  /** Current selection, so a caller can run just the highlighted statement. */
  onSelectionChange?: (selected: string) => void;
}

export interface CodeEditorHandle {
  /** Selects the first line matching `text` exactly (trimmed) and scrolls to it. */
  revealLine: (text: string) => void;
  focus: () => void;
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
});

function languageExtension(
  language: "yaml" | "sql",
  dialect: "mysql" | "postgres",
  schema: SqlSchema | undefined,
) {
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
    language = "yaml",
    dialect = "mysql",
    schema,
    readOnly = false,
    rows = 20,
    fill = false,
    id,
    ariaLabel,
    onSubmit,
    onSelectionChange,
  },
  ref,
) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  // Kept in a ref so the editor is created once: rebuilding it on every render
  // would drop selection, scroll position and undo history on each keystroke.
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onSubmitRef = useRef(onSubmit);
  onSubmitRef.current = onSubmit;
  const onSelectionRef = useRef(onSelectionChange);
  onSelectionRef.current = onSelectionChange;
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
          highlightSelectionMatches(),
          history(),
          closeBrackets(),
          languageComp.current.of(languageExtension(language, dialect, schema)),
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
    return () => {
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
    view.current?.dispatch({
      effects: languageComp.current.reconfigure(
        languageExtension(language, dialect, schemaKey === "null" ? undefined : JSON.parse(schemaKey)),
      ),
    });
  }, [language, dialect, schemaKey]);

  useImperativeHandle(ref, () => ({
    focus: () => view.current?.focus(),
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

  return (
    <div
      className={fill ? "code-editor code-editor-fill" : "code-editor"}
      /* Sized in rows to match the `rows` prop on the TextArea this replaces,
         computed from the same line-height the theme sets above. */
      style={fill ? undefined : { height: `calc(${rows} * 1.6 * 0.8125rem + 2 * var(--space-2))` }}
      ref={host}
    />
  );
});
