import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import StatusPill from "../../../components/StatusPill";
import Tabs from "../../../components/Tabs";
import Select from "../../../components/Select";
import { useConfirm } from "../../../app/ConfirmContext";
import { AppStatus, InstallOutcome, installBatch, listApps } from "../api";

const METHOD_LABEL: Record<AppStatus["method"], string> = {
  apt: "apt",
  npm: "npm",
  git: "git",
};

type StatusFilter = "all" | "installed" | "not_installed";

const STATUS_FILTERS: { id: StatusFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "installed", label: "Installed" },
  { id: "not_installed", label: "Not installed" },
];

export default function AppsPage() {
  const confirmAction = useConfirm();
  const [apps, setApps] = useState<AppStatus[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [outcomes, setOutcomes] = useState<Record<string, InstallOutcome>>({});
  const [error, setError] = useState<string | null>(null);
  const [activeCategory, setActiveCategory] = useState("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [query, setQuery] = useState("");
  const [versions, setVersions] = useState<Record<string, string>>({});

  async function refresh() {
    setError(null);
    try {
      const list = await listApps();
      setApps(list);
      setVersions((prev) => {
        const next = { ...prev };
        for (const a of list) {
          if (a.available_versions.length > 0 && !next[a.id]) {
            next[a.id] = a.available_versions[0];
          }
        }
        return next;
      });
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  async function handleInstall() {
    if (selected.size === 0) return;
    const names = apps.filter((a) => selected.has(a.id)).map((a) => a.name);
    const ok = await confirmAction(`Install ${names.length} tool${names.length === 1 ? "" : "s"}: ${names.join(", ")}?`, {
      confirmLabel: "Install",
    });
    if (!ok) return;

    setBusy(true);
    setError(null);
    try {
      const selections = Array.from(selected).map((id) => ({ id, version: versions[id] ?? null }));
      const results = await installBatch(selections);
      const map: Record<string, InstallOutcome> = {};
      for (const r of results) map[r.id] = r;
      setOutcomes(map);
      setSelected(new Set());
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const categories = Array.from(new Set(apps.map((a) => a.category)));
  const categoryTabs = [{ id: "all", label: "All" }, ...categories.map((c) => ({ id: c, label: c }))];

  const q = query.trim().toLowerCase();
  const visible = apps.filter((a) => {
    if (activeCategory !== "all" && a.category !== activeCategory) return false;
    if (statusFilter === "installed" && !a.installed) return false;
    if (statusFilter === "not_installed" && a.installed) return false;
    if (q && !a.name.toLowerCase().includes(q) && !a.description.toLowerCase().includes(q)) return false;
    return true;
  });

  const selectableVisibleIds = visible.filter((a) => !a.installed).map((a) => a.id);
  const allVisibleSelected = selectableVisibleIds.length > 0 && selectableVisibleIds.every((id) => selected.has(id));

  function toggleAllVisible() {
    setSelected((prev) => {
      const next = new Set(prev);
      if (allVisibleSelected) {
        selectableVisibleIds.forEach((id) => next.delete(id));
      } else {
        selectableVisibleIds.forEach((id) => next.add(id));
      }
      return next;
    });
  }

  return (
    <div>
      <h1>Apps</h1>
      <section className="card">
        <div className="card-header">
          <h2>Dev tools</h2>
          <div className="topbar-spacer" />
          <button className="secondary" onClick={refresh}>
            Refresh
          </button>
          <button onClick={handleInstall} disabled={busy || selected.size === 0}>
            {busy ? "Installing…" : `Install selected (${selected.size})`}
          </button>
        </div>
        <p className="muted">
          apt-based tools you select are installed together in one admin-password prompt. nvm and npm-based tools
          (Claude Code, Codex CLI) install for your own account only, no elevation.
        </p>

        <Tabs tabs={categoryTabs} active={activeCategory} onChange={setActiveCategory} />

        <div className="form-row">
          <input placeholder="Search by name or description…" value={query} onChange={(e) => setQuery(e.target.value)} />
        </div>
        <div className="form-row">
          {STATUS_FILTERS.map((f) => (
            <button
              key={f.id}
              type="button"
              className={statusFilter === f.id ? "icon-button" : "icon-button secondary"}
              onClick={() => setStatusFilter(f.id)}
            >
              {f.label}
            </button>
          ))}
        </div>

        {selectableVisibleIds.length > 0 && (
          <div className="form-row">
            <label style={{ display: "flex", alignItems: "center", gap: "0.4em" }}>
              <input type="checkbox" style={{ width: "auto" }} checked={allVisibleSelected} onChange={toggleAllVisible} />
              Select all installable {activeCategory !== "all" || q || statusFilter !== "all" ? "(visible)" : ""}
            </label>
          </div>
        )}
        {error && <p className="field-error">{error}</p>}

        {visible.length === 0 ? (
          <p className="empty-state">No tools match this filter.</p>
        ) : (
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th />
                  <th>Name</th>
                  <th>Category</th>
                  <th>Description</th>
                  <th>Version</th>
                  <th>Method</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                {visible.map((a) => {
                  const outcome = outcomes[a.id];
                  return (
                    <tr key={a.id}>
                      <td>
                        <input
                          type="checkbox"
                          style={{ width: "auto" }}
                          disabled={a.installed}
                          checked={selected.has(a.id)}
                          onChange={() => toggle(a.id)}
                        />
                      </td>
                      <td>{a.name}</td>
                      <td className="wrap">{a.category}</td>
                      <td className="wrap">
                        {a.description}
                        {outcome && (
                          <div className={outcome.success ? "muted" : "field-error"} style={{ marginTop: "0.3em" }}>
                            {outcome.message}
                          </div>
                        )}
                      </td>
                      <td style={{ minWidth: "120px" }}>
                        {a.available_versions.length > 0 ? (
                          <Select
                            value={versions[a.id] ?? a.available_versions[0]}
                            onChange={(v) => setVersions((prev) => ({ ...prev, [a.id]: v }))}
                            options={a.available_versions.map((v) => ({ value: v, label: v }))}
                          />
                        ) : (
                          <span className="muted">default</span>
                        )}
                      </td>
                      <td>{METHOD_LABEL[a.method]}</td>
                      <td>
                        <div className="form-row" style={{ marginTop: 0 }}>
                          <StatusPill ok={a.installed} label={a.installed ? "installed" : "not installed"} />
                          {!a.installed && a.homepage && (
                            <button className="icon-button secondary" onClick={() => openUrl(a.homepage!)}>
                              Learn more
                            </button>
                          )}
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
    </div>
  );
}
