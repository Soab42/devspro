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
  Switch,
  TextInput,
} from "../../../ui";
import { Site } from "../../nginx-ssl/api";
import { ScheduledTaskView, saveScheduledTask } from "../api";
import { CRON_PRESETS, ON_WINDOWS, describeCron, presetForSchedule } from "../utils";

export default function TaskForm({
  task,
  sites,
  onSaved,
  onCancel,
}: {
  task?: ScheduledTaskView;
  sites: Site[];
  onSaved: () => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(task?.name ?? "");
  const [schedule, setSchedule] = useState(task?.schedule ?? "* * * * *");
  // The preset is derived from the expression on open, but tracked separately
  // afterwards so picking "Custom" keeps the current expression editable
  // rather than snapping back to whichever preset it happens to equal.
  const [preset, setPreset] = useState(() => presetForSchedule(task?.schedule ?? "* * * * *"));
  const [command, setCommand] = useState(task?.command ?? "");
  const [workingDir, setWorkingDir] = useState(task?.working_dir ?? "");
  const [siteId, setSiteId] = useState(task?.site_id ?? "");
  const [enabled, setEnabled] = useState(task?.enabled ?? true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const described = describeCron(schedule);

  function handleSelectPreset(next: string) {
    setPreset(next);
    const found = CRON_PRESETS.find((p) => p.value === next);
    if (found?.expr) setSchedule(found.expr);
  }

  function handleSelectSite(nextId: string) {
    setSiteId(nextId);
    const site = sites.find((s) => s.id === nextId);
    if (site?.backend.type === "laravel") {
      if (!workingDir.trim()) setWorkingDir(site.backend.project_path);
      if (!command.trim()) setCommand("php artisan schedule:run");
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
    if (!described.valid) {
      setError(`Schedule is invalid: ${described.text}`);
      return;
    }
    setBusy(true);
    try {
      await saveScheduledTask({
        id: task?.id ?? null,
        name: name.trim(),
        schedule,
        command: command.trim(),
        workingDir: workingDir.trim() || null,
        siteId: siteId || null,
        enabled,
      });
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
        <Field label="Name" htmlFor="task-name" required>
          <TextInput
            id="task-name"
            placeholder="Laravel scheduler"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
          />
        </Field>

        <Field
          label="Site"
          htmlFor="task-site"
          description="Optional. Picking a Laravel site fills in its project path and the scheduler command."
        >
          <Select
            id="task-site"
            value={siteId}
            onChange={handleSelectSite}
            options={[
              { value: "", label: "None — standalone" },
              ...sites.map((s) => ({ value: s.id, label: `${s.name} (${s.hostname})` })),
            ]}
          />
        </Field>

        <Field label="Command" htmlFor="task-command" required>
          <TextInput
            id="task-command"
            mono
            placeholder="php artisan schedule:run"
            value={command}
            onChange={(e) => setCommand(e.target.value)}
          />
        </Field>

        <Field label="Working directory" htmlFor="task-dir">
          <FormRow>
            <TextInput
              id="task-dir"
              mono
              placeholder={ON_WINDOWS ? "C:\\Users\\you\\sites\\myapp" : "/home/you/sites/myapp"}
              value={workingDir}
              onChange={(e) => setWorkingDir(e.target.value)}
              className="u-flex-1"
            />
            <Button variant="secondary" onClick={handleBrowseFolder}>
              Choose folder…
            </Button>
          </FormRow>
        </Field>

        <Field label="Schedule" htmlFor="task-preset" required>
          <Select id="task-preset" value={preset} onChange={handleSelectPreset} options={CRON_PRESETS} />
        </Field>

        <Field
          label="Cron expression"
          htmlFor="task-schedule"
          description={
            ON_WINDOWS
              ? "Minute, hour, day of month, month, weekday. Translated to Task Scheduler triggers; commands run with cmd."
              : "Minute, hour, day of month, month, weekday."
          }
          hint={described.text}
          error={described.valid ? undefined : "Invalid schedule"}
        >
          <TextInput
            id="task-schedule"
            mono
            placeholder="*/5 * * * *"
            value={schedule}
            disabled={preset !== "custom"}
            onChange={(e) => setSchedule(e.target.value)}
          />
        </Field>

        <Switch
          checked={enabled}
          onChange={setEnabled}
          label="Enabled"
          description={
            ON_WINDOWS
              ? "Registered with Task Scheduler when saved. Disabled tasks stay registered but switched off."
              : "Written to your crontab when saved. Disabled tasks are kept here but removed from cron."
          }
        />

        {error && <Callout tone="danger">{error}</Callout>}

        <FormRow className="form-actions">
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={busy}>
            {task ? "Save changes" : "Create task"}
          </Button>
        </FormRow>
      </Stack>
    </Form>
  );
}
