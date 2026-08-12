import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Button,
  Callout,
  Field,
  Form,
  FormRow,
  Select,
  Stack,
  TextInput,
} from "../../../ui";
import { Site } from "../../nginx-ssl/api";
import { WorkerView, createWorker, updateWorker } from "../api";

export default function WorkerForm({
  worker,
  sites,
  onSaved,
  onCancel,
}: {
  worker?: WorkerView;
  sites: Site[];
  onSaved: () => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(worker?.name ?? "");
  const [command, setCommand] = useState(worker?.command ?? "");
  const [workingDir, setWorkingDir] = useState(worker?.working_dir ?? "");
  const [siteId, setSiteId] = useState(worker?.site_id ?? "");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function handleSelectSite(nextId: string) {
    setSiteId(nextId);
    if (workingDir.trim()) return;
    const site = sites.find((s) => s.id === nextId);
    if (site?.backend.type === "laravel") {
      setWorkingDir(site.backend.project_path);
    }
  }

  async function handleBrowseFolder() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Select a working directory",
    });
    if (!selected || Array.isArray(selected)) return;
    setWorkingDir(selected);
  }

  async function handleSubmit() {
    setError(null);
    if (!name.trim() || !command.trim()) {
      setError("Name and command are both required.");
      return;
    }
    setBusy(true);
    try {
      const dir = workingDir.trim() || null;
      const site = siteId || null;
      if (worker) {
        await updateWorker(worker.id, name.trim(), command.trim(), dir, site);
      } else {
        await createWorker(name.trim(), command.trim(), dir, site);
      }
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Form onSubmit={handleSubmit}>
      <Stack gap={5}>
        <Field label="Name" htmlFor="worker-name" required>
          <TextInput
            id="worker-name"
            placeholder="Queue worker"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
          />
        </Field>

        <Field label="Command" htmlFor="worker-command" required>
          <TextInput
            id="worker-command"
            mono
            placeholder="php artisan queue:work --tries=3"
            value={command}
            onChange={(e) => setCommand(e.target.value)}
          />
        </Field>

        <Field
          label="Site"
          htmlFor="worker-site"
          description="Optional. Picking a Laravel site fills in its project path below."
        >
          <Select
            id="worker-site"
            value={siteId}
            onChange={handleSelectSite}
            options={[
              { value: "", label: "None — standalone" },
              ...sites.map((s) => ({
                value: s.id,
                label: `${s.name} (${s.hostname})`,
              })),
            ]}
          />
        </Field>

        <Field label="Working directory" htmlFor="worker-dir">
          <FormRow>
            <TextInput
              id="worker-dir"
              mono
              placeholder="/home/you/sites/myapp"
              value={workingDir}
              onChange={(e) => setWorkingDir(e.target.value)}
              className="u-flex-1"
            />
            <Button variant="secondary" onClick={handleBrowseFolder}>
              Choose folder…
            </Button>
          </FormRow>
        </Field>

        {error && <Callout tone="danger">{error}</Callout>}

        {/* This form is hosted in a Modal but renders its own actions, so the
            ordering here mirrors ModalFooter: cancel, then primary. */}
        <FormRow className="form-actions">
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={busy}>
            {worker ? "Save changes" : "Create worker"}
          </Button>
        </FormRow>
      </Stack>
    </Form>
  );
}
