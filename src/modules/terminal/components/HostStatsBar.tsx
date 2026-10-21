import { useEffect, useRef, useState } from "react";
import { Icon, meterSeverity } from "../../../ui";
import { Clock3, Cpu, HardDrive, MemoryStick, Server, Users, Wifi } from "../../../ui/icons";
import { formatKb, formatRate, formatUptime } from "../../dashboard/utils";
import { termHostStats, type HostSample } from "../../ssh/api";
import { deriveStats, type HostStats } from "../hostStats";
import type { TerminalSession } from "../workspace";

const POLL_MS = 2000;

/**
 * The strip under the terminal: the machine behind the focused pane, at a
 * glance. Local panes read this machine; remote panes read the host through
 * the file browser's SSH connection, so they show numbers once it is
 * connected and a hint until then.
 */
export default function HostStatsBar({ session, sftpId }: { session: TerminalSession | null; sftpId: string | null }) {
  const isLocal = session?.command === "local";
  const source = session ? (isLocal ? "local" : sftpId) : null;
  const [stats, setStats] = useState<HostStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const prev = useRef<{ sample: HostSample; at: number } | null>(null);

  useEffect(() => {
    prev.current = null;
    setStats(null);
    setError(null);
    if (source === null) return;
    let stopped = false;
    let inFlight = false;
    const tick = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const sample = await termHostStats(source === "local" ? null : source);
        if (stopped) return;
        const now = Date.now();
        const last = prev.current;
        setStats(deriveStats(last?.sample ?? null, sample, last ? (now - last.at) / 1000 : 0));
        prev.current = { sample, at: now };
        setError(null);
      } catch (e) {
        if (!stopped) setError(e instanceof Error ? e.message : String(e));
      } finally {
        inFlight = false;
      }
    };
    void tick();
    const timer = window.setInterval(() => void tick(), POLL_MS);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [source]);

  if (!session) return null;
  if (source === null) {
    return (
      <div className="term-stats term-stats-muted" aria-label="Host status">
        <Icon icon={Server} size="xs" /> Connect the file browser to see this host's load.
      </div>
    );
  }
  if (error) {
    return (
      <div className="term-stats term-stats-muted" aria-label="Host status" title={error}>
        <Icon icon={Server} size="xs" /> Host status unavailable
      </div>
    );
  }
  if (!stats) {
    return (
      <div className="term-stats term-stats-muted" aria-label="Host status">
        <Icon icon={Server} size="xs" /> Reading…
      </div>
    );
  }

  const cpu = stats.cpuPercent;
  return (
    <div className="term-stats" aria-label={`Status of ${stats.hostname}`}>
      <span className="term-stat term-stat-host" title="Host">
        <Icon icon={Server} size="xs" />
        {stats.hostname || "host"}
      </span>
      <span className={`term-stat term-stat-${cpu === null ? "good" : meterSeverity(cpu)}`} title={stats.load1 === null ? "CPU" : `CPU · load ${stats.load1.toFixed(2)}`}>
        <Icon icon={Cpu} size="xs" />
        {cpu === null ? "…" : `${Math.round(cpu)}%`}
      </span>
      <span className={`term-stat term-stat-${meterSeverity(stats.memPercent)}`} title={`Memory${stats.swapTotalKb ? ` · swap ${formatKb(stats.swapUsedKb)} / ${formatKb(stats.swapTotalKb)}` : ""}`}>
        <Icon icon={MemoryStick} size="xs" />
        {formatKb(stats.memUsedKb)} / {formatKb(stats.memTotalKb)}
      </span>
      <span className="term-stat" title="Network, received and sent">
        <Icon icon={Wifi} size="xs" />
        <span className="term-stat-arrow">↓</span>
        {stats.rxPerSec === null ? "…" : formatRate(stats.rxPerSec)}
        <span className="term-stat-arrow">↑</span>
        {stats.txPerSec === null ? "…" : formatRate(stats.txPerSec)}
      </span>
      <span className="term-stat" title="Uptime">
        <Icon icon={Clock3} size="xs" />
        {formatUptime(stats.uptimeSeconds)}
      </span>
      {/* Unknown for this machine on Windows; left out rather than shown as 0. */}
      {stats.users !== null && (
        <span className="term-stat" title="Logged-in sessions">
          <Icon icon={Users} size="xs" />
          {stats.users}
        </span>
      )}
      {stats.disks.slice(0, 6).map((d) => (
        <span key={d.mount} className={`term-stat term-stat-${meterSeverity(d.percent, 80, 92)}`} title={`${d.mount} · ${formatKb(d.used_kb)} of ${formatKb(d.total_kb)}`}>
          <Icon icon={HardDrive} size="xs" />
          {d.mount}: {Math.round(d.percent)}%
        </span>
      ))}
    </div>
  );
}
