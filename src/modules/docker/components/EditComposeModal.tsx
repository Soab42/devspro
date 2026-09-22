import { useEffect, useRef, useState } from "react";
import {
  Badge,
  Button,
  Callout,
  Field,
  Modal,
  ModalFooter,
  Mono,
  Spinner,
  Stack,
} from "../../../ui";
/* Not from the ui barrel, on purpose — see the note there: importing it through
   the barrel would drag CodeMirror into every page's bundle. */
import { CodeEditor, CodeEditorHandle } from "../../../ui/CodeEditor";
import { Pencil } from "../../../ui/icons";
import { ComposeSource, readContainerCompose, readProjectCompose, writeCompose } from "../api";

/**
 * Edits the compose file behind a stack or a single container.
 *
 * Both open the *same whole file* — a compose project is one file, and a
 * container's config is a block inside it. What differs is the apply: opened from
 * a container, the save recreates only that service; opened from a stack, it
 * brings the whole stack up. The editor says which, so the button is never a
 * surprise.
 *
 * Raw YAML rather than form fields on purpose. Compose supports far more than a
 * form could model, and a field-by-field rewrite risks mangling the keys it
 * doesn't know about — including comments the user wrote.
 */
export default function EditComposeModal({
  target,
  onClose,
  onSaved,
}: {
  /** A container id with its display name, or a compose project name. */
  target: { kind: "container"; id: string; name: string } | { kind: "project"; project: string };
  onClose: () => void;
  onSaved: (message: string) => void;
}) {
  const [source, setSource] = useState<ComposeSource | null>(null);
  const [contents, setContents] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const editor = useRef<CodeEditorHandle>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const loaded =
          target.kind === "container"
            ? await readContainerCompose(target.id)
            : await readProjectCompose(target.project);
        if (cancelled) return;
        setSource(loaded);
        setContents(loaded.contents);
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
    // Keyed by identity, not by the `target` object, which is a fresh literal on
    // every render of the page and would re-fetch in a loop.
  }, [target.kind === "container" ? target.id : target.project, target.kind]);

  /* Puts the caret on the edited service's line. A compose file for a real stack
     is long enough that landing at the top means hunting for the service you
     clicked — and the block is what you came to change. */
  useEffect(() => {
    if (source?.service) editor.current?.revealLine(`${source.service}:`);
    // Only when the file first arrives — re-running on every keystroke would
    // yank the caret back mid-edit.
  }, [source]);

  const scope = source?.service;
  const unchanged = source !== null && contents === source.contents;

  async function handleSave() {
    if (!source) return;
    setBusy(true);
    setError(null);
    try {
      onSaved(await writeCompose(source.path, contents, source.project, source.service));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  return (
    <Modal
      size="lg"
      title={
        target.kind === "container"
          ? `Edit compose · ${target.name}`
          : `Edit compose · ${target.project}`
      }
      icon={Pencil}
      onClose={onClose}
      footer={
        <ModalFooter
          hint={
            busy
              ? "Validating, saving, then docker compose up…"
              : scope
                ? `Recreates ${scope} only`
                : "Recreates whatever changed in the stack"
          }
        >
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={loading || !source || unchanged}
            onClick={handleSave}
          >
            {scope ? `Save & apply to ${scope}` : "Save & apply to stack"}
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={4}>
        {loading && (
          /* Spinner's own label is screen-reader-only, so the text is spelled
             out here for everyone else. */
          <span className="dt-cell-inline">
            <Spinner />
            Reading the compose file…
          </span>
        )}

        {source && (
          <>
            <Field
              label="Compose file"
              htmlFor="compose-contents"
              description={
                scope
                  ? "The whole file — everything compose supports is editable here, not just this service."
                  : "The whole file, applied to every service in the stack."
              }
              hint={
                <span className="dt-cell-inline">
                  <Mono truncate>{source.path}</Mono>
                  {source.project && <Badge mono>{source.project}</Badge>}
                </span>
              }
            >
              <CodeEditor
                id="compose-contents"
                ref={editor}
                language="yaml"
                ariaLabel={`Compose file ${source.path}`}
                rows={20}
                value={contents}
                readOnly={busy}
                onChange={setContents}
              />
            </Field>

            <Callout tone="info">
              Tab indents, Enter keeps the indent — indentation is what carries
              meaning in YAML, and the line numbers match the ones{" "}
              <Mono>docker compose config</Mono> reports. Checked with it before
              anything is written — if it doesn't parse, the file on disk is left exactly as it
              is. Saving then runs <Mono>up -d</Mono>, which recreates only what
              actually changed.
            </Callout>
          </>
        )}

        {error && <Callout tone="danger">{error}</Callout>}
      </Stack>
    </Modal>
  );
}
