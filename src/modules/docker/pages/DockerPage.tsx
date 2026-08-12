import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import StatusPill from "../../../components/StatusPill";
import LogsModal from "../components/LogsModal";
import PortsModal from "../components/PortsModal";
import { useConfirm } from "../../../app/ConfirmContext";
import { Container, detectDocker, DockerDetection, listContainers, restartContainer, startContainer, stopContainer } from "../api";
import { parsePortMappings } from "../utils";

type StateFilter = "all" | "running" | "stopped";
type SortKey = "name" | "image" | "state" | "status";

export default function DockerPage() {
  const confirmAction = useConfirm();
  const [detection, setDetection] = useState<DockerDetection | null>(null);
  const [containers, setContainers] = useState<Container[]>([]);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [logsFor, setLogsFor] = useState<Container | null>(null);
  const [portsFor, setPortsFor] = useState<Container | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [stateFilter, setStateFilter] = useState<StateFilter>("all");
  const [sortKey, setSortKey] = useState<SortKey>("name");
  const [sortDir, setSortDir] = useState<"asc" | "desc">("asc");

  async function refresh() {
    const d = await detectDocker();
    setDetection(d);
    if (d.found && d.daemon_reachable) {
      try {
        setContainers(await listContainers());
      } catch (e) {
        setError(String(e));
      }
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function handleAction(action: (id: string) => Promise<string>, id: string, verb: string, name: string) {
    const ok = await confirmAction(`${verb} "${name}"?`, { confirmLabel: verb });
    if (!ok) return;
    setBusyId(id);
    setError(null);
    try {
      await action(id);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }

  function toggleSort(key: SortKey) {
    if (sortKey === key) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(key);
      setSortDir("asc");
    }
  }

  function sortIndicator(key: SortKey) {
    if (sortKey !== key) return null;
    return sortDir === "asc" ? " ▲" : " ▼";
  }

  const stopped = containers.filter((c) => c.state !== "running");
  const runningCount = containers.filter((c) => c.state === "running").length;
  const stoppedCount = containers.length - runningCount;

  const query = search.trim().toLowerCase();
  const filtered = containers.filter((c) => {
    if (stateFilter === "running" && c.state !== "running") return false;
    if (stateFilter === "stopped" && c.state === "running") return false;
    if (!query) return true;
    return (
      c.name.toLowerCase().includes(query) ||
      c.image.toLowerCase().includes(query) ||
      c.status.toLowerCase().includes(query)
    );
  });

  const visible = [...filtered].sort((a, b) => {
    const cmp = a[sortKey].localeCompare(b[sortKey]);
    return sortDir === "asc" ? cmp : -cmp;
  });

  if (!detection?.found) {
    return (
      <div>
        <h1>Docker</h1>
        <section className="card">
          <div className="card-header">
            <h2>Docker</h2>
            <StatusPill ok={false} label="not found" />
          </div>
          <p className="muted">
            Docker isn't installed on this machine. Install it from{" "}
            <a href="https://docs.docker.com/engine/install/" target="_blank" rel="noreferrer">
              docs.docker.com
            </a>{" "}
            and recheck.
          </p>
          <button className="secondary" onClick={refresh}>
            Recheck
          </button>
        </section>
      </div>
    );
  }

  if (!detection.daemon_reachable) {
    return (
      <div>
        <h1>Docker</h1>
        <section className="card">
          <div className="card-header">
            <h2>Docker</h2>
            <StatusPill ok={false} label="daemon unreachable" />
          </div>
          <p className="muted">{detection.error ?? "Could not reach the Docker daemon."}</p>
          <p className="muted">
            If this mentions permission denied, make sure your user is in the <code>docker</code> group (
            <code>sudo usermod -aG docker $USER</code>, then log out and back in) — this app never runs Docker
            commands elevated.
          </p>
          <button className="secondary" onClick={refresh}>
            Recheck
          </button>
        </section>
      </div>
    );
  }

  return (
    <div>
      <h1>Docker</h1>

      {stopped.length > 0 && (
        <section className="card">
          <div className="card-header">
            <h2>Needs attention</h2>
            <StatusPill ok={false} label={`${stopped.length} not running`} />
          </div>
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>Name</th>
                  <th>State</th>
                  <th>Status</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {stopped.map((c) => (
                  <tr key={c.id}>
                    <td>{c.name}</td>
                    <td>
                      <StatusPill ok={false} label={c.state} />
                    </td>
                    <td className="wrap">{c.status}</td>
                    <td>
                      <button
                        className="icon-button"
                        disabled={busyId === c.id}
                        onClick={() => handleAction(startContainer, c.id, "Start", c.name)}
                      >
                        {busyId === c.id ? "Starting…" : "Start"}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      )}

      <section className="card">
        <div className="card-header">
          <h2>Containers</h2>
          <StatusPill ok label={`${detection.version ?? ""}`} />
          <div className="topbar-spacer" />
          <button className="secondary" onClick={refresh}>
            Refresh
          </button>
        </div>

        {error && <p className="field-error">{error}</p>}

        {containers.length > 0 && (
          <>
            <div className="form-row">
              <button
                type="button"
                className={stateFilter === "all" ? "icon-button" : "icon-button secondary"}
                onClick={() => setStateFilter("all")}
              >
                All ({containers.length})
              </button>
              <button
                type="button"
                className={stateFilter === "running" ? "icon-button" : "icon-button secondary"}
                onClick={() => setStateFilter("running")}
              >
                Running ({runningCount})
              </button>
              <button
                type="button"
                className={stateFilter === "stopped" ? "icon-button" : "icon-button secondary"}
                onClick={() => setStateFilter("stopped")}
              >
                Stopped ({stoppedCount})
              </button>
            </div>

            <div className="form-row">
              <input
                placeholder="Search by name, image, or status"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>
          </>
        )}

        {containers.length === 0 ? (
          <p className="empty-state">No containers found.</p>
        ) : visible.length === 0 ? (
          <p className="empty-state">No containers match the current filter.</p>
        ) : (
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th className="sortable" onClick={() => toggleSort("name")}>
                    Name{sortIndicator("name")}
                  </th>
                  <th className="sortable" onClick={() => toggleSort("image")}>
                    Image{sortIndicator("image")}
                  </th>
                  <th className="sortable" onClick={() => toggleSort("state")}>
                    State{sortIndicator("state")}
                  </th>
                  <th className="sortable" onClick={() => toggleSort("status")}>
                    Status{sortIndicator("status")}
                  </th>
                  <th>Ports</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {visible.map((c) => {
                  const running = c.state === "running";
                  const busy = busyId === c.id;
                  const mappings = parsePortMappings(c.ports);
                  return (
                    <tr key={c.id}>
                      <td>{c.name}</td>
                      <td className="wrap">{c.image}</td>
                      <td>
                        <StatusPill ok={running} label={c.state} />
                      </td>
                      <td className="wrap">{c.status}</td>
                      <td className="wrap">{c.ports || "—"}</td>
                      <td>
                        <div className="form-row" style={{ marginTop: 0 }}>
                          {running &&
                            mappings.map((m) => (
                              <button
                                key={`${m.containerPort}/${m.proto}`}
                                className="icon-button secondary"
                                onClick={() => openUrl(`http://localhost:${m.hostPort}`)}
                              >
                                Open :{m.hostPort}
                              </button>
                            ))}
                          {running ? (
                            <button
                              className="icon-button secondary"
                              disabled={busy}
                              onClick={() => handleAction(stopContainer, c.id, "Stop", c.name)}
                            >
                              {busy ? "Stopping…" : "Stop"}
                            </button>
                          ) : (
                            <button
                              className="icon-button"
                              disabled={busy}
                              onClick={() => handleAction(startContainer, c.id, "Start", c.name)}
                            >
                              {busy ? "Starting…" : "Start"}
                            </button>
                          )}
                          <button
                            className="icon-button secondary"
                            disabled={busy}
                            onClick={() => handleAction(restartContainer, c.id, "Restart", c.name)}
                          >
                            Restart
                          </button>
                          <button className="icon-button secondary" onClick={() => setLogsFor(c)}>
                            Logs
                          </button>
                          <button className="icon-button secondary" onClick={() => setPortsFor(c)}>
                            Ports
                          </button>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {logsFor && <LogsModal container={logsFor} onClose={() => setLogsFor(null)} />}
      {portsFor && (
        <PortsModal
          container={portsFor}
          onClose={() => setPortsFor(null)}
          onChanged={refresh}
        />
      )}
    </div>
  );
}
