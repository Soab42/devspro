import { Badge, Button, Callout, CodeBlock, CopyButton, Modal, ModalFooter, Spinner, Stack, useResource } from "../../../ui";
import { Workflow } from "../../../ui/icons";
import { peekJob, type QueueView, type RedisConn } from "../api";

/** The first pending job on a queue, as Laravel serialised it. */
export default function JobModal({
  conn,
  queue,
  onClose,
}: {
  conn: RedisConn;
  queue: QueueView;
  onClose: () => void;
}) {
  const job = useResource(() => peekJob(conn, queue.key, 0));
  const name = job.data?.display_name ?? job.data?.command_name;

  return (
    <Modal
      size="lg"
      title={`Next job on ${queue.queue}`}
      icon={Workflow}
      description={name ?? queue.key}
      onClose={onClose}
      footer={
        <ModalFooter start={job.data ? <CopyButton value={job.data.raw} label="Copy payload" size="md" /> : null}>
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={3}>
        {job.loading && !job.error && <Spinner />}
        {job.error && <Callout tone="danger">{job.error}</Callout>}
        {job.data && (
          <>
            <Stack direction="row" gap={2} align="center">
              {job.data.id && <Badge mono>{job.data.id}</Badge>}
              {job.data.attempts !== null && (
                <Badge tone={job.data.attempts > 0 ? "warning" : "neutral"}>
                  {job.data.attempts} attempt{job.data.attempts === 1 ? "" : "s"}
                </Badge>
              )}
            </Stack>
            <CodeBlock maxHeight="60vh" wrap>{job.data.raw}</CodeBlock>
          </>
        )}
      </Stack>
    </Modal>
  );
}
