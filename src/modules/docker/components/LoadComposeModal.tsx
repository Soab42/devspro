import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Button,
  Callout,
  Field,
  FormRow,
  Modal,
  ModalFooter,
  Mono,
  Stack,
  TextInput,
} from "../../../ui";
import { FileUp } from "../../../ui/icons";
import { examplePath } from "../../../app/platform";
import { composeUpFile, composeValidate } from "../api";

export default function LoadComposeModal({
  onClose,
  onLoaded,
}: {
  onClose: () => void;
  onLoaded: (message: string) => void;
}) {
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [validation, setValidation] = useState<string | null>(null);

  async function handleBrowse() {
    const selected = await open({
      multiple: false,
      title: "Select a docker-compose file",
      filters: [{ name: "Compose file", extensions: ["yml", "yaml"] }],
    });
    if (!selected || Array.isArray(selected)) return;
    setPath(selected);
    setError(null);
    setValidation(null);
  }

  /* Validation is a separate, explicit step rather than something that runs on
     every keystroke: `compose config` shells out and resolves variables, which is
     too slow and too surprising to fire while someone is typing a path. */
  async function handleValidate() {
    setBusy(true);
    setError(null);
    setValidation(null);
    try {
      setValidation(await composeValidate(path.trim()));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleLoad() {
    setBusy(true);
    setError(null);
    try {
      onLoaded(await composeUpFile(path.trim()));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  return (
    <Modal
      size="md"
      title="Load compose file"
      icon={FileUp}
      onClose={onClose}
      footer={
        <ModalFooter
          hint={busy ? "Running docker compose…" : undefined}
          start={
            <Button variant="ghost" disabled={!path.trim() || busy} onClick={handleValidate}>
              Check file
            </Button>
          }
        >
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={!path.trim()}
            onClick={handleLoad}
          >
            Start stack
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={4}>
        <Field
          label="Compose file"
          htmlFor="compose-path"
          description="Runs docker compose up -d from the file's own directory."
        >
          <FormRow>
            <TextInput
              id="compose-path"
              mono
              placeholder={examplePath("project", "docker-compose.yml")}
              value={path}
              onChange={(e) => setPath(e.target.value)}
            />
            <Button variant="secondary" onClick={handleBrowse}>
              Browse…
            </Button>
          </FormRow>
        </Field>

        <Callout tone="info">
          The stack is named after its containing folder, exactly as it would be
          from a terminal — so a stack started here and one started by hand are the
          same stack, not two copies.
        </Callout>

        {validation && (
          <Callout tone="success">
            <Mono>{validation}</Mono>
          </Callout>
        )}
        {error && <Callout tone="danger">{error}</Callout>}
      </Stack>
    </Modal>
  );
}
