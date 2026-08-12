import { Button, Callout, Field, Modal, ModalFooter, Mono, Stack } from "../../../ui";
import { Download } from "../../../ui/icons";

export interface BackupResultModalProps {
  base: string;
  path: string;
  onClose: () => void;
}

export default function BackupResultModal({ base, path, onClose }: BackupResultModalProps) {
  return (
    <Modal
      size="sm"
      title={`Backup created — ${base}`}
      icon={Download}
      onClose={onClose}
      footer={
        <ModalFooter>
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={5}>
        <Field label="Backup file">
          <Mono variant="chip" copyable>
            {path}
          </Mono>
        </Field>
        <Callout tone="info">
          Saved under DevsPro's own data directory. Use <strong>Restore</strong> to load
          a backup file back in.
        </Callout>
      </Stack>
    </Modal>
  );
}
