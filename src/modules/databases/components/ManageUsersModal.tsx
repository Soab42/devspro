import { FormEvent, useEffect, useState } from "react";
import Modal from "../../../components/Modal";
import { useConfirm } from "../../../app/ConfirmContext";
import { createDatabaseUser, dropDatabaseUser, listDatabaseUsers } from "../api";
import { generatePassword } from "../utils";
import ChangePasswordModal from "./ChangePasswordModal";

export default function ManageUsersModal({ base, onClose }: { base: string; onClose: () => void }) {
  const confirmAction = useConfirm();
  const [users, setUsers] = useState<string[] | null>(null);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [busyUser, setBusyUser] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [changingPasswordFor, setChangingPasswordFor] = useState<string | null>(null);

  async function refresh() {
    setError(null);
    try {
      setUsers(await listDatabaseUsers(base));
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function handleCreate(e: FormEvent) {
    e.preventDefault();
    if (!username.trim() || !password.trim()) {
      setError("username and password are required");
      return;
    }
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      setMessage(await createDatabaseUser(base, username.trim(), password.trim()));
      setUsername("");
      setPassword("");
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleDrop(user: string) {
    const ok = await confirmAction(`Remove database user "${user}"? Anything still connecting with it will stop working.`, {
      confirmLabel: "Remove user",
      danger: true,
    });
    if (!ok) return;
    setBusyUser(user);
    setError(null);
    try {
      setMessage(await dropDatabaseUser(base, user));
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyUser(null);
    }
  }

  return (
    <Modal title={`Users — ${base}`} onClose={onClose}>
      {users === null ? (
        <p className="empty-state">Loading…</p>
      ) : users.length === 0 ? (
        <p className="empty-state">No extra users yet — the admin account is managed from "Reveal password".</p>
      ) : (
        <ul style={{ listStyle: "none", padding: 0, margin: 0 }}>
          {users.map((u) => (
            <li
              key={u}
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                padding: "0.4em 0",
                borderBottom: "1px solid var(--border)",
              }}
            >
              <span>{u}</span>
              <div className="form-row" style={{ marginTop: 0 }}>
                <button className="icon-button secondary" onClick={() => setChangingPasswordFor(u)}>
                  Change password
                </button>
                <button className="icon-button danger" disabled={busyUser === u} onClick={() => handleDrop(u)}>
                  {busyUser === u ? "Removing…" : "Remove"}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}

      <form onSubmit={handleCreate} style={{ marginTop: "1em" }}>
        <h3 style={{ fontSize: "0.9em", margin: "0 0 0.4em" }}>Add a user</h3>
        <div className="form-field">
          <label htmlFor="new-user-name">Username</label>
          <input id="new-user-name" value={username} onChange={(e) => setUsername(e.target.value)} />
        </div>
        <div className="form-field">
          <label htmlFor="new-user-password">Password</label>
          <div className="form-row">
            <input id="new-user-password" value={password} onChange={(e) => setPassword(e.target.value)} />
            <button type="button" className="secondary" onClick={() => setPassword(generatePassword())}>
              Generate
            </button>
          </div>
        </div>
        {error && <p className="field-error">{error}</p>}
        {message && <p className="muted">{message}</p>}
        <div className="form-row">
          <button type="submit" disabled={busy}>
            {busy ? "Creating…" : "Add user"}
          </button>
        </div>
      </form>

      {changingPasswordFor && (
        <ChangePasswordModal
          base={base}
          username={changingPasswordFor}
          onClose={() => setChangingPasswordFor(null)}
        />
      )}
    </Modal>
  );
}
