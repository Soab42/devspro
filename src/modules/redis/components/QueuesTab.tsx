import { useState } from "react";
import {
  Badge,
  Button,
  Callout,
  Column,
  DataTable,
  Section,
  Stack,
  useResource,
} from "../../../ui";
import { Eye, Inbox } from "../../../ui/icons";
import { laravelQueues, type QueueView, type RedisConn } from "../api";
import JobModal from "./JobModal";

const POLL_MS = 5000;

/**
 * Laravel's Redis queue driver keeps each queue as `queues:<name>` (a list of
 * pending jobs) with `:delayed` and `:reserved` sorted sets beside it. Failed
 * jobs live in the database, not here, so there's no column for them.
 */
export default function QueuesTab({ conn }: { conn: RedisConn }) {
  const queues = useResource(() => laravelQueues(conn), { pollMs: POLL_MS });
  const [peek, setPeek] = useState<QueueView | null>(null);

  const columns: Column<QueueView>[] = [
    {
      key: "queue",
      header: "Queue",
      priority: 1,
      mono: true,
      sortValue: (r) => r.queue,
      render: (r) => (
        <span title={r.key}>{r.queue}</span>
      ),
    },
    {
      key: "pending",
      header: "Pending",
      priority: 1,
      align: "end",
      width: "1px",
      sortValue: (r) => r.pending,
      render: (r) => (
        <Badge tone={r.pending > 0 ? "accent" : "neutral"}>{r.pending}</Badge>
      ),
    },
    {
      key: "delayed",
      header: "Delayed",
      priority: 2,
      align: "end",
      width: "1px",
      sortValue: (r) => r.delayed,
      render: (r) => r.delayed,
    },
    {
      key: "reserved",
      header: "Reserved",
      priority: 2,
      align: "end",
      width: "1px",
      sortValue: (r) => r.reserved,
      render: (r) => (
        <Badge tone={r.reserved > 0 ? "info" : "neutral"}>{r.reserved}</Badge>
      ),
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (r) => (
        <Button
          size="sm"
          variant="secondary"
          icon={Eye}
          disabled={r.pending === 0}
          onClick={() => setPeek(r)}
        >
          Peek
        </Button>
      ),
    },
  ];

  const horizon = queues.data?.horizon_keys ?? 0;

  return (
    <Stack gap={4}>
      {queues.error && queues.data && <Callout tone="danger">{queues.error}</Callout>}
      {horizon > 0 && (
        <Callout tone="info">
          Horizon is in use ({horizon} <code>horizon:*</code> keys). Its dashboard has the full
          picture; the counts below are the raw queue structures it drives.
        </Callout>
      )}
      <Section
        title="Laravel queues"
        padding="none"
        description="Refreshes every 5 seconds. Reserved jobs are being worked on right now."
        meta={
          queues.data && (
            <span className="u-muted">
              {queues.data.queues.reduce((n, q) => n + q.pending, 0)} pending in total
            </span>
          )
        }
      >
        <DataTable
          columns={columns}
          rows={queues.data?.queues ?? []}
          rowKey={(r) => r.key}
          loading={queues.loading}
          defaultSort={{ key: "queue", dir: "asc" }}
          caption="Laravel queues found in this database"
          empty={{
            icon: Inbox,
            title: "No Laravel queues",
            description:
              queues.error ??
              "No queues:* keys in this database. Dispatch a job with QUEUE_CONNECTION=redis and it will appear here.",
          }}
        />
      </Section>
      {peek && <JobModal conn={conn} queue={peek} onClose={() => setPeek(null)} />}
    </Stack>
  );
}
