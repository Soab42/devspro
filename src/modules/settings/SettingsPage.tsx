import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export default function SettingsPage() {
  const [pingResult, setPingResult] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function testElevation() {
    setBusy(true);
    setPingResult(null);
    try {
      const result = await invoke<string>("elevate_ping");
      setPingResult(`Success: ${result}`);
    } catch (e) {
      setPingResult(`Failed: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <h1>Settings</h1>
      <section className="card">
        <div className="card-header">
          <h2>Elevation helper</h2>
        </div>
        <p className="muted">
          Round-trips a harmless Ping through the real OS elevation prompt (pkexec on
          Linux) to confirm the manifest → helper → result pipeline works before any
          module relies on it for real system changes.
        </p>
        <div className="form-row">
          <button onClick={testElevation} disabled={busy}>
            {busy ? "Waiting for prompt…" : "Test elevation"}
          </button>
        </div>
        {pingResult && <p className="muted">{pingResult}</p>}
      </section>
    </div>
  );
}
