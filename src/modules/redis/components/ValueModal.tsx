import { Button, Callout, CodeBlock, CopyButton, Modal, ModalFooter, Spinner, useResource } from "../../../ui";
import { DatabaseZap, Trash2 } from "../../../ui/icons";
import { getValue, type RedisConn } from "../api";
import { describeValue, formatValue } from "../utils";

/** Fetches on open; the row already told us the type, but the value is fresh. */
export default function ValueModal({
  conn,
  keyName,
  onDelete,
  onClose,
}: {
  conn: RedisConn;
  keyName: string;
  onDelete: () => void;
  onClose: () => void;
}) {
  const value = useResource(() => getValue(conn, keyName));
  const text = value.data ? formatValue(value.data) : "";

  return (
    <Modal
      size="lg"
      title={keyName}
      icon={DatabaseZap}
      description={value.data ? describeValue(value.data) : undefined}
      onClose={onClose}
      footer={
        <ModalFooter start={text ? <CopyButton value={text} label="Copy value" size="md" /> : null}>
          <Button variant="danger" icon={Trash2} onClick={onDelete}>
            Delete key
          </Button>
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
        </ModalFooter>
      }
    >
      {value.loading && !value.error && <Spinner />}
      {value.error && <Callout tone="danger">{value.error}</Callout>}
      {value.data && <CodeBlock maxHeight="60vh" wrap>{text || "(empty)"}</CodeBlock>}
    </Modal>
  );
}
