import { useEffect, useState } from "react";
import { Button, Callout, Field, Modal, ModalFooter, Mono, Spinner, Stack } from "../../../ui";
/* Not from the ui barrel, on purpose — see the note there: importing it through
   the barrel would drag CodeMirror into every page's bundle. */
import { CodeEditor } from "../../../ui/CodeEditor";
import { Pencil } from "../../../ui/icons";
import { fmReadFile, fmWriteFile } from "../api";
import { errorMessage, extensionOf } from "../utils";

/**
 * Opens one text file in the in-app editor. Small config and source files are
 * what a dev-environment file manager is really for; anything binary or large
 * is refused on the Rust side and the message shows here.
 */
export default function FileEditorModal({
  path,
  onClose,
  onSaved,
}: {
  path: string;
  onClose: () => void;
  onSaved: (message: string) => void;
}) {
  const [original, setOriginal] = useState<string | null>(null);
  const [contents, setContents] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const text = await fmReadFile(path);
        if (cancelled) return;
        setOriginal(text);
        setContents(text);
      } catch (e) {
        if (!cancelled) setError(errorMessage(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [path]);

  const unchanged = original !== null && contents === original;
  const ext = extensionOf(path);
  const language = ext === "yml" || ext === "yaml" ? "yaml" : undefined;
  const name = path.split("/").pop() ?? path;

  async function handleSave() {
    setBusy(true);
    setError(null);
    try {
      const message = await fmWriteFile(path, contents);
      setOriginal(contents);
      onSaved(message);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      size="lg"
      title={`Edit · ${name}`}
      icon={Pencil}
      onClose={onClose}
      footer={
        <ModalFooter hint={unchanged ? "No unsaved changes" : "Written in place; permissions are kept"}>
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
          <Button variant="primary" loading={busy} disabled={loading || original === null || unchanged} onClick={handleSave}>
            Save
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={4}>
        {loading && (
          <span className="dt-cell-inline">
            <Spinner />
            Reading the file…
          </span>
        )}
        {original !== null && (
          <Field label="Contents" htmlFor="fm-editor" hint={<Mono truncate>{path}</Mono>}>
            <CodeEditor
              id="fm-editor"
              language={language}
              ariaLabel={`Contents of ${path}`}
              rows={24}
              value={contents}
              readOnly={busy}
              onChange={setContents}
            />
          </Field>
        )}
        {error && <Callout tone="danger">{error}</Callout>}
      </Stack>
    </Modal>
  );
}
