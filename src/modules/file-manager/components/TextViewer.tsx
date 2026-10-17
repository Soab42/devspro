import { KeyboardEvent, useEffect, useRef, useState } from "react";
import { Button, Callout, Spinner } from "../../../ui";
/* Not from the ui barrel, on purpose — see the note there: importing it through
   the barrel would drag CodeMirror into every page's bundle. */
import { CodeEditor } from "../../../ui/CodeEditor";
import { Save, SquareArrowOutUpRight } from "../../../ui/icons";
import { fmReadFile, fmWriteFile } from "../api";
import { drafts } from "../openFiles";
import { errorMessage } from "../utils";
import FileTabBar from "./FileTabBar";

/**
 * A text or source file in a tab: the editor, highlighted for the file's
 * language, with Save on Ctrl/Cmd+S. Unsaved edits are kept in `drafts`, so
 * leaving the page and coming back finds them. Anything that is not text is
 * refused on the Rust side, and the message offers the desktop's app instead.
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
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

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

  async function save() {
    if (!dirty || saving) return;
    setSaving(true);
    setError(null);
    try {
      const message = await fmWriteFile(path, contents);
      setOriginal(contents);
      setNotice("Saved");
      onSaved(message);
      window.setTimeout(() => setNotice(null), 2500);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void save();
    }
  }

  return (
    <div className="fm-viewer" onKeyDown={onKeyDown}>
      <FileTabBar path={path} shownPath={shownPath} onReveal={onReveal} onOpenExternal={onOpenExternal} notice={notice}>
        <Button variant={dirty ? "primary" : "secondary"} size="sm" icon={Save} loading={saving} disabled={!dirty} onClick={() => void save()}>
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
          {error}
        </Callout>
      )}
      {loading ? (
        <div className="fm-viewer-empty">
          <Spinner size="md" />
        </div>
      ) : (
        original !== null && (
          <CodeEditor value={contents} onChange={setContents} fileName={name} fill ariaLabel={`Edit ${name}`} onSubmit={() => void save()} />
        )
      )}
    </div>
  );
}
