import { useState } from "react";
import { Button, Callout, Field, Form, FormRow, Stack, TextInput } from "../../../ui";
import { HostsEntry, HostsView, addHostsEntry, updateHostsEntry } from "../api";
import { splitHostnames, validateEntry } from "../utils";

/**
 * Add or edit one entry. Hosted in a Modal but renders its own actions, so the
 * ordering mirrors ModalFooter: cancel, then primary. The primary triggers the
 * system password prompt; the hint says so, since the dialog appears outside
 * the app window and is easy to miss.
 */
export default function HostsEntryForm({
  entry,
  initialHostname,
  onSaved,
  onCancel,
}: {
  entry?: HostsEntry;
  /** Pre-fills the hostname for the "Add entry" shortcut on a missing site. */
  initialHostname?: string;
  onSaved: (view: HostsView) => void;
  onCancel: () => void;
}) {
  const [ip, setIp] = useState(entry?.ip ?? "127.0.0.1");
  const [hostnames, setHostnames] = useState(
    entry ? entry.hostnames.join(" ") : (initialHostname ?? ""),
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function handleSubmit() {
    setError(null);
    const names = splitHostnames(hostnames);
    const problem = validateEntry(ip, names);
    if (problem) {
      setError(problem);
      return;
    }
    setBusy(true);
    try {
      const view = entry
        ? await updateHostsEntry(entry.line_no, ip.trim(), names)
        : await addHostsEntry(ip.trim(), names[0]);
      onSaved(view);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Form onSubmit={handleSubmit}>
      <Stack gap={4}>
        {error && (
          <Callout tone="danger" onDismiss={() => setError(null)}>
            {error}
          </Callout>
        )}

        <Field label="IP address" htmlFor="hosts-ip" required>
          <TextInput
            id="hosts-ip"
            mono
            value={ip}
            onChange={(e) => setIp(e.target.value)}
            placeholder="127.0.0.1"
            autoFocus={Boolean(entry)}
          />
        </Field>

        <Field
          label={entry ? "Hostnames" : "Hostname"}
          htmlFor="hosts-names"
          required
          hint={
            entry
              ? "Separate several names with spaces."
              : "One name per entry, for example app.test."
          }
        >
          <TextInput
            id="hosts-names"
            mono
            value={hostnames}
            onChange={(e) => setHostnames(e.target.value)}
            placeholder="app.test"
            autoFocus={!entry}
          />
        </Field>

        <FormRow className="form-actions">
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={busy}>
            {entry ? "Save changes" : "Add entry"}
          </Button>
        </FormRow>
      </Stack>
    </Form>
  );
}
