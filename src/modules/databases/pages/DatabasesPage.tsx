import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import StatusPill from "../../../components/StatusPill";
import Modal from "../../../components/Modal";
import { useConfirm } from "../../../app/ConfirmContext";
import CreateDatabaseModal from "../components/CreateDatabaseModal";
import ManageUsersModal from "../components/ManageUsersModal";
import ChangePasswordModal from "../components/ChangePasswordModal";
import { Credentials, DevsuiteContainer, getDatabaseCredentials, listDatabases, removeDatabase } from "../api";
import { parsePortMappings } from "../../docker/utils";

interface Group {
  base: string;
  db?: DevsuiteContainer;
  ui?: DevsuiteContainer;
}

function inferKind(image: string): "mysql" | "postgres" | "mongo" | "redis" | null {
  if (image.startsWith("mysql")) return "mysql";
  if (image.startsWith("postgres")) return "postgres";
  if (image.startsWith("mongo")) return "mongo";
  if (image.startsWith("redis")) return "redis";
  return null;
}

function inferDefaultUsername(image: string): string | null {
  const kind = inferKind(image);
  if (kind === "postgres") return "postgres";
  if (kind === "mongo" || kind === "mysql") return "root";
  return null;
}

function groupContainers(containers: DevsuiteContainer[]): Group[] {
  const groups = new Map<string, Group>();
  for (const c of containers) {
    const isUi = c.name.endsWith("_ui");
    const base = c.name.replace(/^devsuite_/, "").replace(/_ui$/, "");
    const group = groups.get(base) ?? { base };
    if (isUi) group.ui = c;
    else group.db = c;
    groups.set(base, group);
  }
  return Array.from(groups.values());
}

export default function DatabasesPage() {
  const confirmAction = useConfirm();
  const [containers, setContainers] = useState<DevsuiteContainer[]>([]);
  const [showCreate, setShowCreate] = useState(false);
  const [busyBase, setBusyBase] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [credentialsFor, setCredentialsFor] = useState<{ base: string; creds: Credentials; fallbackUsername: string | null } | null>(
    null,
  );
  const [usersFor, setUsersFor] = useState<string | null>(null);
  const [changingAdminPasswordFor, setChangingAdminPasswordFor] = useState<string | null>(null);

  async function refresh() {
    setError(null);
    try {
      setContainers(await listDatabases());
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function handleReveal(g: Group) {
    setBusyBase(g.base);
    try {
      const creds = await getDatabaseCredentials(g.base);
      const fallbackUsername = g.db ? inferDefaultUsername(g.db.image) : null;
      setCredentialsFor({ base: g.base, creds, fallbackUsername });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyBase(null);
    }
  }

  // Adminer pre-fills server/username/db from its URL query string (its own
  // documented "permanent login" feature) but — by Adminer's own security
  // design — never accepts a password that way, so that part always has to be
  // typed once when the login page loads. Mongo/Redis's UIs connect via env
  // vars directly and don't show a login form at all, so this only applies to
  // MySQL/Postgres-backed entries.
  async function handleOpenUi(g: Group, hostPort: number) {
    const kind = g.db ? inferKind(g.db.image) : null;
    if (!g.db || (kind !== "mysql" && kind !== "postgres")) {
      openUrl(`http://localhost:${hostPort}`);
      return;
    }
    try {
      const creds = await getDatabaseCredentials(g.base);
      const username = creds.username ?? inferDefaultUsername(g.db.image) ?? "";
      const driverParam = kind === "postgres" ? "pgsql" : "server";
      const params = new URLSearchParams({ [driverParam]: g.base, username, db: g.base });
      openUrl(`http://localhost:${hostPort}/?${params.toString()}`);
    } catch {
      openUrl(`http://localhost:${hostPort}`);
    }
  }

  async function handleRemove(base: string, deleteData: boolean) {
    const ok = await confirmAction(
      deleteData
        ? `Permanently delete "${base}" and its data volume? This cannot be undone.`
        : `Remove "${base}"? The container(s) are removed but data on disk is kept.`,
      { confirmLabel: deleteData ? "Delete everything" : "Remove", danger: true },
    );
    if (!ok) return;
    setBusyBase(base);
    setError(null);
    try {
      await removeDatabase(base, deleteData);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyBase(null);
    }
  }

  const groups = groupContainers(containers);

  return (
    <div>
      <h1>Databases</h1>
      <section className="card">
        <div className="card-header">
          <h2>DevSuite space</h2>
          <div className="topbar-spacer" />
          <button className="secondary" onClick={refresh}>
            Refresh
          </button>
          <button onClick={() => setShowCreate(true)}>+ Create database</button>
        </div>

        <p className="muted">
          Managed in DevSuite's own docker-compose project ("devsuite") — separate from any other compose stacks
          you run yourself.
        </p>
        <p className="muted">
          For MySQL/Postgres, "Open" on the admin UI pre-fills the server, username, and database on Adminer's
          login page — but never the password, since Adminer deliberately never accepts one via URL or env var
          for security. Use "Reveal password" to copy it in, once per session. Mongo/Redis's UIs connect
          automatically with no login form.
        </p>

        {error && <p className="field-error">{error}</p>}

        {groups.length === 0 ? (
          <p className="empty-state">No databases yet — click "Create database" to get started.</p>
        ) : (
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Database</th>
                  <th>Admin UI</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {groups.map((g) => {
                  const busy = busyBase === g.base;
                  const dbMappings = g.db ? parsePortMappings(g.db.ports) : [];
                  const uiMappings = g.ui ? parsePortMappings(g.ui.ports) : [];
                  return (
                    <tr key={g.base}>
                      <td>{g.base}</td>
                      <td>
                        {g.db ? (
                          <div className="form-row" style={{ marginTop: 0 }}>
                            <StatusPill ok={g.db.state === "running"} label={g.db.state} />
                            {dbMappings.map((m) => (
                              <button
                                key={m.hostPort}
                                className="icon-button secondary"
                                onClick={() => openUrl(`http://localhost:${m.hostPort}`)}
                              >
                                :{m.hostPort}
                              </button>
                            ))}
                          </div>
                        ) : (
                          <span className="muted">—</span>
                        )}
                      </td>
                      <td>
                        {g.ui ? (
                          <div className="form-row" style={{ marginTop: 0 }}>
                            <StatusPill ok={g.ui.state === "running"} label={g.ui.state} />
                            {uiMappings.map((m) => (
                              <button
                                key={m.hostPort}
                                className="icon-button secondary"
                                onClick={() => handleOpenUi(g, m.hostPort)}
                              >
                                Open :{m.hostPort}
                              </button>
                            ))}
                          </div>
                        ) : (
                          <span className="muted">none</span>
                        )}
                      </td>
                      <td>
                        <div className="form-row" style={{ marginTop: 0 }}>
                          <button className="icon-button secondary" disabled={busy} onClick={() => handleReveal(g)}>
                            Reveal password
                          </button>
                          <button className="icon-button secondary" disabled={busy} onClick={() => setUsersFor(g.base)}>
                            Manage users
                          </button>
                          <button className="icon-button secondary" disabled={busy} onClick={() => handleRemove(g.base, false)}>
                            Remove
                          </button>
                          <button className="icon-button danger" disabled={busy} onClick={() => handleRemove(g.base, true)}>
                            Delete + data
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

      {showCreate && (
        <CreateDatabaseModal
          onClose={() => setShowCreate(false)}
          onCreated={refresh}
        />
      )}

      {usersFor && <ManageUsersModal base={usersFor} onClose={() => setUsersFor(null)} />}

      {credentialsFor && (
        <Modal title={`Credentials — ${credentialsFor.base}`} onClose={() => setCredentialsFor(null)}>
          {(() => {
            const username = credentialsFor.creds.username ?? credentialsFor.fallbackUsername;
            return (
              <>
                <div className="form-field">
                  <label>Username</label>
                  <input readOnly value={username ?? "(no username — Redis uses password only)"} />
                </div>
                <div className="form-field">
                  <label>Password</label>
                  {credentialsFor.creds.password ? (
                    <div className="form-row">
                      <input readOnly value={credentialsFor.creds.password} />
                      <button
                        onClick={async () => {
                          try {
                            await navigator.clipboard.writeText(credentialsFor.creds.password!);
                          } catch {
                            // clipboard unavailable — value is still visible to copy manually
                          }
                        }}
                      >
                        Copy
                      </button>
                    </div>
                  ) : (
                    <p className="empty-state">Could not find a password for this service.</p>
                  )}
                </div>
                <div className="form-row">
                  <button className="secondary" onClick={() => setChangingAdminPasswordFor(credentialsFor.base)}>
                    Change password
                  </button>
                </div>
              </>
            );
          })()}
        </Modal>
      )}

      {changingAdminPasswordFor && (
        <ChangePasswordModal
          base={changingAdminPasswordFor}
          onClose={() => setChangingAdminPasswordFor(null)}
          onChanged={async () => {
            const creds = await getDatabaseCredentials(changingAdminPasswordFor);
            setCredentialsFor((prev) => (prev ? { ...prev, creds } : prev));
          }}
        />
      )}
    </div>
  );
}
