import { KeyboardEvent, MouseEvent, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { diff } from "@codemirror/merge";
import { foldAll, foldCode, unfoldAll, unfoldCode } from "@codemirror/language";
import { toggleComment } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { Button, Callout, IconButton, MenuItem, Spinner } from "../../../ui";
/* Not from the ui barrel, on purpose — see the note there: importing it through
   the barrel would drag CodeMirror into every page's bundle. */
import { CodeEditor, CodeEditorHandle } from "../../../ui/CodeEditor";
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronsDownUp,
  ChevronsUpDown,
  ClipboardPaste,
  Copy,
  FoldVertical,
  GitCommitHorizontal,
  GitCompare,
  History,
  MessageSquareCode,
  Save,
  Scissors,
  Search,
  SquareArrowOutUpRight,
  UnfoldVertical,
  WandSparkles,
} from "../../../ui/icons";
import { GitBlame, GitCommit, fmFormat, fmGitBase, fmGitBlame, fmReadFile, fmWriteFile } from "../api";
import { changeCount, changeGutter, changesView, commitAtCursor, goToChange, inlineBlame } from "../editorGit";
import { drafts } from "../openFiles";
import { formatWithBundledPrettier } from "../prettierFallback";
import { errorMessage } from "../utils";
import ContextMenu, { ContextMenuState } from "./ContextMenu";
import FileTabBar from "./FileTabBar";
import GitHistory from "./GitHistory";

/* Editor preferences shared by every open file and remembered per viewer. */
const FORMAT_ON_SAVE = "fm.format-on-save";
const LINE_BLAME = "fm.line-blame";
const flagListeners = new Set<() => void>();

function readFlag(key: string, fallback: boolean): boolean {
  try {
    const v = window.localStorage.getItem(key);
    return v === null ? fallback : v === "1";
  } catch {
    return fallback;
  }
}

function useFlag(key: string, fallback: boolean): [boolean, (on: boolean) => void] {
  const value = useSyncExternalStore(
    (l) => {
      flagListeners.add(l);
      return () => flagListeners.delete(l);
    },
    () => readFlag(key, fallback),
  );
  const set = useCallback(
    (on: boolean) => {
      try {
        window.localStorage.setItem(key, on ? "1" : "0");
      } catch {
        // Storage blocked: the choice lasts until the page is reloaded.
      }
      flagListeners.forEach((l) => l());
    },
    [key],
  );
  return [value, set];
}

/** Keeps a blame current while typing, without asking git on every key. */
const BLAME_IDLE_MS = 1500;

/**
 * A text or source file in a tab, VS Code-style: highlighted for its
 * language, with folding, Save on Ctrl/Cmd+S and Format on Shift+Alt+F (by
 * the project's own formatter, else a built-in prettier). Inside a git
 * repository it shows what changed since the staged copy, as bars in the
 * gutter or inline with Revert on each change, who last touched the
 * cursor's line, and the file's history. Right-click has all of it.
 *
 * Unsaved edits are kept in `drafts`, so leaving the page and coming back
 * finds them. Anything that is not text is refused on the Rust side, and the
 * message offers the desktop's app instead.
 */
export default function TextViewer({
  path,
  name,
  shownPath,
  onDirty,
  onSaved,
  onReveal,
  onOpenExternal,
}: {
  path: string;
  name: string;
  shownPath: string;
  onDirty: (dirty: boolean) => void;
  onSaved: (message: string) => void;
  onReveal: () => void;
  onOpenExternal: () => void;
}) {
  const draft = drafts.get(path);
  const [original, setOriginal] = useState<string | null>(draft?.original ?? null);
  const [contents, setContents] = useState(draft?.contents ?? "");
  const [loading, setLoading] = useState(!draft);
  const [saving, setSaving] = useState(false);
  const [formatting, setFormatting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const editor = useRef<CodeEditorHandle>(null);

  // Git: the staged copy the changes are measured against (null outside a
  // repository or for an untracked file), the blame, and how they are shown.
  const [base, setBase] = useState<string | null>(null);
  const [blame, setBlame] = useState<GitBlame | null>(null);
  const [changesOpen, setChangesOpen] = useState(false);
  const [changes, setChanges] = useState(0);
  const [history, setHistory] = useState<{ sha?: string } | null>(null);
  const [formatOnSave, setFormatOnSave] = useFlag(FORMAT_ON_SAVE, false);
  const [lineBlame, setLineBlame] = useFlag(LINE_BLAME, true);

  useEffect(() => {
    if (drafts.has(path)) return;
    let cancelled = false;
    setLoading(true);
    fmReadFile(path)
      .then((text) => {
        if (cancelled) return;
        setOriginal(text);
        setContents(text);
      })
      .catch((e) => !cancelled && setError(errorMessage(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [path]);

  const dirty = original !== null && contents !== original;
  useEffect(() => {
    if (original === null) return;
    if (dirty) drafts.set(path, { original, contents });
    else drafts.delete(path);
  }, [path, original, contents, dirty]);
  // Through a ref: the page hands a fresh callback down on every render.
  const onDirtyRef = useRef(onDirty);
  onDirtyRef.current = onDirty;
  useEffect(() => onDirtyRef.current(dirty), [dirty]);

  function flash(message: string) {
    setNotice(message);
    window.setTimeout(() => setNotice((n) => (n === message ? null : n)), 2500);
  }

  // The staged copy changes when the user commits or stages elsewhere, so it
  // is asked again whenever the window comes back to the front.
  const loadBase = useCallback(() => {
    fmGitBase(path)
      .then(setBase)
      .catch(() => setBase(null));
  }, [path]);
  useEffect(() => {
    loadBase();
    window.addEventListener("focus", loadBase);
    return () => window.removeEventListener("focus", loadBase);
  }, [loadBase]);
  useEffect(() => {
    if (base === null) setChangesOpen(false);
  }, [base]);

  // Blame what is in the editor, unsaved lines included, once typing pauses.
  const tracked = base !== null;
  const blameSeq = useRef(0);
  const hasBlame = blame !== null;
  useEffect(() => {
    if (!lineBlame || !tracked || original === null) return;
    const seq = ++blameSeq.current;
    const timer = window.setTimeout(
      () => {
        fmGitBlame(path, contents)
          .then((b) => seq === blameSeq.current && setBlame(b))
          .catch(() => undefined);
      },
      hasBlame ? BLAME_IDLE_MS : 0,
    );
    return () => window.clearTimeout(timer);
  }, [path, contents, lineBlame, tracked, original, hasBlame]);

  const openCommit = useCallback((c: GitCommit) => setHistory({ sha: c.sha }), []);
  const openChanges = useCallback(() => setChangesOpen(true), []);
  const extensions = useMemo(
    () => [
      base === null ? [] : changesOpen ? changesView(base) : changeGutter(base, openChanges),
      lineBlame && blame && !changesOpen ? inlineBlame(blame, openCommit) : [],
      EditorView.updateListener.of((u) => {
        if (u.docChanged || u.transactions.some((t) => t.reconfigured)) setChanges(changeCount(u.state));
      }),
    ],
    [base, changesOpen, lineBlame, blame, openChanges, openCommit],
  );

  /**
   * Formats the buffer and applies only what changed, as one undoable step,
   * so the cursor, folds and scroll position stay where they were.
   */
  async function format(): Promise<void> {
    const view = editor.current?.getView();
    if (!view || formatting) return;
    const text = view.state.doc.toString();
    setFormatting(true);
    setError(null);
    try {
      const result = await fmFormat(path, text);
      let out = result.contents;
      let tool = result.tool;
      if (out === null) {
        out = await formatWithBundledPrettier(name, text, result.prettier_config);
        tool = "prettier (built in)";
      }
      if (out === null) throw new Error(`there is no formatter for ${name}`);
      // Typed over while the formatter ran: keep the typing.
      if (view.state.doc.toString() !== text) return;
      if (out !== text) {
        const formatted = out;
        const changes = diff(text, formatted, { scanLimit: 10000 }).map((c) => ({ from: c.fromA, to: c.toA, insert: formatted.slice(c.fromB, c.toB) }));
        view.dispatch({ changes, userEvent: "input.format" });
      }
      flash(out === text ? `Already formatted (${tool})` : `Formatted with ${tool}`);
    } catch (e) {
      setError(`Could not format: ${errorMessage(e)}`);
    } finally {
      setFormatting(false);
    }
  }

  async function save() {
    if (saving || original === null) return;
    if (!dirty && !formatOnSave) return;
    setSaving(true);
    setError(null);
    try {
      if (formatOnSave) await format();
      const text = editor.current?.getView()?.state.doc.toString() ?? contents;
      if (text === original) return;
      const message = await fmWriteFile(path, text);
      setOriginal(text);
      flash("Saved");
      onSaved(message);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void save();
    } else if ((e.shiftKey && e.altKey && e.code === "KeyF") || (mod && e.shiftKey && e.code === "KeyI")) {
      // Shift+Alt+F, and VS Code's Linux binding Ctrl+Shift+I.
      e.preventDefault();
      void format();
    }
  }

  async function clipboard(kind: "cut" | "copy" | "paste") {
    const view = editor.current?.getView();
    if (!view) return;
    try {
      if (kind === "paste") {
        const text = await navigator.clipboard.readText();
        view.dispatch({ ...view.state.replaceSelection(text), userEvent: "input.paste" });
      } else {
        const text = view.state.selection.ranges.map((r) => view.state.sliceDoc(r.from, r.to)).join("\n");
        await navigator.clipboard.writeText(text);
        if (kind === "cut") view.dispatch({ ...view.state.replaceSelection(""), userEvent: "delete.cut" });
      }
    } catch {
      flash(`Press Ctrl+${kind === "cut" ? "X" : kind === "copy" ? "C" : "V"} instead`);
    }
    view.focus();
  }

  function menuItems(view: EditorView): MenuItem[] {
    const run = (command: (v: EditorView) => boolean) => () => {
      command(view);
      view.focus();
    };
    const hasSelection = view.state.selection.ranges.some((r) => !r.empty);
    const items: MenuItem[] = [
      { key: "format", label: "Format document", icon: WandSparkles, hint: "Shift+Alt+F", disabled: formatting, onClick: () => void format() },
      { key: "format-on-save", label: "Format on save", icon: formatOnSave ? Check : undefined, onClick: () => setFormatOnSave(!formatOnSave) },
      { key: "cut", label: "Cut", icon: Scissors, hint: "Ctrl+X", separatorBefore: true, disabled: !hasSelection, onClick: () => void clipboard("cut") },
      { key: "copy", label: "Copy", icon: Copy, hint: "Ctrl+C", disabled: !hasSelection, onClick: () => void clipboard("copy") },
      { key: "paste", label: "Paste", icon: ClipboardPaste, hint: "Ctrl+V", onClick: () => void clipboard("paste") },
      { key: "fold", label: "Fold", icon: FoldVertical, hint: "Ctrl+Shift+[", separatorBefore: true, onClick: run(foldCode) },
      { key: "unfold", label: "Unfold", icon: UnfoldVertical, hint: "Ctrl+Shift+]", onClick: run(unfoldCode) },
      { key: "fold-all", label: "Fold all", icon: ChevronsDownUp, hint: "Ctrl+Alt+[", onClick: run(foldAll) },
      { key: "unfold-all", label: "Unfold all", icon: ChevronsUpDown, hint: "Ctrl+Alt+]", onClick: run(unfoldAll) },
      { key: "comment", label: "Toggle comment", icon: MessageSquareCode, hint: "Ctrl+/", separatorBefore: true, onClick: run(toggleComment) },
      { key: "find", label: "Find and replace", icon: Search, hint: "Ctrl+F", onClick: () => editor.current?.openSearch() },
      { key: "goto", label: "Go to line", hint: "Ctrl+Alt+G", onClick: () => editor.current?.openGotoLine() },
    ];
    if (base !== null) {
      items.push(
        { key: "changes", label: changesOpen ? "Hide inline changes" : "Show changes inline", icon: GitCompare, separatorBefore: true, onClick: () => setChangesOpen((v) => !v) },
        { key: "next", label: "Next change", icon: ArrowDown, hint: "Alt+F5", disabled: changes === 0, onClick: () => goToChange(view, 1) },
        { key: "prev", label: "Previous change", icon: ArrowUp, hint: "Shift+Alt+F5", disabled: changes === 0, onClick: () => goToChange(view, -1) },
        { key: "blame", label: "Line blame", icon: lineBlame ? Check : undefined, onClick: () => setLineBlame(!lineBlame) },
      );
      const commit = blame ? commitAtCursor(view.state, blame) : null;
      if (commit && !commit.uncommitted) {
        const sha = commit.sha;
        items.push({ key: "commit", label: `Show commit ${sha.slice(0, 7)}`, icon: GitCommitHorizontal, onClick: () => setHistory({ sha }) });
      }
      items.push({ key: "history", label: "File history", icon: History, onClick: () => setHistory({}) });
    }
    return items;
  }

  function openMenu(e: MouseEvent<HTMLDivElement>) {
    const view = editor.current?.getView();
    if (!view) return;
    e.preventDefault();
    // A right-click outside the selection moves the cursor there first, as editors do.
    const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
    if (pos !== null && !view.state.selection.ranges.some((r) => r.from <= pos && pos <= r.to)) view.dispatch({ selection: { anchor: pos } });
    setMenu({ x: e.clientX, y: e.clientY, items: menuItems(view) });
  }

  return (
    <div className="fm-viewer" onKeyDown={onKeyDown}>
      <FileTabBar path={path} shownPath={shownPath} onReveal={onReveal} onOpenExternal={onOpenExternal} notice={notice}>
        {tracked && (
          <>
            <span className="fm-viewer-notice u-secondary u-nowrap">{changes === 0 ? "No changes" : changes === 1 ? "1 change" : `${changes} changes`}</span>
            <IconButton
              icon={GitCompare}
              label={changesOpen ? "Hide inline changes" : "Show changes inline"}
              size="sm"
              variant={changesOpen ? "secondary" : "ghost"}
              onClick={() => setChangesOpen((v) => !v)}
            />
            <IconButton icon={History} label="File history" size="sm" onClick={() => setHistory({})} />
          </>
        )}
        <IconButton icon={WandSparkles} label="Format document (Shift+Alt+F)" size="sm" loading={formatting} disabled={original === null} onClick={() => void format()} />
        <Button variant={dirty ? "primary" : "secondary"} size="sm" icon={Save} loading={saving} disabled={!dirty && !formatOnSave} onClick={() => void save()}>
          Save
        </Button>
      </FileTabBar>
      {error && (
        <Callout
          tone="danger"
          onDismiss={original === null ? undefined : () => setError(null)}
          actions={
            original === null && (
              <Button variant="secondary" size="sm" icon={SquareArrowOutUpRight} onClick={onOpenExternal}>
                Open with default app
              </Button>
            )
          }
        >
          <span className="fm-viewer-error">{error}</span>
        </Callout>
      )}
      {loading ? (
        <div className="fm-viewer-empty">
          <Spinner size="md" />
        </div>
      ) : (
        original !== null && (
          <div className="fm-editor-host" onContextMenu={openMenu}>
            <CodeEditor
              ref={editor}
              value={contents}
              onChange={setContents}
              fileName={name}
              fill
              ide
              extensions={extensions}
              ariaLabel={`Edit ${name}`}
              onSubmit={() => void save()}
            />
          </div>
        )
      )}
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} />}
      {history && <GitHistory path={path} name={name} initialSha={history.sha} onClose={() => setHistory(null)} />}
    </div>
  );
}
