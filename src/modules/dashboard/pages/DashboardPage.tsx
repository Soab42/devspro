import { useEffect, useState } from "react";
import Meter, { meterSeverity } from "../../../components/Meter";
import Sparkline from "../../../components/Sparkline";
import Tabs from "../../../components/Tabs";
import { fetchSystemSnapshot, SystemSnapshot } from "../api";
import { formatBytes, formatKb, formatRate, formatUptime, percent } from "../utils";

const POLL_MS = 2000;
const HISTORY_LENGTH = 30;

const TABS = [
  { id: "overview", label: "Overview" },
  { id: "cpu", label: "CPU" },
  { id: "memory", label: "Memory" },
  { id: "gpu", label: "GPU" },
  { id: "storage", label: "Storage" },
  { id: "network", label: "Network" },
];

function pushHistory(history: number[], value: number): number[] {
  const next = [...history, value];
  return next.length > HISTORY_LENGTH ? next.slice(next.length - HISTORY_LENGTH) : next;
}

export default function DashboardPage() {
  const [snapshot, setSnapshot] = useState<SystemSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [cpuHistory, setCpuHistory] = useState<number[]>([]);
  const [memHistory, setMemHistory] = useState<number[]>([]);
  const [netHistory, setNetHistory] = useState<number[]>([]);
  const [activeTab, setActiveTab] = useState("overview");

  useEffect(() => {
    let cancelled = false;

    async function poll() {
      try {
        const data = await fetchSystemSnapshot();
        if (cancelled) return;
        setSnapshot(data);
        setError(null);
        setCpuHistory((h) => pushHistory(h, data.cpu.usage_percent));
        setMemHistory((h) => pushHistory(h, percent(data.memory.used_kb, data.memory.total_kb)));
        setNetHistory((h) =>
          pushHistory(
            h,
            data.network.filter((n) => n.name !== "lo").reduce((sum, n) => sum + n.rx_bytes_per_sec + n.tx_bytes_per_sec, 0),
          ),
        );
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    }

    poll();
    const interval = setInterval(poll, POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, []);

  if (!snapshot) {
    return (
      <div>
        <h1>Dashboard</h1>
        <section className="card">
          <p className="empty-state">{error ?? "Reading system stats…"}</p>
        </section>
      </div>
    );
  }

  const { cpu, memory, disks, network, gpus } = snapshot;
  const memPercent = percent(memory.used_kb, memory.total_kb);
  const swapPercent = percent(memory.swap_used_kb, memory.swap_total_kb);
  const activeInterfaces = network
    .filter((n) => n.name !== "lo")
    .sort((a, b) => b.rx_bytes_per_sec + b.tx_bytes_per_sec - (a.rx_bytes_per_sec + a.tx_bytes_per_sec));
  const netThroughput = activeInterfaces.reduce((sum, n) => sum + n.rx_bytes_per_sec + n.tx_bytes_per_sec, 0);
  const primaryDisk = [...disks].sort((a, b) => b.total_bytes - a.total_bytes)[0];
  const diskPercent = primaryDisk ? percent(primaryDisk.used_bytes, primaryDisk.total_bytes) : 0;

  const kpiClass = (p: number) => {
    const severity = meterSeverity(p);
    return severity === "good" ? "kpi-tile-value" : `kpi-tile-value ${severity}`;
  };

  const healthChecks = [
    { label: "CPU", ok: meterSeverity(cpu.usage_percent) === "good" },
    { label: "Memory", ok: meterSeverity(memPercent) === "good" },
    { label: primaryDisk?.mount_point ?? "Disk", ok: meterSeverity(diskPercent) === "good" },
  ];
  const underLoad = healthChecks.filter((c) => !c.ok);

  const cpuCard = (
    <section className="card span-2" key="cpu">
      <div className="card-header">
        <span className="card-icon">⚙️</span>
        <h2>CPU</h2>
      </div>
      <p className="muted" style={{ marginTop: 0 }} title={cpu.model}>
        {cpu.model} · {cpu.core_count} {cpu.core_count === 1 ? "core" : "cores"}
      </p>
      <Meter label="Overall" percent={cpu.usage_percent} />
      {cpu.per_core_percent.length > 1 && (
        <div className="core-grid">
          {cpu.per_core_percent.map((p, i) => (
            <div key={i}>
              <div className="core-meter-label">
                <span>Core {i}</span>
                <span>{Math.round(p)}%</span>
              </div>
              <div className="meter-track">
                <div className={`meter-fill meter-fill-${meterSeverity(p)}`} style={{ width: `${p}%` }} />
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );

  const systemCard = (
    <section className="card" key="system">
      <div className="card-header">
        <span className="card-icon">🖥️</span>
        <h2>System</h2>
      </div>
      <div className="stat-grid">
        <div className="stat-tile">
          <span className="stat-tile-label">Hostname</span>
          <span className="stat-tile-value" title={snapshot.hostname}>{snapshot.hostname || "—"}</span>
        </div>
        <div className="stat-tile">
          <span className="stat-tile-label">OS</span>
          <span className="stat-tile-value" title={snapshot.os_name}>{snapshot.os_name || "—"}</span>
          {snapshot.os_version && <span className="stat-tile-sub">{snapshot.os_version}</span>}
        </div>
        <div className="stat-tile">
          <span className="stat-tile-label">Kernel</span>
          <span className="stat-tile-value" title={snapshot.kernel_version}>{snapshot.kernel_version || "—"}</span>
        </div>
        <div className="stat-tile">
          <span className="stat-tile-label">Load avg</span>
          <span className="stat-tile-value">
            {cpu.load_avg_1.toFixed(2)} / {cpu.load_avg_5.toFixed(2)} / {cpu.load_avg_15.toFixed(2)}
          </span>
          <span className="stat-tile-sub">1m / 5m / 15m</span>
        </div>
      </div>
    </section>
  );

  const memoryCard = (
    <section className="card" key="memory">
      <div className="card-header">
        <span className="card-icon">🧠</span>
        <h2>Memory</h2>
      </div>
      <Meter label="RAM" percent={memPercent} valueLabel={`${formatKb(memory.used_kb)} / ${formatKb(memory.total_kb)}`} />
      {memory.swap_total_kb > 0 && (
        <Meter
          label="Swap"
          percent={swapPercent}
          valueLabel={`${formatKb(memory.swap_used_kb)} / ${formatKb(memory.swap_total_kb)}`}
        />
      )}
    </section>
  );

  const gpuCard = (
    <section className="card" key="gpu">
      <div className="card-header">
        <span className="card-icon">🎮</span>
        <h2>GPU</h2>
      </div>
      {gpus.length === 0 ? (
        <p className="empty-state">No GPU detected.</p>
      ) : (
        gpus.map((g, i) => (
          <div key={i} style={{ marginBottom: i < gpus.length - 1 ? "1em" : 0 }}>
            <p className="muted" style={{ marginTop: 0 }} title={g.name}>
              {g.name}
              {g.driver ? ` · ${g.driver}` : ""}
            </p>
            {g.usage_percent !== null ? (
              <Meter label="Usage" percent={g.usage_percent} />
            ) : (
              <p className="dashboard-note" style={{ marginTop: "0.4em" }}>
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
            {g.temperature_c !== null && <p className="stat-tile-sub">Temp: {g.temperature_c}°C</p>}
          </div>
        ))
      )}
    </section>
  );

  const storageCard = (
    <section className="card" key="storage">
      <div className="card-header">
        <span className="card-icon">💾</span>
        <h2>Storage</h2>
      </div>
      {disks.length === 0 ? (
        <p className="empty-state">No mounted disks found.</p>
      ) : (
        disks.map((d) => (
          <Meter
            key={d.mount_point}
            label={d.mount_point}
            percent={percent(d.used_bytes, d.total_bytes)}
            valueLabel={`${formatBytes(d.used_bytes)} / ${formatBytes(d.total_bytes)}`}
          />
        ))
      )}
    </section>
  );

  const networkCard = (
    <section className="card span-2" key="network">
      <div className="card-header">
        <span className="card-icon">📡</span>
        <h2>Network</h2>
      </div>
      {activeInterfaces.length === 0 ? (
        <p className="empty-state">No network interfaces found.</p>
      ) : (
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>Interface</th>
                <th>Down</th>
                <th>Up</th>
                <th>Total received</th>
                <th>Total sent</th>
              </tr>
            </thead>
            <tbody>
              {activeInterfaces.map((n) => (
                <tr key={n.name}>
                  <td>{n.name}</td>
                  <td>{formatRate(n.rx_bytes_per_sec)}</td>
                  <td>{formatRate(n.tx_bytes_per_sec)}</td>
                  <td className="wrap">{formatBytes(n.rx_bytes_total)}</td>
                  <td className="wrap">{formatBytes(n.tx_bytes_total)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );

  return (
    <div>
      <h1>Dashboard</h1>

      {error && <p className="field-error">{error}</p>}

      <div className="kpi-row">
        <div className="kpi-tile">
          <div className="kpi-tile-head">
            <span className="card-icon">⚙️</span> CPU
          </div>
          <div className="kpi-tile-body">
            <span className={kpiClass(cpu.usage_percent)}>{Math.round(cpu.usage_percent)}%</span>
          </div>
          <Sparkline data={cpuHistory} color="var(--primary)" max={100} min={0} />
          <span className="kpi-tile-sub">{cpu.core_count} cores · load {cpu.load_avg_1.toFixed(2)}</span>
        </div>

        <div className="kpi-tile">
          <div className="kpi-tile-head">
            <span className="card-icon">🧠</span> Memory
          </div>
          <div className="kpi-tile-body">
            <span className={kpiClass(memPercent)}>{Math.round(memPercent)}%</span>
          </div>
          <Sparkline data={memHistory} color="var(--primary)" max={100} min={0} />
          <span className="kpi-tile-sub">{formatKb(memory.used_kb)} / {formatKb(memory.total_kb)}</span>
        </div>

        <div className="kpi-tile">
          <div className="kpi-tile-head">
            <span className="card-icon">💾</span> {primaryDisk?.mount_point ?? "Disk"}
          </div>
          <div className="kpi-tile-body">
            <span className={kpiClass(diskPercent)}>{Math.round(diskPercent)}%</span>
          </div>
          <span className="kpi-tile-sub">
            {primaryDisk ? `${formatBytes(primaryDisk.used_bytes)} / ${formatBytes(primaryDisk.total_bytes)}` : "No disks found"}
          </span>
        </div>

        <div className="kpi-tile">
          <div className="kpi-tile-head">
            <span className="card-icon">📡</span> Network
          </div>
          <div className="kpi-tile-body">
            <span className="kpi-tile-value">{formatRate(netThroughput)}</span>
          </div>
          <Sparkline data={netHistory} color="var(--primary)" />
          <span className="kpi-tile-sub">{activeInterfaces.length} active {activeInterfaces.length === 1 ? "interface" : "interfaces"}</span>
        </div>

        <div className="kpi-tile">
          <div className="kpi-tile-head">
            <span className="card-icon">⏱️</span> Uptime
          </div>
          <div className="kpi-tile-body">
            <span className="kpi-tile-value">{formatUptime(snapshot.uptime_seconds)}</span>
          </div>
          <span className="kpi-tile-sub" title={snapshot.hostname}>{snapshot.hostname || "—"}</span>
        </div>

        <div className="kpi-tile">
          <div className="kpi-tile-head">
            <span className="card-icon">🚦</span> Health
          </div>
          <div className="kpi-tile-body">
            <span className={underLoad.length === 0 ? "kpi-tile-value" : "kpi-tile-value warning"}>
              {underLoad.length === 0 ? "All nominal" : `${underLoad.length} under load`}
            </span>
          </div>
          <span className="kpi-tile-sub">
            {underLoad.length === 0 ? "CPU, memory, disk" : underLoad.map((c) => c.label).join(", ")}
          </span>
        </div>
      </div>

      <Tabs tabs={TABS} active={activeTab} onChange={setActiveTab} />

      {activeTab === "overview" ? (
        <div className="dashboard-grid">
          {cpuCard}
          {systemCard}
          {memoryCard}
          {gpuCard}
          {storageCard}
          {networkCard}
        </div>
      ) : (
        <div className="dashboard-grid dashboard-grid-single">
          {activeTab === "cpu" && cpuCard}
          {activeTab === "memory" && memoryCard}
          {activeTab === "gpu" && gpuCard}
          {activeTab === "storage" && storageCard}
          {activeTab === "network" && networkCard}
        </div>
      )}
    </div>
  );
}
