import { useId } from "react";
import {
  Column,
  DataTable,
  Field,
  Section,
  Select,
  Skeleton,
  Stack,
  StatusPill,
  Switch,
} from "../../../ui";
import { ShieldCheck } from "../../../ui/icons";
import type { WatchdogConfig, WatchdogEvent } from "../api";
import { formatMb, formatWhen } from "../memory";

/** The range the page offers. The backend accepts 50–99, so a value set below
    80 some other way is still shown rather than silently replaced. */
function thresholdOptions(current: number | undefined) {
  const values = Array.from({ length: 20 }, (_, i) => 80 + i);
  if (current !== undefined && !values.includes(current)) values.unshift(current);
  return values.map((v) => ({ value: String(v), label: `${v}% used` }));
}

const columns: Column<WatchdogEvent>[] = [
  {
    key: "at",
    header: "Time",
    priority: 1,
    sortValue: (e) => e.at_ms,
    render: (e) => <span className="u-tnum">{formatWhen(e.at_ms)}</span>,
  },
  {
    key: "app",
    header: "App",
    priority: 1,
    sortValue: (e) => e.app.toLowerCase(),
    render: (e) => e.app,
  },
  {
    key: "ram",
    header: "RAM",
    priority: 2,
    align: "end",
    sortValue: (e) => e.ram_mb,
    render: (e) => <span className="u-tnum">{formatMb(e.ram_mb)}</span>,
  },
  {
    key: "ram_pct",
    header: "RAM %",
    priority: 2,
    align: "end",
    sortValue: (e) => e.ram_pct,
    render: (e) => <span className="u-tnum">{e.ram_pct}%</span>,
  },
  {
    key: "reason",
    header: "Reason",
    priority: 3,
    sortValue: (e) => e.reason,
    render: (e) => (e.reason === "idle" ? "Largest idle app" : "Largest app"),
  },
  {
    key: "result",
    header: "Result",
    priority: 1,
    sortValue: (e) => (e.ok ? 0 : 1),
    render: (e) =>
      e.ok ? (
        <StatusPill variant="ok" label="Closed" />
      ) : (
        <span className="dt-cell-inline" title={e.message ?? undefined}>
          <StatusPill variant="bad" label="Failed" />
          {e.message && <span className="u-muted u-truncate">{e.message}</span>}
        </span>
      ),
  },
];

export default function WatchdogSection({
  config,
  onChange,
  events,
}: {
  config: WatchdogConfig | null;
  onChange: (next: WatchdogConfig) => void;
  events: WatchdogEvent[] | null;
}) {
  const thresholdId = useId();

  return (
    <Section
      title="Watchdog"
      icon={ShieldCheck}
      description="Runs in the background, so it keeps working while the window is closed to the tray."
      collapsible
    >
      <Stack gap={5}>
        {config ? (
          <>
            <Switch
              checked={config.enabled}
              onChange={(enabled) => onChange({ ...config, enabled })}
              label="Close an app when memory runs out"
              description="Checks every 8 seconds. It never closes services, DevsPro or your desktop."
            />
            <Field label="Close an app at" htmlFor={thresholdId} inline>
              <Select
                id={thresholdId}
                aria-label="Close an app at"
                value={String(config.threshold_pct)}
                onChange={(v) => onChange({ ...config, threshold_pct: Number(v) })}
                options={thresholdOptions(config.threshold_pct)}
              />
            </Field>
            <Switch
              checked={config.prefer_idle}
              onChange={(prefer_idle) => onChange({ ...config, prefer_idle })}
              label="Close idle apps first"
              description="An app is idle after 16 seconds under 1% CPU. With this off, the largest app goes first."
            />
          </>
        ) : (
          <Skeleton height="6rem" />
        )}

        <Stack gap={3}>
          <h3>Recent actions</h3>
          <DataTable
            columns={columns}
            rows={events ?? []}
            rowKey={(e) => `${e.at_ms}:${e.app}`}
            loading={events === null}
            skeletonRows={2}
            density="compact"
            defaultSort={{ key: "at", dir: "desc" }}
            caption="Apps the watchdog closed"
            minWidth={480}
            empty={{
              icon: ShieldCheck,
              title: "Nothing closed yet",
              description: "When RAM use reaches the threshold, what the watchdog closes is listed here.",
            }}
          />
        </Stack>
      </Stack>
    </Section>
  );
}
