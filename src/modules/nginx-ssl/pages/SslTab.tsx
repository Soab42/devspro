import { useEffect, useState } from "react";
import StatusPill from "../../../components/StatusPill";
import { caEnsure, caInstall, caStatus, CaStatus } from "../api";

export default function SslTab() {
  const [ca, setCa] = useState<CaStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function refresh() {
    setCa(await caStatus());
  }

  useEffect(() => {
    refresh();
  }, []);

  async function handleCreate() {
    setBusy(true);
    setMessage(null);
    try {
      setCa(await caEnsure());
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleInstall() {
    setBusy(true);
    setMessage(null);
    try {
      setMessage(await caInstall());
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card">
      <div className="card-header">
        <h2>Local trusted CA</h2>
        {ca && <StatusPill ok={ca.exists} label={ca.exists ? "created" : "not created"} />}
      </div>

      {ca?.exists ? (
        <p className="muted">
          Expires {ca.not_after ?? "unknown"}. Install it into your system and browser trust stores so sites
          show a valid padlock.
        </p>
      ) : (
        <p className="muted">No local CA yet — created automatically the first time you add a site, or right now.</p>
      )}

      <div className="form-row">
        {!ca?.exists && (
          <button onClick={handleCreate} disabled={busy}>
            {busy ? "Working…" : "Create local CA"}
          </button>
        )}
        {ca?.exists && (
          <button onClick={handleInstall} disabled={busy}>
            {busy ? "Waiting for admin password…" : "Install into system trust store"}
          </button>
        )}
      </div>
      {message && <p className="muted">{message}</p>}
    </section>
  );
}
