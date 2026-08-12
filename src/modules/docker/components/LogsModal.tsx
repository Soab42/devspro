import { useEffect, useState } from "react";
import Modal from "../../../components/Modal";
import { Container, fetchLogs } from "../api";

export default function LogsModal({ container, onClose }: { container: Container; onClose: () => void }) {
  const [logs, setLogs] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;

    async function load() {
      try {
        const text = await fetchLogs(container.id, 300);
        if (active) setLogs(text);
      } catch (e) {
        if (active) setError(String(e));
      }
    }

    load();
    const interval = setInterval(load, 3000);
    return () => {
      active = false;
      clearInterval(interval);
    };
  }, [container.id]);

  return (
    <Modal title={`Logs — ${container.name}`} onClose={onClose}>
      <p className="muted">Last 300 lines, refreshing every 3s.</p>
      {error && <p className="field-error">{error}</p>}
      <pre className="log-viewer">{logs || "(no output yet)"}</pre>
    </Modal>
  );
}
