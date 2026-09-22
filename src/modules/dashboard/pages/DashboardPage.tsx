import { ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { useModule } from "../../../app/ModuleRegistry";
import {
  Badge,
  Callout,
  Card,
  EmptyState,
  Icon,
  Meter,
  Mono,
  Page,
  Section,
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
  Network,
  Server,
  ShieldCheck,
  TriangleAlert,
} from "../../../ui/icons";
import { fetchPortClaims, fetchSystemSnapshot, SystemSnapshot } from "../api";
import { Claimant, findPortConflicts, PortConflict } from "../portConflicts";
import { formatBytes, formatKb, formatRate, formatUptime, percent } from "../utils";
import "./DashboardPage.css";

const POLL_MS = 2000;
/* Port claims change on the order of seconds, not milliseconds, and the fetch
   spawns `ss` plus `docker ps`; polling it at the metrics rate would be waste. */
const PORT_POLL_MS = 10000;
const HISTORY_LENGTH = 30;

type Facet = "all" | "cpu" | "memory" | "gpu" | "storage";

const FACETS = [
  { value: "all" as Facet, label: "All" },
  { value: "cpu" as Facet, label: "CPU" },
  { value: "memory" as Facet, label: "Memory" },
  { value: "gpu" as Facet, label: "GPU" },
  { value: "storage" as Facet, label: "Storage" },
];

function pushHistory(history: number[], value: number): number[] {
  const next = [...history, value];
  return next.length > HISTORY_LENGTH ? next.slice(next.length - HISTORY_LENGTH) : next;
}

export default function DashboardPage() {
  const module = useModule("dashboard");
  const snapshot = useResource(fetchSystemSnapshot, { pollMs: POLL_MS });
  const claims = useResource(fetchPortClaims, { pollMs: PORT_POLL_MS });
  const conflicts = useMemo(
    () => (claims.data ? findPortConflicts(claims.data) : []),
    [claims.data],
  );
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
          {conflicts.length > 0 && <PortConflictsSection conflicts={conflicts} />}
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

/* -------------------------------------------------------------- conflicts -- */

const CLAIMANT_LINK: Record<Claimant["kind"], string> = {
  process: "/processes",
  container: "/docker",
  site: "/nginx-ssl",
};

/**
 * Rendered only when there is something to say. Sites all listen on 80/443,
 * so a container publishing 443 would otherwise list every site by name;
 * they are collapsed to a count and one link instead.
 */
function PortConflictsSection({ conflicts }: { conflicts: PortConflict[] }) {
  return (
    <Section
      title="Port conflicts"
      icon={Network}
      meta={<Badge tone="warning">{conflicts.length}</Badge>}
      description="The same host port is claimed by more than one owner. A stopped container or site on one of these ports will fail to start."
    >
      <ul className="dash-conflicts">
        {conflicts.map((c) => (
          <li key={`${c.port}/${c.proto}`} className="dash-conflict">
            <Mono className="dash-conflict-port">
              :{c.port}
              {c.proto === "udp" ? "/udp" : ""}
            </Mono>
            <div className="dash-conflict-claimants">
              {claimantRows(c.claimants).map((row) => (
                <Link key={row.key} to={row.to} className="dash-conflict-claimant">
                  <Badge tone={row.tone}>{row.kind}</Badge>
                  <span>{row.label}</span>
                  {row.detail && <span className="dash-conflict-detail">{row.detail}</span>}
                </Link>
              ))}
            </div>
          </li>
        ))}
      </ul>
    </Section>
  );
}

function claimantRows(claimants: Claimant[]) {
  const rows: { key: string; to: string; tone: "neutral" | "info" | "accent"; kind: string; label: string; detail?: string }[] = [];
  const sites = claimants.filter((c) => c.kind === "site");
  for (const c of claimants) {
    if (c.kind === "site") continue;
    rows.push({
      key: `${c.kind}:${c.label}:${c.detail ?? ""}`,
      to: CLAIMANT_LINK[c.kind],
      tone: c.kind === "process" ? "info" : "accent",
      kind: c.kind,
      label: c.label,
      detail: c.detail,
    });
  }
  if (sites.length === 1) {
    rows.push({ key: "site", to: CLAIMANT_LINK.site, tone: "neutral", kind: "site", label: sites[0].label });
  } else if (sites.length > 1) {
    rows.push({ key: "sites", to: CLAIMANT_LINK.site, tone: "neutral", kind: "sites", label: `${sites.length} nginx sites` });
  }
  return rows;
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
        /* Total throughput only. The per-interface table this used to head was
           removed: `eth0`/`wlp3s0` names are not what anyone is looking for, and
           the useful question — which *application* is using the network —
           cannot be answered from /proc. Linux counts bytes per interface, not
           per socket, so per-process rates need libpcap capture (what nethogs
           does) plus root. */
        sub="up and down, all interfaces"
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

