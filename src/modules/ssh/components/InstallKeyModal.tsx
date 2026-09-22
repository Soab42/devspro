import { useState } from "react";
import { Button, Callout, Field, Form, FormRow, Modal, Select, Stack } from "../../../ui";
import { KeyRound } from "../../../ui/icons";
import type { SshHost, SshKey } from "../api";
import { hostLabel } from "../utils";

/**
 * Picks which public key to authorize on a host. The connection itself (and
 * its password / trust prompts) happens after this closes, in the tab that
 * opened it, so the prompts never stack on top of this dialog.
 */
export default function InstallKeyModal({
  host,
  keys,
  onPick,
  onClose,
}: {
  host: SshHost;
  keys: SshKey[];
  onPick: (keyName: string) => void;
  onClose: () => void;
}) {
  const preferred = keys.find((k) => host.key_path && k.path === host.key_path) ?? keys[0];
  const [keyName, setKeyName] = useState(preferred?.name ?? "");

  return (
    <Modal
      size="sm"
      title={`Install key on ${host.name}`}
      description={`Appends the public key to ~/.ssh/authorized_keys for ${hostLabel(host)}, like ssh-copy-id. You will be asked for the host's password if it needs one.`}
      icon={KeyRound}
      onClose={onClose}
    >
      {keys.length === 0 ? (
        <Callout tone="warning">No keys found in ~/.ssh. Generate one in the Keys tab first.</Callout>
      ) : (
        <Form onSubmit={() => keyName && onPick(keyName)}>
          <Stack gap={5}>
            <Field label="Public key" htmlFor="ssh-install-key">
              <Select
                id="ssh-install-key"
                value={keyName}
                onChange={setKeyName}
                options={keys.map((k) => ({
                  value: k.name,
                  label: k.comment ? `${k.name} — ${k.comment}` : k.name,
                }))}
              />
            </Field>
            <FormRow className="form-actions">
              <Button variant="secondary" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" variant="primary" disabled={!keyName}>
                Install
              </Button>
            </FormRow>
          </Stack>
        </Form>
      )}
    </Modal>
  );
}
