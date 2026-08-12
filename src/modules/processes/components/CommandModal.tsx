import { Button, CodeBlock, CopyButton, Modal, ModalFooter } from "../../../ui";
import { Terminal } from "../../../ui/icons";

export default function CommandModal({
  command,
  onClose,
}: {
  command: string;
  onClose: () => void;
}) {
  return (
    <Modal
      size="md"
      title="Full command"
      icon={Terminal}
      onClose={onClose}
      footer={
        <ModalFooter start={<CopyButton value={command} label="Copy command" size="md" />}>
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
        </ModalFooter>
      }
    >
      <CodeBlock>{command}</CodeBlock>
    </Modal>
  );
}
