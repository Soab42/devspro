import { useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import { listSites } from "../../nginx-ssl/api";
import LogsModal from "../components/LogsModal";
import WorkerForm from "../components/WorkerForm";
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
  statusOf,
  useResource,
} from "../../../ui";
import {
  Pencil,
  Play,
  Plus,
  RefreshCw,
  RotateCw,
  ScrollText,
  Square,
  Trash2,
  Workflow,
} from "../../../ui/icons";
import {
  WorkerView,
  deleteWorker,
  listWorkers,
  restartWorker,
  startWorker,
  stopWorker,
} from "../api";

export default function WorkersPage() {
  const confirmAction = useConfirm();
  const module = useModule("workers");

  const workers = useResource(listWorkers, { pollMs: 5000 });
  const sites = useResource(listSites);

  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [formFor, setFormFor] = useState<WorkerView | "new" | null>(null);
  const [logsFor, setLogsFor] = useState<WorkerView | null>(null);

  async function handleLifecycle(
    action: (id: string) => Promise<WorkerView>,
    worker: WorkerView,
    verb: string,
  ) {
    const ok = await confirmAction(`This will ${verb.toLowerCase()} "${worker.name}".`, {
      title: `${verb} worker?`,
      confirmLabel: verb,
    });
    if (!ok) return;

    setBusyId(worker.id);
    setError(null);
    try {
      await action(worker.id);
      await workers.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyId(null);
    }
  }

  async function handleDelete(worker: WorkerView) {
    const ok = await confirmAction(
      `"${worker.name}" will be stopped if running, and its logs removed.`,
      { title: "Delete worker?", confirmLabel: "Delete", danger: true },
    );
    if (!ok) return;

    setBusyId(worker.id);
    setError(null);
    try {
      await deleteWorker(worker.id);
      await workers.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyId(null);
    }
  }

  const siteList = sites.data ?? [];
  const rows = workers.data ?? [];
  const runningCount = rows.filter((w) => w.running).length;

  function siteLabel(siteId: string | null): string {
    if (!siteId) return "—";
    return siteList.find((s) => s.id === siteId)?.name ?? "(removed site)";
  }

  const columns: Column<WorkerView>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      sortValue: (w) => w.name,
      render: (w) => <span className="dt-strong">{w.name}</span>,
    },
    {
      key: "status",
      header: "Status",
      priority: 1,
      sortValue: (w) => (w.running ? 1 : 0),
      render: (w) => (
        <StatusPill
          variant={w.running ? "ok" : "neutral"}
          label={w.running ? "running" : "stopped"}
        />
      ),
    },
    {
      key: "command",
      header: "Command",
      priority: 2,
      mono: true,
      sortValue: (w) => w.command,
      render: (w) => <Mono truncate>{w.command}</Mono>,
    },
    {
      key: "site",
      header: "Site",
      priority: 3,
      sortValue: (w) => siteLabel(w.site_id),
      render: (w) => siteLabel(w.site_id),
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (w) => {
        const busy = busyId === w.id;
        return (
          <ActionGroup>
            {w.running ? (
              <>
                <IconButton
                  icon={Square}
                  label="Stop"
                  loading={busy}
                  onClick={() => handleLifecycle(stopWorker, w, "Stop")}
                />
                <IconButton
                  icon={RotateCw}
                  label="Restart"
                  disabled={busy}
                  onClick={() => handleLifecycle(restartWorker, w, "Restart")}
                />
              </>
            ) : (
              <IconButton
                icon={Play}
                label="Start"
                loading={busy}
                onClick={() => handleLifecycle(startWorker, w, "Start")}
              />
            )}
            <Menu
              items={[
                {
                  key: "logs",
                  label: "View logs",
                  icon: ScrollText,
                  onClick: () => setLogsFor(w),
                },
                { key: "edit", label: "Edit", icon: Pencil, onClick: () => setFormFor(w) },
                {
                  key: "delete",
                  label: "Delete",
                  icon: Trash2,
                  /* Was styled the same as every other action, unlike every
                     other destructive control in the app. */
                  danger: true,
                  separatorBefore: true,
                  disabled: busy,
                  onClick: () => handleDelete(w),
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
      status={statusOf(workers)}
      meta={
        rows.length > 0 && (
          <StatusPill
            variant={runningCount > 0 ? "ok" : "neutral"}
            label={`${runningCount}/${rows.length} running`}
          />
        )
      }
      actions={
        <>
          <Button
            variant="secondary"
            icon={RefreshCw}
            loading={workers.refreshing}
            onClick={workers.refresh}
          >
            Refresh
          </Button>
          <Button variant="primary" icon={Plus} onClick={() => setFormFor("new")}>
            New worker
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
          title="Defined workers"
          padding="none"
          isEmpty={!workers.loading && rows.length === 0}
          empty={{
            icon: Workflow,
            title: "No workers yet",
            description: (
              <>
                A worker is any shell command you want kept running in the background —
                the canonical example is a Laravel queue worker (
                <Mono variant="chip">php artisan queue:work</Mono>), but it can be
                anything.
              </>
            ),
            action: (
              <Button variant="primary" icon={Plus} onClick={() => setFormFor("new")}>
                New worker
              </Button>
            ),
          }}
        >
          <DataTable
            columns={columns}
            rows={rows}
            rowKey={(w) => w.id}
            loading={workers.loading}
            defaultSort={{ key: "name", dir: "asc" }}
            caption="Background workers"
          />
        </Section>
      </Stack>

      {formFor && (
        <Modal
          size="md"
          title={formFor === "new" ? "New worker" : `Edit — ${formFor.name}`}
          icon={Workflow}
          onClose={() => setFormFor(null)}
        >
          <WorkerForm
            worker={formFor === "new" ? undefined : formFor}
            sites={siteList}
            onSaved={() => {
              setFormFor(null);
              void workers.refresh();
            }}
            onCancel={() => setFormFor(null)}
          />
        </Modal>
      )}

      {logsFor && <LogsModal worker={logsFor} onClose={() => setLogsFor(null)} />}
    </Page>
  );
}
