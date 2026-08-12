import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  Button,
  Callout,
  EmptyState,
  Field,
  FormRow,
  Modal,
  ModalFooter,
  Mono,
  Stack,
  TextInput,
} from "../../../ui";
import { ExternalLink, Network } from "../../../ui/icons";
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
      <>
        <p>
          Port <Mono variant="chip">{m.hostPort}</Mono> becomes{" "}
          <Mono variant="chip">{newHostPort}</Mono> for {container.name}.
        </p>
        <p>This recreates the container.</p>
      </>,
      { title: "Change published port?", confirmLabel: "Change port", danger: true },
    );
    if (!ok) return;

    setBusyPort(m.containerPort);
    setError(null);
    setMessage(null);
    try {
      setMessage(await changePort(container.id, m.containerPort, newHostPort));
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyPort(null);
    }
  }

  return (
    <Modal
      size="md"
      title={`Ports — ${container.name}`}
      icon={Network}
      onClose={onClose}
      footer={
        <ModalFooter>
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={5}>
        {mappings.length === 0 ? (
          <EmptyState
            icon={Network}
            title="No published ports"
            description="This container doesn't publish any ports to the host."
          />
        ) : (
          mappings.map((m) => (
            <Field
              key={`${m.containerPort}/${m.proto}`}
              label={
                <>
                  Container port{" "}
                  <Mono>
                    {m.containerPort}/{m.proto}
                  </Mono>
                </>
              }
            >
              <FormRow>
                <TextInput
                  mono
                  inputMode="numeric"
                  value={values[m.containerPort] ?? ""}
                  onChange={(e) =>
                    setValues((v) => ({ ...v, [m.containerPort]: e.target.value }))
                  }
                  className="field-input-narrow"
                  aria-label={`Host port for container port ${m.containerPort}`}
                />
                <Button
                  variant="primary"
                  loading={busyPort === m.containerPort}
                  onClick={() => handleSave(m)}
                >
                  Save
                </Button>
                <Button
                  variant="secondary"
                  icon={ExternalLink}
                  onClick={() => void openUrl(`http://localhost:${m.hostPort}`)}
                >
                  Open
                </Button>
              </FormRow>
            </Field>
          ))
        )}

        {/* Load-bearing: explains why this is unavailable for plain `docker run`
            containers, and that saving recreates the container. */}
        <Callout tone="info">
          Only editable for containers managed by docker compose — this rewrites the{" "}
          <Mono variant="chip">ports:</Mono> line in the compose file and recreates the
          container.
        </Callout>

        {message && <Callout tone="success">{message}</Callout>}
        {error && <Callout tone="danger">{error}</Callout>}
      </Stack>
    </Modal>
  );
}
