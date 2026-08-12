import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Modal from "../../../components/Modal";
import { useConfirm } from "../../../app/ConfirmContext";
import { changePort, Container } from "../api";
import { parsePortMappings, PortMapping } from "../utils";

export default function PortsModal({
  container,
  onClose,
  onChanged,
}: {
  container: Container;
  onClose: () => void;
  onChanged: () => void;
}) {
  const confirmAction = useConfirm();
  const mappings = parsePortMappings(container.ports);
  const [values, setValues] = useState<Record<number, string>>(() =>
    Object.fromEntries(mappings.map((m) => [m.containerPort, String(m.hostPort)])),
  );
  const [busyPort, setBusyPort] = useState<number | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleSave(m: PortMapping) {
    const newHostPort = Number(values[m.containerPort]);
    if (!newHostPort || newHostPort === m.hostPort) return;
    const ok = await confirmAction(
      `Change ${container.name}'s port ${m.hostPort} -> ${newHostPort}? This recreates the container.`,
      { confirmLabel: "Change port", danger: true },
    );
    if (!ok) return;
    setBusyPort(m.containerPort);
    setError(null);
    setMessage(null);
    try {
      setMessage(await changePort(container.id, m.containerPort, newHostPort));
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyPort(null);
    }
  }

  return (
    <Modal title={`Ports — ${container.name}`} onClose={onClose}>
      {mappings.length === 0 ? (
        <p className="empty-state">No published ports to edit.</p>
      ) : (
        mappings.map((m) => (
          <div className="form-field" key={`${m.containerPort}/${m.proto}`}>
            <label>
              Container port {m.containerPort}/{m.proto}
            </label>
            <div className="form-row">
              <input
                value={values[m.containerPort] ?? ""}
                onChange={(e) => setValues((v) => ({ ...v, [m.containerPort]: e.target.value }))}
                style={{ maxWidth: "100px" }}
              />
              <button type="button" disabled={busyPort === m.containerPort} onClick={() => handleSave(m)}>
                {busyPort === m.containerPort ? "Applying…" : "Save"}
              </button>
              <button
                type="button"
                className="secondary"
                onClick={() => openUrl(`http://localhost:${m.hostPort}`)}
              >
                Open
              </button>
            </div>
          </div>
        ))
      )}
      <p className="muted">
        Only editable for containers managed by docker compose — this rewrites the <code>ports:</code> line in
        the compose file and recreates the container.
      </p>
      {message && <p className="muted">{message}</p>}
      {error && <p className="field-error">{error}</p>}
    </Modal>
  );
}
