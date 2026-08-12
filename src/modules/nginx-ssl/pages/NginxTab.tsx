import { useEffect, useState } from "react";
import StatusPill from "../../../components/StatusPill";
import { detectNginx, installNginx, NginxDetection } from "../api";

export default function NginxTab() {
  const [nginx, setNginx] = useState<NginxDetection | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function refresh() {
    setNginx(await detectNginx());
  }

  useEffect(() => {
    refresh();
  }, []);

  async function handleInstall() {
    setBusy(true);
    setMessage(null);
    try {
      setMessage(await installNginx());
      await refresh();
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card">
      <div className="card-header">
        <h2>Nginx</h2>
        {nginx && <StatusPill ok={nginx.found} label={nginx.found ? "detected" : "not found"} />}
      </div>

      {nginx?.found ? (
        <p className="muted">
          {nginx.version} — config at <code>{nginx.config_path ?? "unknown"}</code>
        </p>
      ) : (
        <p className="muted">
          Not found on this machine.
          {nginx?.linux_package_manager
            ? ` Click below to install it via ${nginx.linux_package_manager} (will prompt for your admin password).`
            : " Install it manually, then recheck — e.g. sudo apt install nginx on Debian/Ubuntu or brew install nginx on macOS."}
        </p>
      )}

      <div className="form-row">
        {!nginx?.found && nginx?.linux_package_manager && (
          <button onClick={handleInstall} disabled={busy}>
            {busy ? "Waiting for admin password…" : `Install nginx via ${nginx.linux_package_manager}`}
          </button>
        )}
        <button className="secondary" onClick={refresh}>
          Recheck
        </button>
      </div>
      {message && <p className="muted">{message}</p>}
    </section>
  );
}
