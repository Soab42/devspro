import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import StatusPill from "../../../components/StatusPill";
import Modal from "../../../components/Modal";
import AddSiteForm from "../components/AddSiteForm";
import { useConfirm } from "../../../app/ConfirmContext";
import { fixLaravelPermissions, listSites, removeSite, setSiteEnabled, Site } from "../api";

function backendLabel(site: Site): string {
  return site.backend.type === "port" ? `:${site.backend.port}` : `Laravel — ${site.backend.project_path}`;
}

export default function SitesTab() {
  const confirmAction = useConfirm();
  const [sites, setSites] = useState<Site[]>([]);
  const [showAddModal, setShowAddModal] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    setSites(await listSites());
  }

  useEffect(() => {
    refresh();
  }, []);

  async function handleToggle(site: Site) {
    setError(null);
    try {
      await setSiteEnabled(site.id, !site.enabled);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleFixPermissions(site: Site) {
    setError(null);
    setMessage(null);
    try {
      setMessage(await fixLaravelPermissions(site.id));
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleRemove(site: Site) {
    const ok = await confirmAction(
      `Remove "${site.name}" (${site.hostname})? This deletes its nginx config and certificate.`,
      { confirmLabel: "Delete", danger: true },
    );
    if (!ok) return;
    setError(null);
    try {
      await removeSite(site.id);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div>
      <section className="card">
        <div className="card-header">
          <h2>Sites</h2>
          <div className="topbar-spacer" />
          <button onClick={() => setShowAddModal(true)}>+ Add site</button>
        </div>

        {message && <p className="muted">{message}</p>}
        {error && <p className="field-error">{error}</p>}

        {sites.length === 0 ? (
          <p className="empty-state">No sites yet — click "Add site" to get started.</p>
        ) : (
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Hostname</th>
                  <th>Backend</th>
                  <th>SSL expires</th>
                  <th>Status</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {sites.map((site) => (
                  <tr key={site.id}>
                    <td>{site.name}</td>
                    <td>{site.hostname}</td>
                    <td className="wrap">{backendLabel(site)}</td>
                    <td>{site.ssl_not_after ?? "—"}</td>
                    <td>
                      <StatusPill ok={site.enabled} label={site.enabled ? "enabled" : "disabled"} />
                    </td>
                    <td>
                      <div className="form-row" style={{ marginTop: 0 }}>
                        <button className="icon-button secondary" onClick={() => openUrl(`https://${site.hostname}`)}>
                          Open
                        </button>
                        <button className="icon-button secondary" onClick={() => handleToggle(site)}>
                          {site.enabled ? "Disable" : "Enable"}
                        </button>
                        {site.backend.type === "laravel" && (
                          <button className="icon-button secondary" onClick={() => handleFixPermissions(site)}>
                            Fix permissions
                          </button>
                        )}
                        <button className="icon-button danger" onClick={() => handleRemove(site)}>
                          Delete
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {showAddModal && (
        <Modal title="Add a site" onClose={() => setShowAddModal(false)}>
          <AddSiteForm
            onAdded={() => {
              setShowAddModal(false);
              refresh();
            }}
          />
        </Modal>
      )}
    </div>
  );
}
