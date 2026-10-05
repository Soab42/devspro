import { useId, useMemo, useState } from "react";
import {
  Button,
  Callout,
  Checkbox,
  Column,
  DataTable,
  EmptyState,
  Field,
  Form,
  Icon,
  IconButton,
  Modal,
  ModalFooter,
  Mono,
  Spinner,
  Stack,
  TextInput,
  useResource,
} from "../../../ui";
import { ChevronDown, ChevronRight, CircleCheck, Lock, OctagonX } from "../../../ui/icons";
import { ResetSummary, ResetTarget, resetMemory, resetPreview } from "../api";
import { formatMb, groupTargets, processCount, TargetGroup, targetKey } from "../memory";

const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

function plural(n: number, one: string, many: string) {
  return n === 1 ? `1 ${one}` : `${n} ${many}`;
}

/** "Stop 143 processes and 2 services" — the button says exactly what it does. */
function actionLabel(processes: number, services: number, dropCaches: boolean): string {
  const parts = [
    processes > 0 && plural(processes, "process", "processes"),
    services > 0 && plural(services, "service", "services"),
  ].filter(Boolean);
  if (parts.length > 0) return `Stop ${parts.join(" and ")}`;
  return dropCaches ? "Drop cache and reclaim swap" : "Nothing selected";
}

/**
 * Stops everything the user runs except DevsPro and the desktop session. The
 * list comes from the backend, all ticked; the user unticks what should stay
 * and types YES. The backend re-checks every pid before it signals it.
 */
export default function ResetModal({
  onFinished,
  onClose,
}: {
  onFinished: () => void;
  onClose: () => void;
}) {
  const preview = useResource(resetPreview);
  // Stored as what is unticked, so everything starts ticked.
  const [unticked, setUnticked] = useState<Set<string>>(() => new Set());
  const [keptServices, setKeptServices] = useState<Set<string>>(() => new Set());
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [dropCaches, setDropCaches] = useState(true);
  const [typed, setTyped] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [summary, setSummary] = useState<ResetSummary | null>(null);
  const yesId = useId();

  const groups = useMemo(() => groupTargets(preview.data?.processes ?? []), [preview.data]);
  const processes = (preview.data?.processes ?? []).filter((t) => !unticked.has(targetKey(t)));
  const services = (preview.data?.services ?? []).filter((s) => !keptServices.has(s));
  const ram = processes.reduce((sum, t) => sum + t.ram_mb, 0);
  const needsAdmin = services.length > 0 || dropCaches;
  const confirmed = typed.trim().toUpperCase() === "YES";
  const nothing = processes.length === 0 && services.length === 0 && !dropCaches;
  const canRun = Boolean(preview.data) && confirmed && !nothing && !running;

  function toggle<T>(set: Set<T>, items: T[], on: boolean): Set<T> {
    const next = new Set(set);
    for (const item of items) {
      if (on) next.add(item);
      else next.delete(item);
    }
    return next;
  }

  async function run() {
    if (!canRun) return;
    setRunning(true);
    setError(null);
    try {
      setSummary(
        await resetMemory({
          processes: processes.map((t) => [t.pid, t.start_time]),
          services,
          drop_caches: dropCaches,
        }),
      );
      onFinished();
    } catch (e) {
      setError(messageOf(e));
    } finally {
      setRunning(false);
    }
  }

  if (summary) {
    return (
      <Modal
        title="Full reset"
        icon={OctagonX}
        tone="danger"
        size="md"
        onClose={onClose}
        footer={
          <ModalFooter>
            <Button variant="primary" onClick={onClose}>
              Done
            </Button>
          </ModalFooter>
        }
      >
        <Stack gap={3}>
          <Callout tone="success">Stopped {processCount(summary.killed)}.</Callout>
          {summary.failed.length > 0 && (
            <Callout tone="warning" title={`${plural(summary.failed.length, "process", "processes")} couldn't be stopped`}>
              <Stack gap={1}>
                {summary.failed.map((f) => (
                  <span key={f}>{f}</span>
                ))}
              </Stack>
            </Callout>
          )}
          {summary.admin.map((r, i) => (
            <Callout key={i} tone={r.success ? "success" : "danger"}>
              {r.message}
            </Callout>
          ))}
        </Stack>
      </Modal>
    );
  }

  const hint = running && needsAdmin
    ? "Waiting for your admin password…"
    : needsAdmin
      ? "Asks for your admin password."
      : undefined;

  return (
    <Modal
      title="Full reset"
      description="Stops every app you have open, so you can start again with free memory. DevsPro and your desktop session keep running."
      icon={OctagonX}
      tone="danger"
      size="lg"
      onClose={onClose}
      footer={
        <ModalFooter hint={hint}>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="danger-solid" disabled={!canRun} loading={running} onClick={() => void run()}>
            {actionLabel(processes.length, services.length, dropCaches)}
          </Button>
        </ModalFooter>
      }
    >
      {preview.error && !preview.data ? (
        <Callout
          tone="danger"
          title="Couldn't list what's running"
          actions={
            <Button size="sm" onClick={() => void preview.refresh()}>
              Try again
            </Button>
          }
        >
          {preview.error}
        </Callout>
      ) : !preview.data ? (
        <Stack direction="row" gap={3} align="center">
          <Spinner />
          <span className="u-muted">Listing what&apos;s running…</span>
        </Stack>
      ) : (
        <Form onSubmit={() => void run()}>
          <Stack gap={5}>
            {error && (
              <Callout tone="danger" onDismiss={() => setError(null)}>
                {error}
              </Callout>
            )}
            <Callout tone="warning">
              Unsaved work in the apps you leave ticked is lost. Untick anything that should keep
              running.
            </Callout>

            <Stack gap={3}>
              <h3>Apps and processes</h3>
              {groups.length === 0 ? (
                <EmptyState
                  icon={CircleCheck}
                  title="Nothing else is running"
                  description="Only DevsPro and your desktop session are left."
                />
              ) : (
                <Stack gap={2}>
                  {groups.map((g) => (
                    <GroupRow
                      key={g.key}
                      group={g}
                      unticked={unticked}
                      open={expanded.has(g.key)}
                      onToggleOpen={() =>
                        setExpanded((s) => toggle(s, [g.key], !s.has(g.key)))
                      }
                      onTick={(targets, on) =>
                        setUnticked((s) => toggle(s, targets.map(targetKey), !on))
                      }
                    />
                  ))}
                </Stack>
              )}
            </Stack>

            {preview.data.services.length > 0 && (
              <Stack gap={3}>
                <h3>Services</h3>
                <Stack gap={2}>
                  {preview.data.services.map((unit) => (
                    <Checkbox
                      key={unit}
                      label={unit}
                      adornment={<Icon icon={Lock} size="xs" />}
                      checked={!keptServices.has(unit)}
                      onChange={(on) => setKeptServices((s) => toggle(s, [unit], !on))}
                    />
                  ))}
                </Stack>
                <p className="u-muted">Stopping services asks for your admin password.</p>
              </Stack>
            )}

            <Stack gap={1}>
              <Checkbox
                label="Also drop cache and reclaim swap"
                checked={dropCaches}
                onChange={setDropCaches}
              />
              <p className="u-muted">
                Asks for your admin password. Swap is left alone if there isn&apos;t enough free
                RAM to take it back.
              </p>
            </Stack>

            <p>
              {processes.length > 0 || services.length > 0 ? (
                <>
                  This stops {plural(processes.length, "process", "processes")} using{" "}
                  {formatMb(ram)}
                  {services.length > 0 && ` and ${plural(services.length, "service", "services")}`}
                  {dropCaches && ", then drops the cache and reclaims swap"}.
                </>
              ) : dropCaches ? (
                "Nothing is stopped. This only drops the cache and reclaims swap."
              ) : (
                "Nothing is selected."
              )}
            </p>

            <Field label="Type YES to confirm" htmlFor={yesId}>
              <TextInput
                id={yesId}
                autoComplete="off"
                spellCheck={false}
                value={typed}
                onChange={(e) => setTyped(e.target.value)}
              />
            </Field>
          </Stack>
        </Form>
      )}
    </Modal>
  );
}

const processColumns = (
  unticked: Set<string>,
  onTick: (targets: ResetTarget[], on: boolean) => void,
): Column<ResetTarget>[] => [
  {
    key: "pick",
    header: "",
    headerLabel: "Stop",
    priority: 1,
    width: "1px",
    render: (t) => (
      <Checkbox
        hideLabel
        label={`Stop ${t.name} (${t.pid})`}
        checked={!unticked.has(targetKey(t))}
        onChange={(on) => onTick([t], on)}
      />
    ),
  },
  {
    key: "name",
    header: "Process",
    priority: 1,
    sortValue: (t) => t.name,
    render: (t) => t.name,
  },
  {
    key: "pid",
    header: "PID",
    priority: 2,
    mono: true,
    align: "end",
    sortValue: (t) => t.pid,
    render: (t) => t.pid,
  },
  {
    key: "ram",
    header: "RAM",
    priority: 2,
    align: "end",
    sortValue: (t) => t.ram_mb,
    render: (t) => <span className="u-tnum">{formatMb(t.ram_mb)}</span>,
  },
  {
    key: "command",
    header: "Command",
    priority: 3,
    render: (t) => (
      <Mono truncate title={t.command}>
        {t.command}
      </Mono>
    ),
  },
];

function GroupRow({
  group,
  unticked,
  open,
  onToggleOpen,
  onTick,
}: {
  group: TargetGroup;
  unticked: Set<string>;
  open: boolean;
  onToggleOpen: () => void;
  onTick: (targets: ResetTarget[], on: boolean) => void;
}) {
  const ticked = group.targets.filter((t) => !unticked.has(targetKey(t))).length;
  const all = ticked === group.targets.length;

  return (
    <Stack gap={2}>
      <Stack direction="row" gap={2} align="center">
        <IconButton
          icon={open ? ChevronDown : ChevronRight}
          size="sm"
          label={open ? `Hide ${group.label} processes` : `Show ${group.label} processes`}
          aria-expanded={open}
          onClick={onToggleOpen}
        />
        <Checkbox
          label={group.label}
          checked={all}
          indeterminate={ticked > 0 && !all}
          onChange={(on) => onTick(group.targets, on)}
        />
        <span className="u-muted u-tnum">
          {processCount(group.targets.length)} · {formatMb(group.ram_mb)}
        </span>
      </Stack>
      {open && (
        <DataTable
          columns={processColumns(unticked, onTick)}
          rows={group.targets}
          rowKey={targetKey}
          density="compact"
          defaultSort={{ key: "ram", dir: "desc" }}
          caption={`${group.label} processes`}
          minWidth={480}
        />
      )}
    </Stack>
  );
}
