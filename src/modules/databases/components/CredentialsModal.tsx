import { Button, EmptyState, Field, Modal, ModalFooter, Mono, Stack } from "../../../ui";
import { KeyRound } from "../../../ui/icons";
import type { Credentials } from "../api";

export interface CredentialsModalProps {
  base: string;
  credentials: Credentials;
  /** Used when the image implies a username the service doesn't report. */
  fallbackUsername: string | null;
  onClose: () => void;
  onChangePassword: () => void;
}

/**
 * Was an inline IIFE inside DatabasesPage. Extracted so the page reads as a
 * page rather than as four dialogs in a trench coat.
 */
export default function CredentialsModal({
  base,
  credentials,
  fallbackUsername,
  onClose,
  onChangePassword,
}: CredentialsModalProps) {
  const username = credentials.username ?? fallbackUsername;

  return (
    <Modal
      size="sm"
      title={`Credentials — ${base}`}
      icon={KeyRound}
      onClose={onClose}
      footer={
        <ModalFooter
          start={
            <Button variant="secondary" onClick={onChangePassword}>
              Change password
            </Button>
          }
        >
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={5}>
        <Field label="Username">
          <Mono variant="chip" copyable>
            {username ?? "(none — Redis uses a password only)"}
          </Mono>
        </Field>

        <Field label="Password">
          {credentials.password ? (
            <Mono variant="chip" copyable>
              {credentials.password}
            </Mono>
          ) : (
            <EmptyState
              title="No password found"
              description="Could not find a password for this service."
            />
          )}
        </Field>
      </Stack>
    </Modal>
  );
}
