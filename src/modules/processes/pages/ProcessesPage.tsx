import { useEffect, useState } from "react";
import StatusPill from "../../../components/StatusPill";
import { useConfirm } from "../../../app/ConfirmContext";
import CommandModal from "../components/CommandModal";
import { killProcess, listPorts, PortEntry } from "../api";

const COMMAND_PREVIEW_LENGTH = 28;

type Category = "next" | "node" | "php" | "python" | "other" | "unowned";

const CATEGORY_LABELS: Record<Category, string> = {
  next: "Next.js",
  node: "Node.js",
  php: "PHP",
  python: "Python",
  other: "Other (mine)",
  unowned: "No owner shown",
};

// Checked in this order — a Next.js dev server IS a node process (its command
// line contains "next" as well as being launched by "node"), so the more
// specific match has to win before the generic "node" one does.
function categorize(p: PortEntry): Category {
  const haystack = `${p.process_name ?? ""} ${p.command ?? ""}`.toLowerCase();
  if (!p.pid) return "unowned";
  if (haystack.includes("next")) return "next";
  if (haystack.includes("node")) return "node";
  if (haystack.includes("php")) return "php";
  if (haystack.includes("python") || haystack.includes("gunicorn") || haystack.includes("uvicorn")) return "python";
  return "other";
}

export default function ProcessesPage() {
  const confirmAction = useConfirm();
  const [ports, setPorts] = useState<PortEntry[]>([]);
  const [filter, setFilter] = useState("");
  const [category, setCategory] = useState<Category | null>(null);
  const [ownerOnly, setOwnerOnly] = useState(true);
  const [commandFor, setCommandFor] = useState<string | null>(null);
  const [busyPid, setBusyPid] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  async function refresh() {
    setError(null);
    try {
      setPorts(await listPorts());
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function handleKill(entry: PortEntry, force: boolean) {
    if (!entry.pid) return;
    const label = entry.process_name ?? `pid ${entry.pid}`;
    const ok = await confirmAction(
      `${force ? "Force kill" : "Stop"} "${label}" (port ${entry.port}, pid ${entry.pid})?${
        force ? " This cannot be undone and may lose unsaved work in that process." : ""
      }`,
      { confirmLabel: force ? "Force kill" : "Stop", danger: true },
    );
    if (!ok) return;

    setBusyPid(entry.pid);
    setMessage(null);
    setError(null);
    try {
      setMessage(await killProcess(entry.pid, force));
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyPid(null);
    }
  }

  const query = filter.trim().toLowerCase();
  const ownerFiltered = ownerOnly ? ports.filter((p) => p.pid) : ports;
  const counts = ownerFiltered.reduce<Record<string, number>>((acc, p) => {
    const c = categorize(p);
    acc[c] = (acc[c] ?? 0) + 1;
    return acc;
  }, {});

  const visible = ownerFiltered.filter((p) => {
    if (category && categorize(p) !== category) return false;
    if (!query) return true;
    return (
      String(p.port).includes(query) ||
      (p.process_name ?? "").toLowerCase().includes(query) ||
      (p.command ?? "").toLowerCase().includes(query)
    );
  });

  return (
    <div>
      <h1>Processes</h1>
      <section className="card">
        <div className="card-header">
          <h2>Listening ports</h2>
          <div className="topbar-spacer" />
          <button className="secondary" onClick={refresh}>
            Refresh
          </button>
        </div>

        <div className="form-row">
          <label style={{ display: "flex", alignItems: "center", gap: "0.4em", fontWeight: 500 }}>
            <input
              type="checkbox"
              style={{ width: "auto" }}
              checked={ownerOnly}
              onChange={(e) => setOwnerOnly(e.target.checked)}
            />
            Owner only
          </label>
        </div>

        <div className="form-row">
          <button
            type="button"
            className={category === null ? "icon-button" : "icon-button secondary"}
            onClick={() => setCategory(null)}
          >
            All ({ownerFiltered.length})
          </button>
          {(Object.keys(CATEGORY_LABELS) as Category[])
            .filter((c) => counts[c])
            .map((c) => (
              <button
                key={c}
                type="button"
                className={category === c ? "icon-button" : "icon-button secondary"}
                onClick={() => setCategory((prev) => (prev === c ? null : c))}
              >
                {CATEGORY_LABELS[c]} ({counts[c]})
              </button>
            ))}
        </div>

        <div className="form-row">
          <input
            placeholder="Filter by port, name, or command (e.g. node, php, next)"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        </div>

        {message && <p className="muted">{message}</p>}
        {error && <p className="field-error">{error}</p>}

        {visible.length === 0 ? (
          <p className="empty-state">No matching listening ports.</p>
        ) : (
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>Port</th>
                  <th>Address</th>
                  <th>Process</th>
                  <th>PID</th>
                  <th>Command</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {visible.map((p) => {
                  const busy = busyPid === p.pid;
                  return (
                    <tr key={`${p.address}:${p.port}`}>
                      <td>{p.port}</td>
                      <td>{p.address}</td>
                      <td>
                        {p.process_name ? (
                          <StatusPill ok label={p.process_name} />
                        ) : (
                          <span className="muted">unknown</span>
                        )}
                      </td>
                      <td>{p.pid ?? "—"}</td>
                      <td>
                        {p.command ? (
                          <>
                            <span className="truncate-cell" title={p.command}>
                              {p.command}
                            </span>
                            {p.command.length > COMMAND_PREVIEW_LENGTH && (
                              <button className="ghost-button" onClick={() => setCommandFor(p.command)}>
                                See more
                              </button>
                            )}
                          </>
                        ) : (
                          "—"
                        )}
                      </td>
                      <td>
                        {p.pid && (
                          <div className="form-row" style={{ marginTop: 0 }}>
                            <button
                              className="icon-button secondary"
                              disabled={busy}
                              onClick={() => handleKill(p, false)}
                            >
                              {busy ? "Working…" : "Stop"}
                            </button>
                            <button className="icon-button danger" disabled={busy} onClick={() => handleKill(p, true)}>
                              Force kill
                            </button>
                          </div>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        <p className="muted" style={{ marginTop: "1em" }}>
          Only processes owned by your own user can be stopped from here — no elevation involved, on purpose.
          "Owner only" (on by default) hides everything else, like Docker's proxy for published container
          ports, which is managed from the Docker tab instead — turn it off to see those too.
        </p>
      </section>

      {commandFor && <CommandModal command={commandFor} onClose={() => setCommandFor(null)} />}
    </div>
  );
}
