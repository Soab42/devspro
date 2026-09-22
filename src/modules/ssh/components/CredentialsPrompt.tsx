import { useState } from "react";
import { Button, Field, Form, FormRow, Modal, Stack, TextInput } from "../../../ui";
import { KeyRound } from "../../../ui/icons";
import type { SshHost } from "../api";
import { hostLabel } from "../utils";

export type PromptKind = "password" | "passphrase";

/**
 * Asks for the one secret a connection needs right now. The value goes
 * straight to the connect call and is never kept anywhere — that is the whole
 * reason hosts don't store passwords.
 */
export default function CredentialsPrompt({
  host,
  kind,
  error,
  onSubmit,
  onCancel,
}: {
  host: SshHost;
  kind: PromptKind;
  /** Why the previous attempt failed, if this is a retry. */
  error?: string | null;
  onSubmit: (secret: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState("");
  const isPassword = kind === "password";

  return (
    <Modal
      size="sm"
      title={isPassword ? "Password required" : "Key passphrase required"}
      description={
        isPassword
          ? `Enter the password for ${hostLabel(host)}. It is used for this connection only.`
          : `The private key for ${host.name} is encrypted. Enter its passphrase to unlock it for this connection.`
      }
      icon={KeyRound}
      onClose={onCancel}
    >
      <Form onSubmit={() => onSubmit(value)}>
        <Stack gap={5}>
          <Field label={isPassword ? "Password" : "Passphrase"} htmlFor="ssh-secret" error={error ?? undefined}>
            <TextInput
              id="ssh-secret"
              type="password"
              autoComplete="off"
              value={value}
              onChange={(e) => setValue(e.target.value)}
              autoFocus
            />
          </Field>
          <FormRow className="form-actions">
            <Button variant="secondary" onClick={onCancel}>
              Cancel
            </Button>
            <Button type="submit" variant="primary">
              Connect
            </Button>
          </FormRow>
        </Stack>
      </Form>
    </Modal>
  );
}
