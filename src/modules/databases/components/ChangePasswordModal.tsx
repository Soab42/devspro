import { FormEvent, useState } from "react";
import Modal from "../../../components/Modal";
import { changeAdminPassword, changeUserPassword } from "../api";
import { generatePassword } from "../utils";

export default function ChangePasswordModal({
  base,
  username,
  onClose,
  onChanged,
}: {
  base: string;
  username?: string;
  onClose: () => void;
  onChanged?: () => void;
}) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!password.trim()) {
      setError("password is required");
      return;
    }
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const result = username
        ? await changeUserPassword(base, username, password.trim())
        : await changeAdminPassword(base, password.trim());
      setMessage(result);
      onChanged?.();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal title={`Change password — ${username ?? "admin"}`} onClose={onClose}>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label htmlFor="new-password">New password</label>
          <div className="form-row">
            <input id="new-password" value={password} onChange={(e) => setPassword(e.target.value)} autoFocus />
            <button type="button" className="secondary" onClick={() => setPassword(generatePassword())}>
              Generate
            </button>
          </div>
        </div>
        {!username && (
          <p className="muted">
            Updates the live database and rewrites DevSuite's compose file so future actions (Reveal password,
            Manage users) keep using the new password automatically.
          </p>
        )}
        {error && <p className="field-error">{error}</p>}
        {message && <p className="muted">{message}</p>}
        <div className="form-row">
          <button type="submit" disabled={busy}>
            {busy ? "Changing…" : "Change password"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
