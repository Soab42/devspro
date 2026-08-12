import { FormEvent, useState } from "react";
import { useAuth } from "./AuthContext";

export default function LockScreen() {
  const { profiles, unlock, error } = useAuth();
  const [profileId, setProfileId] = useState(profiles[0]?.id ?? "");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);

  const activeId = profileId || profiles[0]?.id;

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!activeId) return;
    setBusy(true);
    try {
      await unlock(activeId, password);
    } catch {
      // surfaced via context `error`
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="auth-screen">
      <form className="auth-card" onSubmit={handleSubmit}>
        <h1>Locked</h1>
        {profiles.length > 1 ? (
          <select value={activeId} onChange={(e) => setProfileId(e.target.value)}>
            {profiles.map((p) => (
              <option key={p.id} value={p.id}>
                {p.display_name}
              </option>
            ))}
          </select>
        ) : (
          <p className="auth-subtitle">{profiles[0]?.display_name}</p>
        )}
        <input
          type="password"
          placeholder="Password"
          autoFocus
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        {error && <p className="auth-error">{error}</p>}
        <button type="submit" disabled={busy}>
          {busy ? "Unlocking…" : "Unlock"}
        </button>
      </form>
    </div>
  );
}
