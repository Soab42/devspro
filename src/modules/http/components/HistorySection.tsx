import { useConfirm } from "../../../app/ConfirmContext";
import { ActionGroup, Badge, Button, Column, DataTable, IconButton, Mono, Section } from "../../../ui";
import { Clock, Trash2 } from "../../../ui/icons";
import type { HttpRequestRecord } from "../api";
import { formatTime } from "../utils";

interface Props {
  records: HttpRequestRecord[];
  loading: boolean;
  onLoad: (record: HttpRequestRecord) => void;
  onDelete: (id: string) => Promise<void>;
  onClear: () => Promise<void>;
}

export default function HistorySection({ records, loading, onLoad, onDelete, onClear }: Props) {
  const confirmAction = useConfirm();

  async function clearAll() {
    const ok = await confirmAction(
      `All ${records.length} saved requests will be removed.`,
      { title: "Clear history?", confirmLabel: "Clear", danger: true },
    );
    if (ok) await onClear();
  }

  const columns: Column<HttpRequestRecord>[] = [
    {
      key: "method",
      header: "Method",
      priority: 1,
      width: "1px",
      render: (r) => (
        <Badge tone="accent" mono size="sm">
          {r.method}
        </Badge>
      ),
    },
    {
      key: "url",
      header: "URL",
      priority: 1,
      render: (r) => (
        <Mono truncate title={r.url}>
          {r.url}
        </Mono>
      ),
    },
    {
      key: "time",
      header: "Sent",
      priority: 2,
      sortValue: (r) => r.created_at,
      render: (r) => <span className="u-nowrap u-tnum">{formatTime(r.created_at)}</span>,
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (r) => (
        <ActionGroup>
          <IconButton
            icon={Trash2}
            label="Delete from history"
            size="sm"
            variant="ghost"
            onClick={(e) => {
              // The row itself loads the request; deleting must not also do that.
              e.stopPropagation();
              void onDelete(r.id);
            }}
          />
        </ActionGroup>
      ),
    },
  ];

  return (
    <Section
      title="History"
      icon={Clock}
      padding="none"
      collapsible
      defaultCollapsed={records.length === 0}
      meta={records.length > 0 ? <Badge tone="neutral">{records.length}</Badge> : undefined}
      actions={
        <Button variant="ghost" size="sm" icon={Trash2} disabled={records.length === 0} onClick={clearAll}>
          Clear
        </Button>
      }
    >
      <DataTable
        columns={columns}
        rows={records}
        rowKey={(r) => r.id}
        loading={loading}
        density="compact"
        onRowClick={onLoad}
        caption="Previously sent requests; click one to load it"
        empty={{
          icon: Clock,
          title: "No requests yet",
          description: "Every request you send is kept here so you can run it again.",
        }}
      />
    </Section>
  );
}
