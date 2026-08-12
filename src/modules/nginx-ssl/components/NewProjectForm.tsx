import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Button,
  Callout,
  Field,
  Form,
  FormRow,
  Mono,
  Stack,
  TextInput,
} from "../../../ui";
import { scaffoldCreateLaravelProject } from "../api";

function slugForHostname(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export default function NewProjectForm({
  onCreated,
  onCancel,
}: {
  onCreated: () => void;
  onCancel: () => void;
}) {
  const [projectName, setProjectName] = useState("");
  const [parentDir, setParentDir] = useState("");
  const [hostname, setHostname] = useState("");
  const [hostnameTouched, setHostnameTouched] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function handleNameChange(value: string) {
    setProjectName(value);
    if (!hostnameTouched) {
      const slug = slugForHostname(value);
      setHostname(slug ? `${slug}.test` : "");
    }
  }

  async function handleBrowseFolder() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Choose a parent folder for the new project",
    });
    if (!selected || Array.isArray(selected)) return;
    setParentDir(selected);
  }

  async function handleSubmit() {
    setError(null);
    if (!projectName.trim() || !parentDir || !hostname.trim()) {
      setError("Project name, parent folder, and hostname are all required.");
      return;
    }
    setBusy(true);
    try {
      await scaffoldCreateLaravelProject(parentDir, projectName.trim(), hostname.trim());
      onCreated();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Form onSubmit={handleSubmit}>
      <Stack gap={5}>
        <Callout tone="info">
          Creates a fresh Laravel project via Composer, then wires it up as an nginx + SSL
          site — no separate "add site" step needed.
        </Callout>

        <Field label="Project name" htmlFor="new-project-name" required>
          <TextInput
            id="new-project-name"
            placeholder="myapp"
            value={projectName}
            onChange={(e) => handleNameChange(e.target.value)}
            autoFocus
          />
        </Field>

        <Field label="Parent folder" required>
          <Stack gap={3}>
            <FormRow>
              <Button variant="secondary" onClick={handleBrowseFolder}>
                Choose folder…
              </Button>
            </FormRow>
            {parentDir && <Mono truncate>{parentDir}</Mono>}
          </Stack>
        </Field>

        <Field
          label="Hostname"
          htmlFor="new-project-hostname"
          required
          hint="Derived from the project name until you edit it."
        >
          <TextInput
            id="new-project-hostname"
            mono
            placeholder="myapp.test"
            value={hostname}
            onChange={(e) => {
              setHostnameTouched(true);
              setHostname(e.target.value);
            }}
          />
        </Field>

        {error && <Callout tone="danger">{error}</Callout>}

        <FormRow className="form-actions">
          {/* Composer install is genuinely slow; the hint says so rather than
              hiding it in a swapped button label. */}
          {busy && <span className="u-muted">This can take a minute…</span>}
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={busy}>
            Create project
          </Button>
        </FormRow>
      </Stack>
    </Form>
  );
}
