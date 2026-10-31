import { useState } from "react";
import { Button, Checkbox, Field, Form, FormRow, Modal, Stack, TextInput } from "../../../ui";
import { KeyRound } from "../../../ui/icons";
import type { SshHost } from "../api";
import { hostLabel } from "../utils";

export type PromptKind = "password" | "passphrase";

export interface PromptAnswer {
  secret: string;
  /** Only ever true for a password, and only when the user ticked the box. */
  remember: boolean;
}

/**
 * Asks for the one secret a connection needs right now. By default the value
 * goes straight to the connect call and is kept nowhere. A password can be
 * remembered instead, encrypted on this machine, if the user ticks the box;
 * a key passphrase never is.
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
  onSubmit: (answer: PromptAnswer) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState("");
  const [remember, setRemember] = useState(false);
  const isPassword = kind === "password";

  return (
    <Modal
      size="sm"
      title={isPassword ? "Password required" : "Key passphrase required"}
      description={
        isPassword
          ? `Enter the password for ${hostLabel(host)}. Unless you choose to remember it, it is used for this connection only.`
          : `The private key for ${host.name} is encrypted. Enter its passphrase to unlock it for this connection.`
      }
      icon={KeyRound}
      onClose={onCancel}
    >
      <Form onSubmit={() => onSubmit({ secret: value, remember: isPassword && remember })}>
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
          {isPassword && (
            <Stack gap={1}>
              <Checkbox label="Remember this password" checked={remember} onChange={setRemember} />
              <span className="u-muted">
                Saved encrypted on this computer, so connecting won't ask again. Forget it from the host's menu in SSH.
              </span>
            </Stack>
          )}
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
