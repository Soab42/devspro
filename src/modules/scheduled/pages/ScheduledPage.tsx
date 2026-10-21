import { useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import { listSites } from "../../nginx-ssl/api";
import LogsModal from "../components/LogsModal";
import TaskForm from "../components/TaskForm";
import {
  ActionGroup,
  Button,
  Callout,
  Column,
  DataTable,
  IconButton,
  Menu,
  Modal,
  Mono,
  Page,
  Section,
  Stack,
  StatusPill,
  Switch,
  statusOf,
  useResource,
} from "../../../ui";
import {
  CalendarClock,
  FileText,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Trash2,
} from "../../../ui/icons";
import {
  ScheduledTaskView,
  deleteScheduledTask,
  listScheduledTasks,
  runScheduledTaskNow,
  setScheduledTaskEnabled,
} from "../api";
import { ON_WINDOWS, SCHEDULER_NAME, formatAbsolute, formatRelative, lastOutputLine } from "../utils";

export default function ScheduledPage() {
  const confirmAction = useConfirm();
  const module = useModule("scheduled");

  // Polling keeps "next run" / "last run" honest: cron fires without telling
  // us, so the log mtime is the only signal that a run happened.
  const tasks = useResource(listScheduledTasks, { pollMs: 10000 });
  const sites = useResource(listSites);

  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [formFor, setFormFor] = useState<ScheduledTaskView | "new" | null>(null);
  const [logsFor, setLogsFor] = useState<ScheduledTaskView | null>(null);

  async function withBusy(task: ScheduledTaskView, action: () => Promise<unknown>) {
    setBusyId(task.id);
    setError(null);
    try {
      await action();
      await tasks.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyId(null);
    }
  }

  async function handleRunNow(task: ScheduledTaskView) {
    const ok = await confirmAction(
      `Runs "${task.name}" once, right now, the same way ${ON_WINDOWS ? "Task Scheduler" : "cron"} would. Output is appended to its log.`,
      { title: "Run now?", confirmLabel: "Run" },
    );
    if (!ok) return;
    await withBusy(task, async () => {
      await runScheduledTaskNow(task.id);
      setLogsFor(task);
    });
  }

  async function handleDelete(task: ScheduledTaskView) {
    const ok = await confirmAction(
      `"${task.name}" will be removed from ${ON_WINDOWS ? "Task Scheduler" : "your crontab"} and its log deleted.`,
      { title: "Delete scheduled task?", confirmLabel: "Delete", danger: true },
    );
    if (!ok) return;
    await withBusy(task, () => deleteScheduledTask(task.id));
  }

  const siteList = sites.data ?? [];
  const rows = tasks.data ?? [];
  const enabledCount = rows.filter((t) => t.enabled).length;
  const now = Date.now();

  const columns: Column<ScheduledTaskView>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      sortValue: (t) => t.name,
      render: (t) => (
        <div>
          <div className="dt-strong">{t.name}</div>
          <Mono truncate>{t.command}</Mono>
        </div>
      ),
    },
    {
      key: "schedule",
      header: "Schedule",
      priority: 2,
      sortValue: (t) => t.description,
      render: (t) => (
        <div>
          <div>{t.description}</div>
          <Mono variant="chip">{t.schedule}</Mono>
        </div>
      ),
    },
    {
      key: "next",
      header: "Next run",
      priority: 2,
      sortValue: (t) => (t.next_run ? Date.parse(t.next_run) : Number.MAX_SAFE_INTEGER),
      render: (t) =>
        t.enabled ? (
          <span title={formatAbsolute(t.next_run)}>{formatRelative(t.next_run, now)}</span>
        ) : (
          <StatusPill variant="neutral" label="paused" />
        ),
    },
    {
      key: "last",
      header: "Last run",
      priority: 3,
      sortValue: (t) => (t.last_run_at ? Date.parse(t.last_run_at) : 0),
      render: (t) => {
        const preview = lastOutputLine(t.last_output_tail);
        return (
          <div>
            <div title={formatAbsolute(t.last_run_at)}>{formatRelative(t.last_run_at, now)}</div>
            {preview && <Mono truncate>{preview}</Mono>}
          </div>
        );
      },
    },
    {
      key: "enabled",
      header: "Enabled",
      priority: 1,
      width: "1px",
      sortValue: (t) => (t.enabled ? 1 : 0),
      render: (t) => (
        <Switch
          checked={t.enabled}
          disabled={busyId === t.id}
          label={t.enabled ? "On" : "Off"}
          onChange={(next) => withBusy(t, () => setScheduledTaskEnabled(t.id, next))}
        />
      ),
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (t) => {
        const busy = busyId === t.id;
        return (
          <ActionGroup>
            <IconButton icon={Play} label="Run now" loading={busy} onClick={() => handleRunNow(t)} />
            <Menu
              items={[
                { key: "logs", label: "View output", icon: FileText, onClick: () => setLogsFor(t) },
                { key: "edit", label: "Edit", icon: Pencil, onClick: () => setFormFor(t) },
                {
                  key: "delete",
                  label: "Delete",
                  icon: Trash2,
                  danger: true,
                  separatorBefore: true,
                  disabled: busy,
                  onClick: () => handleDelete(t),
                },
              ]}
            />
          </ActionGroup>
        );
      },
    },
  ];

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(tasks)}
      meta={
        rows.length > 0 && (
          <StatusPill
            variant={enabledCount > 0 ? "ok" : "neutral"}
            label={`${enabledCount}/${rows.length} ${ON_WINDOWS ? "enabled" : "in crontab"}`}
          />
        )
      }
      actions={
        <>
          <Button
            variant="secondary"
            icon={RefreshCw}
            loading={tasks.refreshing}
            onClick={tasks.refresh}
          >
            Refresh
          </Button>
          <Button variant="primary" icon={Plus} onClick={() => setFormFor("new")}>
            New task
          </Button>
        </>
      }
    >
      <Stack gap={5}>
        {error && (
          <Callout tone="danger" onDismiss={() => setError(null)}>
            {error}
          </Callout>
        )}

        <Section
          title="Recurring commands"
          description={
            ON_WINDOWS
              ? "Tasks are registered with Windows Task Scheduler under a \\DevsPro\\ folder and run while you're signed in; your other scheduled tasks are left untouched."
              : `Enabled tasks are written to your user ${SCHEDULER_NAME} inside a block DevsPro manages; everything else in it is left untouched.`
          }
          padding="none"
          isEmpty={!tasks.loading && rows.length === 0}
          empty={{
            icon: CalendarClock,
            title: "No scheduled tasks yet",
            description: (
              <>
                A scheduled task is a command {ON_WINDOWS ? "Task Scheduler" : "cron"} runs for you on a timer — the canonical
                one is Laravel&apos;s <Mono variant="chip">php artisan schedule:run</Mono> every
                minute, but any shell command works.
              </>
            ),
            action: (
              <Button variant="primary" icon={Plus} onClick={() => setFormFor("new")}>
                New task
              </Button>
            ),
          }}
        >
          <DataTable
            columns={columns}
            rows={rows}
            rowKey={(t) => t.id}
            loading={tasks.loading}
            defaultSort={{ key: "name", dir: "asc" }}
            caption="Scheduled tasks"
          />
        </Section>
      </Stack>

      {formFor && (
        <Modal
          size="md"
          title={formFor === "new" ? "New scheduled task" : `Edit — ${formFor.name}`}
          icon={CalendarClock}
          onClose={() => setFormFor(null)}
        >
          <TaskForm
            task={formFor === "new" ? undefined : formFor}
            sites={siteList}
            onSaved={() => {
              setFormFor(null);
              void tasks.refresh();
            }}
            onCancel={() => setFormFor(null)}
          />
        </Modal>
      )}

      {logsFor && <LogsModal task={logsFor} onClose={() => setLogsFor(null)} />}
    </Page>
  );
}
