import { useState } from "react";
import {
  Button,
  Callout,
  Field,
  Form,
  FormRow,
  Modal,
  ModalFooter,
  Stack,
  TextInput,
} from "../../../ui";
import { KeyRound, WandSparkles } from "../../../ui/icons";
import { changeAdminPassword, changeUserPassword } from "../api";
import { generatePassword } from "../utils";

export default function ChangePasswordModal({
  base,
  username,
  onClose,
  onChanged,
}: {
  base: string;
  username?: string;
  onClose: () => void;
  onChanged?: () => void;
}) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  async function handleSubmit() {
    if (!password.trim()) {
      setError("A password is required.");
      return;
    }
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const result = username
        ? await changeUserPassword(base, username, password.trim())
        : await changeAdminPassword(base, password.trim());
      setMessage(result);
      onChanged?.();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      size="sm"
      title={`Change password — ${username ?? "admin"}`}
      icon={KeyRound}
      onClose={onClose}
      footer={
        <ModalFooter>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} onClick={handleSubmit}>
            Change password
          </Button>
        </ModalFooter>
      }
    >
      <Form onSubmit={handleSubmit}>
        <Stack gap={4}>
          <Field label="New password" htmlFor="new-password" required>
            <FormRow>
              <TextInput
                id="new-password"
                mono
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                autoFocus
                className="u-flex-1"
              />
              <Button
                variant="secondary"
                icon={WandSparkles}
                onClick={() => setPassword(generatePassword())}
              >
                Generate
              </Button>
            </FormRow>
          </Field>

          {!username && (
            <Callout tone="info">
              Updates the live database and rewrites DevsPro's compose file, so future
              actions (Reveal password, Manage users) keep using the new password
              automatically.
            </Callout>
          )}

          {error && <Callout tone="danger">{error}</Callout>}
          {message && <Callout tone="success">{message}</Callout>}
        </Stack>
      </Form>
    </Modal>
  );
}
