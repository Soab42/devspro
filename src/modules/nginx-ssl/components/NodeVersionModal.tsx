import { useState } from "react";
import {
  Button,
  Callout,
  Field,
  Modal,
  ModalFooter,
  Mono,
  Select,
  Stack,
  useResource,
} from "../../../ui";
import { Hexagon } from "../../../ui/icons";
import { listNvmVersions, Site, writeNvmrc } from "../api";

export default function NodeVersionModal({
  site,
  onClose,
  onWritten,
}: {
  site: Site;
  onClose: () => void;
  onWritten: (message: string) => void;
}) {
  const versions = useResource(listNvmVersions);
  const [selected, setSelected] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleWrite() {
    if (site.backend.type !== "laravel" || !selected) return;
    setBusy(true);
    setError(null);
    try {
      await writeNvmrc(site.backend.project_path, selected);
      onWritten(`Wrote .nvmrc (${selected}) to ${site.backend.project_path}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  const available = versions.data ?? [];

  return (
    <Modal
      size="sm"
      title={`Node version — ${site.name}`}
      icon={Hexagon}
      onClose={onClose}
      footer={
        <ModalFooter>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={!selected}
            onClick={handleWrite}
          >
            Write .nvmrc
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={4}>
        {/* Load-bearing: explains that DevsPro is not running the Node app,
            so nobody expects this to start anything. */}
        <Callout tone="info">
          DevsPro doesn't run Node apps directly — this writes a{" "}
          <Mono variant="chip">.nvmrc</Mono> file into the project so{" "}
          <Mono variant="chip">nvm use</Mono> (run manually, or via your editor's nvm
          integration) picks up this version.
        </Callout>

        {!versions.loading && available.length === 0 ? (
          <Callout tone="warning">
            No Node versions found under nvm — install one first from the Apps page.
          </Callout>
        ) : (
          <Field label="Node version" htmlFor="site-node-version">
            <Select
              id="site-node-version"
              value={selected}
              onChange={setSelected}
              placeholder="Choose a version"
              options={available.map((v) => ({ value: v, label: v }))}
            />
          </Field>
        )}

        {error && <Callout tone="danger">{error}</Callout>}
      </Stack>
    </Modal>
  );
}
