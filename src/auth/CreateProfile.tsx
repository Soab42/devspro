import { FormEvent, useState } from "react";
import { useAuth } from "./AuthContext";

export default function CreateProfile() {
  const { createProfile, error } = useAuth();
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (password !== confirm) {
      setLocalError("Passwords do not match");
      return;
    }
    setLocalError(null);
    setBusy(true);
    try {
      await createProfile(displayName, password);
    } catch {
      // surfaced via context `error`
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="auth-screen">
      <form className="auth-card" onSubmit={handleSubmit}>
        <h1>Welcome</h1>
        <p className="auth-subtitle">Create a local profile to get started.</p>
        <input
          placeholder="Your name"
          value={displayName}
          onChange={(e) => setDisplayName(e.target.value)}
          autoFocus
        />
        <input
          type="password"
          placeholder="Password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <input
          type="password"
          placeholder="Confirm password"
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
        />
        {(localError ?? error) && <p className="auth-error">{localError ?? error}</p>}
        <button type="submit" disabled={busy}>
          {busy ? "Creating…" : "Create profile"}
        </button>
      </form>
    </div>
  );
}
