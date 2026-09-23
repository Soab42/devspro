import { useState } from "react";
import {
  Button,
  Callout,
  Field,
  Form,
  FormGrid,
  FormRow,
  SegmentedControl,
  Stack,
  Switch,
  TextInput,
} from "../../../ui";
import { Monitor, MonitorUp } from "../../../ui/icons";
import { Protocol, RemoteDesktop, saveDesktop } from "../api";
import { DEFAULT_PORT, errorMessage, portForProtocol, validateDesktopForm } from "../utils";

export default function ConnectionForm({
  desktop,
  onSaved,
  onCancel,
}: {
  desktop?: RemoteDesktop;
  onSaved: (saved: RemoteDesktop) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(desktop?.name ?? "");
  const [protocol, setProtocol] = useState<Protocol>(desktop?.protocol ?? "vnc");
  const [host, setHost] = useState(desktop?.host ?? "");
  const [port, setPort] = useState(String(desktop?.port ?? DEFAULT_PORT.vnc));
  const [username, setUsername] = useState(desktop?.username ?? "");
  const [domain, setDomain] = useState(desktop?.domain ?? "");
  const [viewOnly, setViewOnly] = useState(desktop?.view_only ?? false);
  const [scale, setScale] = useState(desktop?.scale ?? true);
  const [fullscreen, setFullscreen] = useState(desktop?.fullscreen ?? false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function changeProtocol(next: Protocol) {
    setPort(portForProtocol(port, protocol, next));
    setProtocol(next);
  }

  async function handleSubmit() {
    const problem = validateDesktopForm({ name, protocol, host, port, username });
    if (problem) {
      setError(problem);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const saved = await saveDesktop(desktop?.id ?? null, {
        name: name.trim(),
        protocol,
        host: host.trim(),
        port: Number(port),
        username: username.trim(),
        domain: domain.trim(),
        view_only: viewOnly,
        scale,
        fullscreen,
      });
      onSaved(saved);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Form onSubmit={handleSubmit}>
      <Stack gap={5}>
        <Field label="Protocol" description={protocol === "vnc" ? "Viewed inside DevsPro." : "Opens in your system's RDP client."}>
          <SegmentedControl
            ariaLabel="Protocol"
            value={protocol}
            onChange={changeProtocol}
            options={[
              { value: "vnc", label: "VNC", icon: Monitor },
              { value: "rdp", label: "RDP", icon: MonitorUp },
            ]}
          />
        </Field>

        <Field label="Name" htmlFor="rd-name" required>
          <TextInput id="rd-name" placeholder="Office PC" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        </Field>

        <FormGrid columns={3}>
          <Field label="Host" htmlFor="rd-host" required>
            <TextInput id="rd-host" mono placeholder="192.168.1.20" value={host} onChange={(e) => setHost(e.target.value)} />
          </Field>
          <Field label="Port" htmlFor="rd-port" required>
            <TextInput id="rd-port" mono inputMode="numeric" value={port} onChange={(e) => setPort(e.target.value)} />
          </Field>
          <Field label="Username" htmlFor="rd-user" required={protocol === "rdp"}>
            <TextInput
              id="rd-user"
              mono
              placeholder={protocol === "rdp" ? "alice" : "optional"}
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
          </Field>
        </FormGrid>

        {protocol === "rdp" && (
          <Field label="Domain" htmlFor="rd-domain" description="Leave empty for a local account.">
            <TextInput id="rd-domain" mono placeholder="CORP" value={domain} onChange={(e) => setDomain(e.target.value)} />
          </Field>
        )}

        {protocol === "vnc" ? (
          <Stack gap={3}>
            <Switch
              id="rd-scale"
              label="Scale to fit"
              description="Shrink the remote screen to fit the viewer instead of scrolling it."
              checked={scale}
              onChange={setScale}
            />
            <Switch
              id="rd-view-only"
              label="View only"
              description="Watch the screen without sending any mouse or keyboard input."
              checked={viewOnly}
              onChange={setViewOnly}
            />
          </Stack>
        ) : (
          <Switch
            id="rd-fullscreen"
            label="Fullscreen"
            description="Ask the client for a fullscreen window instead of a resizable one."
            checked={fullscreen}
            onChange={setFullscreen}
          />
        )}

        <Callout tone="info">Passwords are never saved; you are asked for one each time you connect.</Callout>

        {error && <Callout tone="danger">{error}</Callout>}

        <FormRow className="form-actions">
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={busy}>
            {desktop ? "Save changes" : "Add connection"}
          </Button>
        </FormRow>
      </Stack>
    </Form>
  );
}
