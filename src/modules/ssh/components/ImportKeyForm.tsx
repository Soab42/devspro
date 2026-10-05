import { useState } from "react";
import { Button, Callout, Field, Form, FormRow, Stack, TextArea, TextInput } from "../../../ui";
import { SshKey, importKey } from "../api";

/**
 * A key pair pasted from somewhere else — a password manager, another
 * machine, a hosting panel — saved under a name like a generated one, so
 * every host's key picker lists it.
 */
export default function ImportKeyForm({
  existing,
  onImported,
  onCancel,
}: {
  existing: SshKey[];
  onImported: (key: SshKey) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState("");
  const [privateKey, setPrivateKey] = useState("");
  const [publicKey, setPublicKey] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const nameTaken = existing.some((k) => k.name === name.trim());

  async function handleSubmit() {
    setError(null);
    if (!name.trim()) {
      setError("A key name is required.");
      return;
    }
    if (nameTaken) {
      setError(`A key named "${name.trim()}" already exists.`);
      return;
    }
    if (!privateKey.trim()) {
      setError("Paste the private key.");
      return;
    }
    setBusy(true);
    try {
      onImported(await importKey({ name: name.trim(), privateKey, publicKey, passphrase }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Form onSubmit={handleSubmit}>
      <Stack gap={5}>
        <Field
          label="Name"
          htmlFor="ssh-import-name"
          required
          description="Saved as ~/.ssh/<name> and ~/.ssh/<name>.pub — e.g. brokercore-live."
          error={nameTaken ? "A key with this name already exists." : undefined}
        >
          <TextInput
            id="ssh-import-name"
            mono
            placeholder="brokercore-live"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
          />
        </Field>

        <Field
          label="Private key"
          htmlFor="ssh-import-private"
          required
          description="The whole file, BEGIN and END lines included. Saved readable by you only."
        >
          <TextArea
            id="ssh-import-private"
            mono
            rows={8}
            spellCheck={false}
            autoComplete="off"
            placeholder={"-----BEGIN OPENSSH PRIVATE KEY-----\n…\n-----END OPENSSH PRIVATE KEY-----"}
            value={privateKey}
            onChange={(e) => setPrivateKey(e.target.value)}
          />
        </Field>

        <Field
          label="Public key"
          htmlFor="ssh-import-public"
          description="Optional — worked out from the private key when left empty."
        >
          <TextArea
            id="ssh-import-public"
            mono
            rows={3}
            spellCheck={false}
            placeholder="ssh-ed25519 AAAAC3… you@laptop"
            value={publicKey}
            onChange={(e) => setPublicKey(e.target.value)}
          />
        </Field>

        <Field
          label="Passphrase"
          htmlFor="ssh-import-pass"
          description="Only for a key that has one, and only to work out its public key. Not saved."
        >
          <TextInput
            id="ssh-import-pass"
            type="password"
            autoComplete="off"
            value={passphrase}
            onChange={(e) => setPassphrase(e.target.value)}
          />
        </Field>

        {error && <Callout tone="danger">{error}</Callout>}

        <FormRow className="form-actions">
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={busy}>
            Import key
          </Button>
        </FormRow>
      </Stack>
    </Form>
  );
}
