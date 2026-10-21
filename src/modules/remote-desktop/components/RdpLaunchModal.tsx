import { useState } from "react";
import { Button, Callout, CodeBlock, Field, Form, FormRow, Modal, Select, Stack, TextInput, useResource } from "../../../ui";
import { ExternalLink, MonitorUp } from "../../../ui/icons";
import { RemoteDesktop, launchRdp, listRdpClients } from "../api";
import { RDP_INSTALL_HINTS, desktopLabel, errorMessage } from "../utils";

/**
 * Picks the installed RDP client, asks for the password, and launches. The
 * client is a separate window; once it's up this dialog's job is done and
 * the page reports how the client exits.
 */
export default function RdpLaunchModal({
  desktop,
  onLaunched,
  onClose,
}: {
  desktop: RemoteDesktop;
  onLaunched: (client: string) => void;
  onClose: () => void;
}) {
  const clients = useResource(listRdpClients);
  const [client, setClient] = useState<string>("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const found = clients.data ?? [];
  const chosen = client || found[0]?.id || "";
  const chosenClient = found.find((c) => c.id === chosen);
  const none = !clients.loading && found.length === 0;

  async function handleSubmit() {
    if (!chosen) return;
    setBusy(true);
    setError(null);
    try {
      const launched = await launchRdp(desktop.id, chosen, password);
      onLaunched(launched.client);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      size="sm"
      title={`Connect to ${desktop.name}`}
      description={`${desktopLabel(desktop)} opens in your RDP client's own window.`}
      icon={MonitorUp}
      onClose={onClose}
    >
      <Form onSubmit={handleSubmit}>
        <Stack gap={5}>
          {none ? (
            <Callout tone="warning" title="No RDP client installed">
              <Stack gap={3}>
                <span>Install FreeRDP (or Remmina), then try again. DevsPro looks for it on your PATH.</span>
                {RDP_INSTALL_HINTS.map((hint) => (
                  <Stack key={hint.label} gap={1}>
                    <span>{hint.label}</span>
                    <CodeBlock>{hint.command}</CodeBlock>
                  </Stack>
                ))}
              </Stack>
            </Callout>
          ) : (
            <>
              <Field label="Client" htmlFor="rd-rdp-client">
                <Select
                  id="rd-rdp-client"
                  value={chosen}
                  onChange={setClient}
                  options={found.map((c) => ({ value: c.id, label: `${c.label} — ${c.path}` }))}
                />
              </Field>
              <Field
                label="Password"
                htmlFor="rd-rdp-password"
                description={chosenClient?.password_note}
              >
                <TextInput
                  id="rd-rdp-password"
                  type="password"
                  autoComplete="off"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  autoFocus
                />
              </Field>
            </>
          )}

          {clients.error && <Callout tone="danger">{clients.error}</Callout>}
          {error && <Callout tone="danger">{error}</Callout>}

          <FormRow className="form-actions">
            <Button variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            {none ? (
              <Button variant="primary" loading={clients.refreshing} onClick={() => void clients.refresh()}>
                Check again
              </Button>
            ) : (
              <Button type="submit" variant="primary" icon={ExternalLink} loading={busy} disabled={!chosen}>
                Connect
              </Button>
            )}
          </FormRow>
        </Stack>
      </Form>
    </Modal>
  );
}
