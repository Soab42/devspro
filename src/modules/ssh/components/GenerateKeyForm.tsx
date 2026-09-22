import { useState } from "react";
import { Button, Callout, Field, Form, FormGrid, FormRow, Select, Stack, TextInput } from "../../../ui";
import { SshKey, generateKey } from "../api";

type KeyType = "ed25519" | "rsa";

const TYPE_OPTIONS = [
  { value: "ed25519", label: "Ed25519 (recommended)" },
  { value: "rsa", label: "RSA" },
];

const RSA_BITS = [
  { value: "2048", label: "2048 bits" },
  { value: "3072", label: "3072 bits" },
  { value: "4096", label: "4096 bits" },
];

export default function GenerateKeyForm({
  existing,
  onGenerated,
  onCancel,
}: {
  existing: SshKey[];
  onGenerated: (key: SshKey) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(existing.some((k) => k.name === "id_ed25519") ? "" : "id_ed25519");
  const [keyType, setKeyType] = useState<KeyType>("ed25519");
  const [bits, setBits] = useState("4096");
  const [comment, setComment] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [confirm, setConfirm] = useState("");
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
    if (passphrase !== confirm) {
      setError("The passphrases do not match.");
      return;
    }
    setBusy(true);
    try {
      const key = await generateKey({
        name: name.trim(),
        keyType,
        bits: keyType === "rsa" ? Number(bits) : null,
        comment: comment.trim(),
        passphrase,
      });
      onGenerated(key);
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
          label="File name"
          htmlFor="ssh-key-name"
          required
          description="Saved as ~/.ssh/<name> and ~/.ssh/<name>.pub."
          error={nameTaken ? "A key with this name already exists." : undefined}
        >
          <TextInput
            id="ssh-key-name"
            mono
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
          />
        </Field>

        <FormGrid columns={2}>
          <Field label="Type" htmlFor="ssh-key-type">
            <Select
              id="ssh-key-type"
              value={keyType}
              onChange={(v) => setKeyType(v as KeyType)}
              options={TYPE_OPTIONS}
            />
          </Field>
          {keyType === "rsa" && (
            <Field label="Size" htmlFor="ssh-key-bits">
              <Select id="ssh-key-bits" value={bits} onChange={setBits} options={RSA_BITS} />
            </Field>
          )}
        </FormGrid>

        <Field label="Comment" htmlFor="ssh-key-comment" description="Usually who and where: you@laptop.">
          <TextInput
            id="ssh-key-comment"
            placeholder="you@laptop"
            value={comment}
            onChange={(e) => setComment(e.target.value)}
          />
        </Field>

        <FormGrid columns={2}>
          <Field label="Passphrase" htmlFor="ssh-key-pass" description="Optional, but recommended.">
            <TextInput
              id="ssh-key-pass"
              type="password"
              autoComplete="new-password"
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
            />
          </Field>
          <Field label="Confirm passphrase" htmlFor="ssh-key-pass2">
            <TextInput
              id="ssh-key-pass2"
              type="password"
              autoComplete="new-password"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
            />
          </Field>
        </FormGrid>

        {error && <Callout tone="danger">{error}</Callout>}

        <FormRow className="form-actions">
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={busy}>
            Generate key
          </Button>
        </FormRow>
      </Stack>
    </Form>
  );
}
