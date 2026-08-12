import { ReactNode, useCallback, useEffect, useState } from "react";
import { useModule } from "../../../app/ModuleRegistry";
import {
  Badge,
  Callout,
  Card,
  Column,
  DataTable,
  EmptyState,
  Icon,
  Meter,
  Mono,
  Page,
  SegmentedControl,
  Sparkline,
  Stack,
  meterSeverity,
  statusOf,
  useResource,
} from "../../../ui";
import {
  Activity,
  Clock,
  Cpu as CpuIcon,
  Gpu,
  HardDrive,
  MemoryStick,
  Network as NetworkIcon,
  Server,
  ShieldCheck,
  TriangleAlert,
} from "../../../ui/icons";
import { fetchSystemSnapshot, NetworkInterfaceInfo, SystemSnapshot } from "../api";
import { formatBytes, formatKb, formatRate, formatUptime, percent } from "../utils";
import "./DashboardPage.css";

const POLL_MS = 2000;
const HISTORY_LENGTH = 30;

type Facet = "all" | "cpu" | "memory" | "gpu" | "storage" | "network";

const FACETS = [
  { value: "all" as Facet, label: "All" },
  { value: "cpu" as Facet, label: "CPU" },
  { value: "memory" as Facet, label: "Memory" },
  { value: "gpu" as Facet, label: "GPU" },
  { value: "storage" as Facet, label: "Storage" },
  { value: "network" as Facet, label: "Network" },
];

function pushHistory(history: number[], value: number): number[] {
  const next = [...history, value];
  return next.length > HISTORY_LENGTH ? next.slice(next.length - HISTORY_LENGTH) : next;
}

export default function DashboardPage() {
  const module = useModule("dashboard");
  const snapshot = useResource(fetchSystemSnapshot, { pollMs: POLL_MS });
  const [facet, setFacet] = useState<Facet>("all");

  const [cpuHistory, setCpuHistory] = useState<number[]>([]);
  const [memHistory, setMemHistory] = useState<number[]>([]);
  const [netHistory, setNetHistory] = useState<number[]>([]);

  const data = snapshot.data;

  useEffect(() => {
    if (!data) return;
    setCpuHistory((h) => pushHistory(h, data.cpu.usage_percent));
    setMemHistory((h) => pushHistory(h, percent(data.memory.used_kb, data.memory.total_kb)));
    setNetHistory((h) =>
      pushHistory(
        h,
        data.network
          .filter((n) => n.name !== "lo")
          .reduce((sum, n) => sum + n.rx_bytes_per_sec + n.tx_bytes_per_sec, 0),
      ),
    );
  }, [data]);

  const shows = useCallback(
    (id: Facet) => facet === "all" || facet === id,
    [facet],
  );

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(snapshot)}
      meta={
        data && (
          <>
            <Mono>{data.hostname || "unknown host"}</Mono>
            <span aria-hidden="true">·</span>
            <span>{data.os_name}</span>
            <span aria-hidden="true">·</span>
            <span className="dash-uptime">
              <Icon icon={Clock} size="xs" />
              up {formatUptime(data.uptime_seconds)}
            </span>
          </>
        )
      }
      actions={
        <SegmentedControl
          ariaLabel="Filter metrics"
          value={facet}
          onChange={setFacet}
          options={FACETS}
        />
      }
    >
      {data && (
        <Stack gap={5}>
          {/* A failed poll keeps the last good snapshot on screen. */}
          {snapshot.error && <Callout tone="warning">{snapshot.error}</Callout>}

          <HealthStrip snapshot={data} />
          <KpiRow
            snapshot={data}
            cpuHistory={cpuHistory}
            memHistory={memHistory}
            netHistory={netHistory}
          />

          <div className="dash-grid">
            {shows("cpu") && <CpuCard snapshot={data} />}
            {facet === "all" && <SystemCard snapshot={data} />}
            {shows("memory") && <MemoryCard snapshot={data} />}
            {shows("gpu") && <GpuCard snapshot={data} />}
            {shows("storage") && <StorageCard snapshot={data} />}
            {shows("network") && <NetworkCard snapshot={data} />}
          </div>
        </Stack>
      )}
    </Page>
  );
}

/* ---------------------------------------------------------------- health -- */

function healthChecks(s: SystemSnapshot) {
  const memPercent = percent(s.memory.used_kb, s.memory.total_kb);
  const primaryDisk = [...s.disks].sort((a, b) => b.total_bytes - a.total_bytes)[0];
  const diskPercent = primaryDisk ? percent(primaryDisk.used_bytes, primaryDisk.total_bytes) : 0;
  return [
    { label: "CPU", ok: meterSeverity(s.cpu.usage_percent) === "good" },
    { label: "Memory", ok: meterSeverity(memPercent) === "good" },
    { label: primaryDisk?.mount_point ?? "Disk", ok: meterSeverity(diskPercent) === "good" },
  ];
}

/**
 * Health was previously the last of six equal KPI tiles, which made the single
 * most important question on the page — "is anything wrong?" — the last thing
 * you read. It gets the full width and the top slot instead.
 */
function HealthStrip({ snapshot }: { snapshot: SystemSnapshot }) {
  const checks = healthChecks(snapshot);
  const underLoad = checks.filter((c) => !c.ok);
  const nominal = underLoad.length === 0;

  return (
    <div className={`dash-health dash-health-${nominal ? "ok" : "warn"}`}>
      <span className="dash-health-icon">
        <Icon icon={nominal ? ShieldCheck : TriangleAlert} size="md" />
      </span>
      <div className="dash-health-text">
        <p className="dash-health-title">
          {nominal ? "All systems nominal" : `${underLoad.length} under load`}
        </p>
        <p className="dash-health-sub">
          {nominal
            ? "CPU, memory and disk are all within normal range."
            : "Sustained pressure on the subsystems below."}
        </p>
      </div>
      <div className="dash-health-badges">
        {checks.map((c) => (
          <Badge key={c.label} tone={c.ok ? "success" : "warning"}>
            {c.label}
          </Badge>
        ))}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------- KPIs -- */

function KpiRow({
  snapshot,
  cpuHistory,
  memHistory,
  netHistory,
}: {
  snapshot: SystemSnapshot;
  cpuHistory: number[];
  memHistory: number[];
  netHistory: number[];
}) {
  const { cpu, memory, disks } = snapshot;
  const memPercent = percent(memory.used_kb, memory.total_kb);
  const primaryDisk = [...disks].sort((a, b) => b.total_bytes - a.total_bytes)[0];
  const diskPercent = primaryDisk ? percent(primaryDisk.used_bytes, primaryDisk.total_bytes) : 0;
  const activeInterfaces = snapshot.network.filter((n) => n.name !== "lo");
  const netThroughput = activeInterfaces.reduce(
    (sum, n) => sum + n.rx_bytes_per_sec + n.tx_bytes_per_sec,
    0,
  );

  return (
    <div className="dash-kpis">
      <Kpi
        icon={CpuIcon}
        label="CPU"
        value={`${Math.round(cpu.usage_percent)}%`}
        severity={meterSeverity(cpu.usage_percent)}
        sub={`${cpu.core_count} cores · load ${cpu.load_avg_1.toFixed(2)}`}
        history={cpuHistory}
        bounded
      />
      <Kpi
        icon={MemoryStick}
        label="Memory"
        value={`${Math.round(memPercent)}%`}
        severity={meterSeverity(memPercent)}
        sub={`${formatKb(memory.used_kb)} / ${formatKb(memory.total_kb)}`}
        history={memHistory}
        bounded
      />
      <Kpi
        icon={HardDrive}
        label={primaryDisk?.mount_point ?? "Disk"}
        value={`${Math.round(diskPercent)}%`}
        severity={meterSeverity(diskPercent)}
        sub={
          primaryDisk
            ? `${formatBytes(primaryDisk.used_bytes)} / ${formatBytes(primaryDisk.total_bytes)}`
            : "No disks found"
        }
        /* Disk has no meaningful history to plot, so it shows a meter instead
           of leaving a ragged gap where the other tiles have sparklines. */
        meter={diskPercent}
      />
      <Kpi
        icon={Activity}
        label="Network"
        value={formatRate(netThroughput)}
        sub={`${activeInterfaces.length} active ${
          activeInterfaces.length === 1 ? "interface" : "interfaces"
        }`}
        history={netHistory}
      />
    </div>
  );
}

function Kpi({
  icon,
  label,
  value,
  sub,
  severity = "good",
  history,
  meter,
  bounded = false,
}: {
  icon: typeof CpuIcon;
  label: string;
  value: string;
  sub: ReactNode;
  severity?: "good" | "warning" | "critical";
  history?: number[];
  meter?: number;
  bounded?: boolean;
}) {
  return (
    <div className="dash-kpi">
      <div className="dash-kpi-head">
        <Icon icon={icon} size="sm" />
        <span className="u-truncate">{label}</span>
      </div>
      <p className={`dash-kpi-value dash-kpi-${severity} u-tnum`}>{value}</p>
      {history && (
        <Sparkline
          data={history}
          color={
            severity === "good"
              ? "var(--color-accent)"
              : severity === "warning"
                ? "var(--color-warning)"
                : "var(--color-danger)"
          }
          max={bounded ? 100 : undefined}
          min={bounded ? 0 : undefined}
        />
      )}
      {meter !== undefined && <Meter percent={meter} variant="compact" />}
      <p className="dash-kpi-sub">{sub}</p>
    </div>
  );
}

/* ------------------------------------------------------------------ cards -- */

function CpuCard({ snapshot }: { snapshot: SystemSnapshot }) {
  const { cpu } = snapshot;
  return (
    <Card title="CPU" icon={CpuIcon} span={2} description={`${cpu.model} · ${cpu.core_count} ${cpu.core_count === 1 ? "core" : "cores"}`}>
      <Stack gap={4}>
        <Meter label="Overall" percent={cpu.usage_percent} />
        {cpu.per_core_percent.length > 1 && (
          <div className="dash-cores">
            {cpu.per_core_percent.map((p, i) => (
              <Meter key={i} variant="compact" label={`Core ${i}`} percent={p} />
            ))}
          </div>
        )}
      </Stack>
    </Card>
  );
}

function SystemCard({ snapshot }: { snapshot: SystemSnapshot }) {
  const { cpu } = snapshot;
  return (
    <Card title="System" icon={Server}>
      <div className="dash-stats">
        <Stat label="Hostname" value={snapshot.hostname || "—"} mono />
        <Stat label="OS" value={snapshot.os_name || "—"} sub={snapshot.os_version} />
        <Stat label="Kernel" value={snapshot.kernel_version || "—"} mono />
        <Stat
          label="Load avg"
          value={`${cpu.load_avg_1.toFixed(2)} / ${cpu.load_avg_5.toFixed(2)} / ${cpu.load_avg_15.toFixed(2)}`}
          sub="1m / 5m / 15m"
        />
      </div>
    </Card>
  );
}

function Stat({
  label,
  value,
  sub,
  mono,
}: {
  label: string;
  value: string;
  sub?: string | null;
  mono?: boolean;
}) {
  return (
    <div className="dash-stat">
      <span className="dash-stat-label">{label}</span>
      <span className={`dash-stat-value${mono ? " u-mono" : ""}`} title={value}>
        {value}
      </span>
      {sub && <span className="dash-stat-sub">{sub}</span>}
    </div>
  );
}

function MemoryCard({ snapshot }: { snapshot: SystemSnapshot }) {
  const { memory } = snapshot;
  const memPercent = percent(memory.used_kb, memory.total_kb);
  const swapPercent = percent(memory.swap_used_kb, memory.swap_total_kb);
  return (
    <Card title="Memory" icon={MemoryStick}>
      <Stack gap={4}>
        <Meter
          label="RAM"
          percent={memPercent}
          valueLabel={`${formatKb(memory.used_kb)} / ${formatKb(memory.total_kb)}`}
        />
        {memory.swap_total_kb > 0 && (
          <Meter
            label="Swap"
            percent={swapPercent}
            valueLabel={`${formatKb(memory.swap_used_kb)} / ${formatKb(memory.swap_total_kb)}`}
          />
        )}
      </Stack>
    </Card>
  );
}

function GpuCard({ snapshot }: { snapshot: SystemSnapshot }) {
  const { gpus } = snapshot;
  return (
    <Card title="GPU" icon={Gpu} padding={gpus.length === 0 ? "none" : "md"}>
      {gpus.length === 0 ? (
        <EmptyState icon={Gpu} title="No GPU detected" />
      ) : (
        <Stack gap={5}>
          {gpus.map((g, i) => (
            <Stack gap={3} key={i}>
              <p className="dash-gpu-name" title={g.name}>
                {g.name}
                {g.driver ? ` · ${g.driver}` : ""}
              </p>
              {g.usage_percent !== null ? (
                <Meter label="Usage" percent={g.usage_percent} />
              ) : (
                <p className="dash-note">
                  Live usage isn't exposed by this GPU's driver.
                </p>
              )}
              {g.memory_used_mb !== null && g.memory_total_mb !== null && (
                <Meter
                  label="Memory"
                  percent={percent(g.memory_used_mb, g.memory_total_mb)}
                  valueLabel={`${Math.round(g.memory_used_mb)} / ${Math.round(g.memory_total_mb)} MB`}
                />
              )}
              {g.temperature_c !== null && (
                <p className="dash-note">Temp: {g.temperature_c}°C</p>
              )}
            </Stack>
          ))}
        </Stack>
      )}
    </Card>
  );
}

function StorageCard({ snapshot }: { snapshot: SystemSnapshot }) {
  const { disks } = snapshot;
  return (
    <Card title="Storage" icon={HardDrive} padding={disks.length === 0 ? "none" : "md"}>
      {disks.length === 0 ? (
        <EmptyState icon={HardDrive} title="No mounted disks found" />
      ) : (
        <Stack gap={4}>
          {disks.map((d) => (
            <Meter
              key={d.mount_point}
              label={d.mount_point}
              percent={percent(d.used_bytes, d.total_bytes)}
              valueLabel={`${formatBytes(d.used_bytes)} / ${formatBytes(d.total_bytes)}`}
            />
          ))}
        </Stack>
      )}
    </Card>
  );
}

const NETWORK_COLUMNS: Column<NetworkInterfaceInfo>[] = [
  {
    key: "name",
    header: "Interface",
    priority: 1,
    mono: true,
    sortValue: (n) => n.name,
    render: (n) => n.name,
  },
  {
    key: "down",
    header: "Down",
    priority: 1,
    align: "end",
    mono: true,
    sortValue: (n) => n.rx_bytes_per_sec,
    render: (n) => formatRate(n.rx_bytes_per_sec),
  },
  {
    key: "up",
    header: "Up",
    priority: 1,
    align: "end",
    mono: true,
    sortValue: (n) => n.tx_bytes_per_sec,
    render: (n) => formatRate(n.tx_bytes_per_sec),
  },
  {
    key: "rx",
    header: "Total received",
    priority: 2,
    align: "end",
    mono: true,
    sortValue: (n) => n.rx_bytes_total,
    render: (n) => formatBytes(n.rx_bytes_total),
  },
  {
    key: "tx",
    header: "Total sent",
    priority: 2,
    align: "end",
    mono: true,
    sortValue: (n) => n.tx_bytes_total,
    render: (n) => formatBytes(n.tx_bytes_total),
  },
];

function NetworkCard({ snapshot }: { snapshot: SystemSnapshot }) {
  const activeInterfaces = snapshot.network
    .filter((n) => n.name !== "lo")
    .sort(
      (a, b) =>
        b.rx_bytes_per_sec + b.tx_bytes_per_sec - (a.rx_bytes_per_sec + a.tx_bytes_per_sec),
    );

  return (
    <Card title="Network" icon={NetworkIcon} span={2} padding="none">
      <DataTable
        columns={NETWORK_COLUMNS}
        rows={activeInterfaces}
        rowKey={(n) => n.name}
        minWidth={520}
        caption="Network interfaces"
        empty={{ icon: NetworkIcon, title: "No network interfaces found" }}
      />
    </Card>
  );
}
