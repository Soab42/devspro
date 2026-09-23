import {
  useCallback,
  useMemo,
  useState,
  lazy,
  Suspense,
  useDeferredValue,
} from "react";
import { GroupTable } from "../components/GroupTable";
import LoadComposeModal from "../components/LoadComposeModal";

/* Lazy: this modal pulls in CodeMirror, which roughly doubles the bundle. Behind
   a menu item nobody clicks on most visits, it has no business being in the
   initial load — the app is served from local disk, so the fetch when it is
   finally opened is imperceptible. */
const EditComposeModal = lazy(() => import("../components/EditComposeModal"));
import LogsModal from "../components/LogsModal";
import PortsModal from "../components/PortsModal";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Menu,
  Mono,
  Page,
  SearchInput,
  Section,
  SegmentedControl,
  Stack,
  StatusPill,
  Toolbar,
  statusOf,
  useResource,
} from "../../../ui";
import {
  ArrowUpCircle,
  ChevronsDownUp,
  ChevronsUpDown,
  Container as ContainerIcon,
  Download,
  FileUp,
  Layers,
  Pencil,
  Play,
  RefreshCw,
  RotateCw,
  SearchX,
  Square,
  Trash2,
} from "../../../ui/icons";
import {
  BulkAction,
  bulkAction,
  Container,
  ComposeAction,
  composeProjectAction,
  detectDocker,
  listContainers,
  restartContainer,
  startContainer,
  stopContainer,
} from "../api";
import { dockerBlockedStatus } from "../blocked";
import { listPorts } from "../../processes/api";
import {
  describeStartCollisions,
  startCollisions,
} from "../../dashboard/portConflicts";
import { ContainerGroup, groupContainers, summarizeOutcomes } from "../utils";

type StateFilter = "all" | "running" | "stopped";

export default function DockerPage() {
  const confirmAction = useConfirm();
  const module = useModule("docker");

  const detection = useResource(detectDocker);
  const dockerUsable = Boolean(
    detection.data?.found && detection.data.daemon_reachable,
  );
  const containers = useResource(listContainers, { enabled: dockerUsable });
  /* Listening sockets, for the "this port is already taken" warning shown
     before a start. Not polled: refreshed with the container list, which is
     the moment the answer can have changed. */
  const listening = useResource(listPorts, { enabled: dockerUsable });

  const [busyId, setBusyId] = useState<string | null>(null);
  const [bulkBusy, setBulkBusy] = useState(false);
  const [logsFor, setLogsFor] = useState<Container | null>(null);
  const [portsFor, setPortsFor] = useState<Container | null>(null);
  const [showLoadCompose, setShowLoadCompose] = useState(false);
  /* One piece of state for both entry points: the modal reads the same file
     either way, and `kind` is only what narrows the apply. */
  const [editCompose, setEditCompose] = useState<
    | { kind: "container"; id: string; name: string }
    | { kind: "project"; project: string }
    | null
  >(null);
  /* Action failures are shown inline, NOT through Page's status — routing them
     there would blank the whole table because one Stop button failed. */
  const [actionError, setActionError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  /* A port collision is a warning, never a block: the user may be about to
     stop the other process, or the container may bind a different address. */
  const [portWarning, setPortWarning] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [stateFilter, setStateFilter] = useState<StateFilter>("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  /* Stacks start folded: a machine with a handful of compose projects otherwise
     opens on several screens of table. Tracked as the set of *expanded* keys so
     a group that appears later (a stack just brought up, or one the filter
     brings back) is folded like the rest instead of springing open. */
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set());

  /* After an action, only the container list can have changed — whether Docker
     is installed and the daemon is up cannot have. Re-detecting there spawned a
     second `docker` process on every Start/Stop/Restart, adding its latency to
     the wait before the table caught up. Detection is refreshed by the toolbar's
     Refresh button, which is where "did the daemon come back?" is actually
     being asked. */
  const refreshAfterAction = useCallback(async () => {
    await Promise.all([containers.refresh(), listening.refresh()]);
  }, [containers.refresh, listening.refresh]);

  const refreshAll = useCallback(async () => {
    // Both in flight at once: neither depends on the other's result, and
    // `containers` is already gated on `enabled`.
    await Promise.all([
      detection.refresh(),
      containers.refresh(),
      listening.refresh(),
    ]);
  }, [detection.refresh, containers.refresh, listening.refresh]);

  const rows = useMemo(() => containers.data ?? [], [containers.data]);

  /* The filter runs against a deferred copy of the search text, so React keeps
     the input responsive and re-renders the tables at lower priority. Typing
     used to re-filter, re-group and re-render every table synchronously on each
     keystroke, which is what made the field feel like it was fighting back. */
  const query = useDeferredValue(search).trim().toLowerCase();
  const visible = useMemo(
    () =>
      rows.filter((c) => {
        if (stateFilter === "running" && c.state !== "running") return false;
        if (stateFilter === "stopped" && c.state === "running") return false;
        if (!query) return true;
        return (
          c.name.toLowerCase().includes(query) ||
          c.image.toLowerCase().includes(query) ||
          c.status.toLowerCase().includes(query) ||
          (c.compose_project ?? "").toLowerCase().includes(query)
        );
      }),
    [rows, stateFilter, query],
  );

  const groups = useMemo(() => groupContainers(visible), [visible]);

  /* Selection handed to each table as its own id list rather than the shared
     `Set`: a Set gets a new identity on every toggle, which would defeat
     GroupTable's memo for every group when one checkbox changed. */
  const selectedByGroup = useMemo(
    () =>
      new Map(
        groups.map((g) => [
          g.key,
          g.containers.filter((c) => selected.has(c.id)).map((c) => c.id),
        ]),
      ),
    [groups, selected],
  );

  /* Selection is keyed by container id and intersected with what's on screen at
     use time, so a container that disappears (or is filtered out) can never be
     acted on from a stale checkbox. */
  const selectedVisible = useMemo(
    () => visible.filter((c) => selected.has(c.id)),
    [visible, selected],
  );

  const anyExpanded = useMemo(
    () => groups.some((g) => expandedGroups.has(g.key)),
    [groups, expandedGroups],
  );

  const setGroupExpanded = useCallback((key: string, expanded: boolean) => {
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (expanded) next.add(key);
      else next.delete(key);
      return next;
    });
  }, []);

  /* One button for both directions: while anything is open it folds everything,
     and once everything is folded it becomes the way back. */
  const toggleAllGroups = useCallback(() => {
    setExpandedGroups(
      anyExpanded ? new Set() : new Set(groups.map((g) => g.key)),
    );
  }, [anyExpanded, groups]);

  const runningCount = rows.filter((c) => c.state === "running").length;
  const stoppedCount = rows.length - runningCount;

  const toggleSelected = useCallback((id: string, checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  }, []);

  const toggleGroup = useCallback((group: ContainerGroup, checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      for (const c of group.containers) {
        if (checked) next.add(c.id);
        else next.delete(c.id);
      }
      return next;
    });
  }, []);

  /* Names the processes sitting on the host ports of the stopped containers
     among `targets`, shows them as a warning Callout, and returns a sentence to
     append to the confirm body — or "" when nothing collides. */
  const warnPortCollisions = useCallback(
    (targets: Container[]): string => {
      const held = (listening.data ?? []).map((p) => ({
        port: p.port,
        pid: p.pid,
        process: p.process_name,
      }));
      const found = startCollisions(targets, held, rows);
      if (found.length === 0) {
        setPortWarning(null);
        return "";
      }
      const text = describeStartCollisions(found);
      setPortWarning(`Port already in use: ${text}.`);
      return ` Warning: ${text}.`;
    },
    [listening.data, rows],
  );

  const runAction = useCallback(
    async (
      action: (id: string) => Promise<string>,
      container: Container,
      verb: string,
    ) => {
      const warning = verb === "Start" ? warnPortCollisions([container]) : "";
      const ok = await confirmAction(
        `This will ${verb.toLowerCase()} the container "${container.name}".${warning}`,
        { title: `${verb} container?`, confirmLabel: verb },
      );
      if (!ok) return;

      setBusyId(container.id);
      setActionError(null);
      try {
        await action(container.id);
        await refreshAfterAction();
      } catch (e) {
        setActionError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusyId(null);
      }
    },
    [confirmAction, refreshAfterAction, warnPortCollisions],
  );

  /* The verb-keyed entry point GroupTable calls. A stable identity matters here:
     passed as an inline arrow it would change every render and take the memo
     down with it. */
  const handleLifecycle = useCallback(
    (action: "start" | "stop" | "restart", container: Container) => {
      const verbs = {
        start: [startContainer, "Start"],
        stop: [stopContainer, "Stop"],
        restart: [restartContainer, "Restart"],
      } as const;
      const [fn, verb] = verbs[action];
      void runAction(fn, container, verb);
    },
    [runAction],
  );

  async function runBulk(
    action: BulkAction,
    targets: Container[],
    label: string,
  ) {
    if (targets.length === 0) return;
    const warning = action === "start" ? warnPortCollisions(targets) : "";
    const ok = await confirmAction(
      `This will ${action} ${targets.length} container(s): ${targets
        .slice(0, 5)
        .map((c) => c.name)
        .join(
          ", ",
        )}${targets.length > 5 ? `, and ${targets.length - 5} more` : ""}.${warning}`,
      { title: `${label}?`, confirmLabel: label, danger: action === "stop" },
    );
    if (!ok) return;

    setBulkBusy(true);
    setActionError(null);
    setMessage(null);
    try {
      const outcomes = await bulkAction(
        action,
        targets.map((c) => c.id),
      );
      const past =
        action === "stop"
          ? "Stopped"
          : action === "start"
            ? "Started"
            : "Restarted";
      const summary = summarizeOutcomes(past, outcomes);
      if (summary.failed) setActionError(summary.message);
      else setMessage(summary.message);
      /* Only clear selection on a clean run — after a partial failure the
         selection is what the user needs in order to retry the stragglers. */
      if (!summary.failed) setSelected(new Set());
      await refreshAfterAction();
    } catch (e) {
      setActionError(e instanceof Error ? e.message : String(e));
    } finally {
      setBulkBusy(false);
    }
  }

  async function runComposeAction(
    group: ContainerGroup,
    action: ComposeAction,
    label: string,
  ) {
    const destructive = action === "down";
    const warning =
      action === "up" || action === "start"
        ? warnPortCollisions(group.containers)
        : "";
    const ok = await confirmAction(
      destructive
        ? `This removes the containers and network for "${group.label}". Named volumes — including database data — are kept.`
        : `This runs docker compose ${action} on the "${group.label}" stack.${warning}`,
      {
        title: `${label} ${group.label}?`,
        confirmLabel: label,
        danger: destructive,
      },
    );
    if (!ok) return;

    setBulkBusy(true);
    setActionError(null);
    setMessage(null);
    try {
      setMessage(await composeProjectAction(group.key, action));
      await refreshAfterAction();
    } catch (e) {
      setActionError(e instanceof Error ? e.message : String(e));
    } finally {
      setBulkBusy(false);
    }
  }

  const running = rows.filter((c) => c.state === "running");

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(
        detection,
        dockerBlockedStatus(detection.data, refreshAll),
      )}
      meta={
        detection.data?.version && <Badge mono>{detection.data.version}</Badge>
      }
      actions={
        <>
          <Button
            variant="secondary"
            icon={FileUp}
            onClick={() => setShowLoadCompose(true)}
          >
            Load compose file
          </Button>
          <Button
            variant="secondary"
            icon={Square}
            disabled={running.length === 0 || bulkBusy}
            onClick={() =>
              runBulk("stop", running, `Stop all ${running.length} containers`)
            }
          >
            Stop all
          </Button>
          <Button
            variant="secondary"
            icon={RefreshCw}
            loading={containers.refreshing}
            onClick={refreshAll}
          >
            Refresh
          </Button>
        </>
      }
    >
      <Stack gap={5}>
        {message && (
          <Callout tone="success" onDismiss={() => setMessage(null)}>
            <Mono>{message}</Mono>
          </Callout>
        )}
        {actionError && (
          <Callout tone="danger" onDismiss={() => setActionError(null)}>
            {actionError}
          </Callout>
        )}
        {portWarning && (
          <Callout
            tone="warning"
            title="Port conflict"
            onDismiss={() => setPortWarning(null)}
          >
            {portWarning}
          </Callout>
        )}

        {/* The selection bar is the only place bulk start/stop/restart lives, and
            it only exists while something is selected — a permanently visible bar
            with everything disabled is just clutter. */}
        {selectedVisible.length > 0 && (
          <Section padding="sm">
            <Toolbar
              end={
                <ActionGroup>
                  <Button
                    size="sm"
                    variant="secondary"
                    icon={Play}
                    disabled={bulkBusy}
                    onClick={() =>
                      runBulk("start", selectedVisible, "Start selected")
                    }
                  >
                    Start
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    icon={Square}
                    disabled={bulkBusy}
                    onClick={() =>
                      runBulk("stop", selectedVisible, "Stop selected")
                    }
                  >
                    Stop
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    icon={RotateCw}
                    disabled={bulkBusy}
                    onClick={() =>
                      runBulk("restart", selectedVisible, "Restart selected")
                    }
                  >
                    Restart
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => setSelected(new Set())}
                  >
                    Clear
                  </Button>
                </ActionGroup>
              }
            >
              <strong>{selectedVisible.length} selected</strong>
            </Toolbar>
          </Section>
        )}

        <Section
          title="Containers"
          padding="none"
          toolbar={
            rows.length > 0 && (
              <Toolbar
                end={
                  <ActionGroup>
                    <Button
                      size="sm"
                      variant="ghost"
                      icon={anyExpanded ? ChevronsDownUp : ChevronsUpDown}
                      onClick={toggleAllGroups}
                      title={
                        anyExpanded
                          ? "Fold every stack away"
                          : "Expand every stack"
                      }
                    >
                      {anyExpanded ? "Fold all" : "Expand all"}
                    </Button>
                    <SegmentedControl
                      ariaLabel="Filter by state"
                      value={stateFilter}
                      onChange={setStateFilter}
                      options={[
                        { value: "all", label: "All", count: rows.length },
                        {
                          value: "running",
                          label: "Running",
                          count: runningCount,
                        },
                        {
                          value: "stopped",
                          label: "Stopped",
                          count: stoppedCount,
                        },
                      ]}
                    />
                  </ActionGroup>
                }
              >
                <SearchInput
                  value={search}
                  onChange={setSearch}
                  placeholder="Search name, image, status, or stack"
                />
              </Toolbar>
            )
          }
          isEmpty={!containers.loading && groups.length === 0}
          empty={
            rows.length === 0
              ? {
                  icon: ContainerIcon,
                  title: "No containers",
                  description:
                    "Nothing is running or stopped on this daemon yet.",
                  action: (
                    <Button
                      variant="primary"
                      icon={FileUp}
                      onClick={() => setShowLoadCompose(true)}
                    >
                      Load compose file
                    </Button>
                  ),
                }
              : {
                  icon: SearchX,
                  title: "No matches",
                  description:
                    "No containers match the current search and filter.",
                  action: (
                    <Button
                      variant="ghost"
                      onClick={() => {
                        setSearch("");
                        setStateFilter("all");
                      }}
                    >
                      Clear filters
                    </Button>
                  ),
                }
          }
        >
          <Stack gap={4}>
            {groups.map((group) => (
              <Section
                key={group.key}
                headingLevel={3}
                title={group.label}
                icon={group.isCompose ? Layers : ContainerIcon}
                padding="none"
                /* A machine with several stacks is mostly scrolling; folding a
                   group keeps its header and running count in view. */
                collapsible
                collapsed={!expandedGroups.has(group.key)}
                onCollapsedChange={(collapsed) =>
                  setGroupExpanded(group.key, !collapsed)
                }
                meta={
                  <StatusPill
                    variant={
                      group.runningCount === group.containers.length
                        ? "ok"
                        : "warn"
                    }
                    label={`${group.runningCount}/${group.containers.length} running`}
                  />
                }
                actions={
                  /* Stack-level verbs exist only for a real compose project.
                     Standalone containers have no compose file behind them, so
                     there is nothing for `up` or `pull` to act on. */
                  group.isCompose && (
                    <ActionGroup>
                      <Button
                        size="sm"
                        variant="secondary"
                        icon={ArrowUpCircle}
                        disabled={bulkBusy}
                        title="Re-read the compose file and recreate whatever changed"
                        onClick={() => runComposeAction(group, "up", "Update")}
                      >
                        Update
                      </Button>
                      <Button
                        size="sm"
                        variant="secondary"
                        icon={Square}
                        disabled={bulkBusy || group.runningCount === 0}
                        onClick={() => runComposeAction(group, "stop", "Stop")}
                      >
                        Stop
                      </Button>
                      <Menu
                        items={[
                          {
                            key: "start",
                            label: "Start stack",
                            icon: Play,
                            onClick: () =>
                              runComposeAction(group, "start", "Start"),
                          },
                          {
                            key: "restart",
                            label: "Restart stack",
                            icon: RotateCw,
                            onClick: () =>
                              runComposeAction(group, "restart", "Restart"),
                          },
                          {
                            key: "pull",
                            label: "Pull images",
                            icon: Download,
                            onClick: () =>
                              runComposeAction(
                                group,
                                "pull",
                                "Pull images for",
                              ),
                          },
                          {
                            key: "edit",
                            label: "Edit compose…",
                            icon: Pencil,
                            onClick: () =>
                              setEditCompose({
                                kind: "project",
                                project: group.key,
                              }),
                          },
                          {
                            key: "down",
                            label: "Remove stack",
                            icon: Trash2,
                            danger: true,
                            separatorBefore: true,
                            onClick: () =>
                              runComposeAction(group, "down", "Remove"),
                          },
                        ]}
                      />
                    </ActionGroup>
                  )
                }
              >
                <GroupTable
                  group={group}
                  selectedIds={selectedByGroup.get(group.key) ?? []}
                  busyId={busyId}
                  onToggleContainer={toggleSelected}
                  onToggleGroup={toggleGroup}
                  onLifecycle={handleLifecycle}
                  onViewLogs={setLogsFor}
                  onManagePorts={setPortsFor}
                  onEditCompose={(c) =>
                    setEditCompose({
                      kind: "container",
                      id: c.id,
                      name: c.name,
                    })
                  }
                />
              </Section>
            ))}
          </Stack>
        </Section>
      </Stack>

      {logsFor && (
        <LogsModal container={logsFor} onClose={() => setLogsFor(null)} />
      )}
      {portsFor && (
        <PortsModal
          container={portsFor}
          onClose={() => setPortsFor(null)}
          onChanged={refreshAfterAction}
        />
      )}
      {editCompose && (
        /* No fallback: the modal renders its own "reading the compose file"
           state, and a spinner that flashes for one frame of chunk-loading
           before being replaced by another spinner is just a flicker. */
        <Suspense fallback={null}>
          <EditComposeModal
            target={editCompose}
            onClose={() => setEditCompose(null)}
            onSaved={(msg) => {
              setEditCompose(null);
              setMessage(msg);
              void refreshAfterAction();
            }}
          />
        </Suspense>
      )}
      {showLoadCompose && (
        <LoadComposeModal
          onClose={() => setShowLoadCompose(false)}
          onLoaded={(msg) => {
            setShowLoadCompose(false);
            setMessage(msg);
            void refreshAll();
          }}
        />
      )}
    </Page>
  );
}
