import { useState } from "react";
import {
  Button,
  Callout,
  Field,
  Modal,
  ModalFooter,
  Select,
  Stack,
  useResource,
} from "../../../ui";
import { FileCode } from "../../../ui/icons";
import { listPhpVersions, setSitePhpVersion, Site } from "../api";

const SYSTEM_DEFAULT = "";

export default function PhpVersionModal({
  site,
  onClose,
  onSaved,
}: {
  site: Site;
  onClose: () => void;
  onSaved: () => void;
}) {
  const versions = useResource(listPhpVersions);
  const [selected, setSelected] = useState(
    site.backend.type === "laravel"
      ? (site.backend.php_version ?? SYSTEM_DEFAULT)
      : SYSTEM_DEFAULT,
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSave() {
    setBusy(true);
    setError(null);
    try {
      await setSitePhpVersion(site.id, selected || null);
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  const available = versions.data ?? [];

  return (
    <Modal
      size="sm"
      title={`PHP version — ${site.name}`}
      icon={FileCode}
      onClose={onClose}
      footer={
        <ModalFooter hint={busy ? "Waiting for your admin password…" : undefined}>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} onClick={handleSave}>
            Save
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={4}>
        <Field label="PHP version" htmlFor="site-php-version">
          <Select
            id="site-php-version"
            value={selected}
            onChange={setSelected}
            options={[
              { value: SYSTEM_DEFAULT, label: "System default" },
              ...available.map((v) => ({ value: v.version, label: `PHP ${v.version}` })),
            ]}
          />
        </Field>

        {!versions.loading && available.length === 0 && (
          <Callout tone="info">
            No PHP-FPM pools are currently running — this site will use the system
            default.
          </Callout>
        )}

        {error && <Callout tone="danger">{error}</Callout>}
      </Stack>
    </Modal>
  );
}
