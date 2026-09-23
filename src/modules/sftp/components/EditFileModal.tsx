import { useCallback, useEffect, useRef, useState } from "react";
import {
  Button,
  Callout,
  Field,
  Modal,
  ModalFooter,
  Mono,
  Spinner,
} from "../../../ui";
/* Not from the ui barrel, on purpose — see the note there: importing it through
   the barrel would drag CodeMirror into every page's bundle. */
import { CodeEditor } from "../../../ui/CodeEditor";
import { Pencil, SquareArrowOutUpRight } from "../../../ui/icons";
import { fmStat } from "../../file-manager/api";
import { errorMessage } from "../../ssh/utils";

/** How often the external copy is checked for a save. */
const WATCH_MS = 1500;

/**
 * Edits one text file in place, on either side of the SFTP panes. The caller
 * supplies the read and write, so the same dialog serves a local file (the
 * file-manager commands) and a remote one (the SFTP session). Size and
 * binary-ness are policed on the Rust side; the message shows here.
 *
 * "Open in editor" hands the file to the user's own editor instead. The
 * caller says where the editable copy lives (`openExternal`) and what to do
 * when it changes (`onExternalSaved`, e.g. upload it); this dialog watches
 * the copy's mtime and, after each save, refreshes its own view too.
 */
export default function EditFileModal({
  path,
  where,
  read,
  write,
  openExternal,
  onExternalSaved,
  startExternal = false,
  onClose,
  onSaved,
}: {
  path: string;
  /** "Local" or the host name, for the title. */
  where: string;
  read: () => Promise<string>;
  write: (contents: string) => Promise<string>;
  /** Opens the file in the external editor; resolves to the local path to watch. */
  openExternal: () => Promise<string>;
  /** Runs after each save in the external editor, with the local copy's path. */
  onExternalSaved: (localPath: string) => Promise<void>;
  /** Open the external editor as soon as the dialog appears. */
  startExternal?: boolean;
  onClose: () => void;
  onSaved: (message: string) => void;
}) {
  const [original, setOriginal] = useState<string | null>(null);
  const [contents, setContents] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [external, setExternal] = useState<{
    local: string;
    modified: number;
    synced: Date | null;
  } | null>(null);
  const [opening, setOpening] = useState(false);

  const load = useCallback(async () => {
    const text = await read();
    setOriginal(text);
    setContents(text);
  }, [read]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        await load();
      } catch (e) {
        if (!cancelled) setError(errorMessage(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
    // `read` is bound to the path; a new path is a new dialog (keyed by the caller).
  }, [path]);

  const unchanged = original !== null && contents === original;
  const name = path.split("/").pop() ?? path;
  const ext = name.includes(".")
    ? name.slice(name.lastIndexOf(".") + 1).toLowerCase()
    : "";
  const language = ext === "yml" || ext === "yaml" ? "yaml" : undefined;

  async function handleSave() {
    setBusy(true);
    setError(null);
    try {
      const message = await write(contents);
      setOriginal(contents);
      onSaved(message);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const handleOpenExternal = useCallback(async () => {
    setOpening(true);
    setError(null);
    try {
      const local = await openExternal();
      const stat = await fmStat(local);
      setExternal({ local, modified: stat.modified, synced: null });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setOpening(false);
    }
  }, [openExternal]);

  const started = useRef(false);
  useEffect(() => {
    if (!startExternal || started.current) return;
    started.current = true;
    void handleOpenExternal();
  }, [startExternal, handleOpenExternal]);

  // Watch the external copy. A newer mtime means the editor saved: hand the
  // copy to the caller, then pull the result back into this view — unless the
  // user has been typing here too, in which case their draft is left alone.
  const unchangedRef = useRef(unchanged);
  unchangedRef.current = unchanged;
  useEffect(() => {
    if (!external) return;
    let stopped = false;
    let last = external.modified;
    const timer = setInterval(async () => {
      try {
        const stat = await fmStat(external.local);
        if (stopped || stat.modified === last) return;
        last = stat.modified;
        await onExternalSaved(external.local);
        if (stopped) return;
        if (unchangedRef.current) await load();
        setExternal((prev) =>
          prev ? { ...prev, modified: last, synced: new Date() } : prev,
        );
      } catch (e) {
        if (!stopped) setError(errorMessage(e));
      }
    }, WATCH_MS);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
    // Re-arm only when a different external copy is opened, not on each tick's state update.
  }, [external?.local]);

  const syncNote = external
    ? external.synced
      ? `Last saved from your editor at ${external.synced.toLocaleTimeString()}`
      : "Open in your editor — saves there are picked up automatically"
    : unchanged
      ? "No unsaved changes"
      : "Written in place; permissions are kept";

  return (
    <Modal
      size="lg"
      resizable
      title={`Edit · ${name} · ${where}`}
      icon={Pencil}
      onClose={onClose}
      footer={
        <ModalFooter
          start={
            <Button
              variant="secondary"
              icon={SquareArrowOutUpRight}
              loading={opening}
              disabled={loading || original === null}
              onClick={handleOpenExternal}
            >
              Open in editor
            </Button>
          }
          hint={syncNote}
        >
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={loading || original === null || unchanged}
            onClick={handleSave}
          >
            Save
          </Button>
        </ModalFooter>
      }
    >
      <div className="sftp-editor-body">
        {loading && (
          <span className="dt-cell-inline">
            <Spinner />
            Reading the file…
          </span>
        )}
        {external && (
          <Callout tone="info">
            Editing <Mono>{external.local}</Mono> in your editor. Each save
            there is written back to <Mono>{path}</Mono>.
          </Callout>
        )}
        {original !== null && (
          <Field
            label="Contents"
            htmlFor="sftp-editor"
            hint={<Mono truncate>{path}</Mono>}
          >
            <CodeEditor
              id="sftp-editor"
              language={language}
              ariaLabel={`Contents of ${path}`}
              fill
              value={contents}
              readOnly={busy}
              onChange={setContents}
            />
          </Field>
        )}
        {error && <Callout tone="danger">{error}</Callout>}
      </div>
    </Modal>
  );
}
