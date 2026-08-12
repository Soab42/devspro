import { useCallback, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import LogsModal from "../components/LogsModal";
import PortsModal from "../components/PortsModal";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Column,
  DataTable,
  IconButton,
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
  Container as ContainerIcon,
  ExternalLink,
  Network,
  Play,
  RefreshCw,
  RotateCw,
  ScrollText,
  SearchX,
  Square,
} from "../../../ui/icons";
import {
  Container,
  detectDocker,
  listContainers,
  restartContainer,
  startContainer,
  stopContainer,
} from "../api";
import { dockerBlockedStatus } from "../blocked";
import { parsePortMappings } from "../utils";

type StateFilter = "all" | "running" | "stopped";

export default function DockerPage() {
  const confirmAction = useConfirm();
  const module = useModule("docker");

  const detection = useResource(detectDocker);
  const dockerUsable = Boolean(detection.data?.found && detection.data.daemon_reachable);
  const containers = useResource(listContainers, { enabled: dockerUsable });

  const [busyId, setBusyId] = useState<string | null>(null);
  const [logsFor, setLogsFor] = useState<Container | null>(null);
  const [portsFor, setPortsFor] = useState<Container | null>(null);
  /* Action failures are shown inline, NOT through Page's status — routing them
     there would blank the whole table because one Stop button failed. */
  const [actionError, setActionError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [stateFilter, setStateFilter] = useState<StateFilter>("all");

  const refreshAll = useCallback(async () => {
    await detection.refresh();
    await containers.refresh();
  }, [detection.refresh, containers.refresh]);

  const rows = useMemo(() => containers.data ?? [], [containers.data]);

  async function runAction(
    action: (id: string) => Promise<string>,
    container: Container,
    verb: string,
  ) {
    const ok = await confirmAction(
      `This will ${verb.toLowerCase()} the container "${container.name}".`,
      { title: `${verb} container?`, confirmLabel: verb },
    );
    if (!ok) return;

    setBusyId(container.id);
    setActionError(null);
    try {
      await action(container.id);
      await refreshAll();
    } catch (e) {
      setActionError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyId(null);
    }
  }

  const runningCount = rows.filter((c) => c.state === "running").length;
  const stoppedCount = rows.length - runningCount;
  const stopped = rows.filter((c) => c.state !== "running");

  const query = search.trim().toLowerCase();
  const visible = rows.filter((c) => {
    if (stateFilter === "running" && c.state !== "running") return false;
    if (stateFilter === "stopped" && c.state === "running") return false;
    if (!query) return true;
    return (
      c.name.toLowerCase().includes(query) ||
      c.image.toLowerCase().includes(query) ||
      c.status.toLowerCase().includes(query)
    );
  });

  const columns: Column<Container>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      sortValue: (c) => c.name,
      render: (c) => <span className="dt-strong">{c.name}</span>,
    },
    {
      key: "state",
      header: "State",
      priority: 1,
      sortValue: (c) => c.state,
      render: (c) => (
        <StatusPill variant={c.state === "running" ? "ok" : "bad"} label={c.state} />
      ),
    },
    {
      key: "status",
      header: "Status",
      priority: 2,
      sortValue: (c) => c.status,
      render: (c) => <span className="u-muted">{c.status}</span>,
    },
    {
      key: "ports",
      header: "Ports",
      priority: 2,
      mono: true,
      render: (c) => (c.ports ? <Mono truncate>{c.ports}</Mono> : <span className="u-muted">—</span>),
    },
    {
      key: "image",
      header: "Image",
      priority: 3,
      mono: true,
      sortValue: (c) => c.image,
      render: (c) => <Mono truncate>{c.image}</Mono>,
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (c) => {
        const running = c.state === "running";
        const busy = busyId === c.id;
        const mappings = running ? parsePortMappings(c.ports) : [];

        return (
          <ActionGroup>
            {running ? (
              <IconButton
                icon={Square}
                label="Stop"
                loading={busy}
                onClick={() => runAction(stopContainer, c, "Stop")}
              />
            ) : (
              <IconButton
                icon={Play}
                label="Start"
                loading={busy}
                onClick={() => runAction(startContainer, c, "Start")}
              />
            )}
            <IconButton
              icon={RotateCw}
              label="Restart"
              disabled={busy}
              onClick={() => runAction(restartContainer, c, "Restart")}
            />
            {/* Six inline buttons per row made the action column wider than the
                data; only the lifecycle verbs stay inline now. */}
            <Menu
              items={[
                {
                  key: "logs",
                  label: "View logs",
                  icon: ScrollText,
                  onClick: () => setLogsFor(c),
                },
                {
                  key: "ports",
                  label: "Manage ports",
                  icon: Network,
                  onClick: () => setPortsFor(c),
                },
                ...mappings.map((m) => ({
                  key: `open-${m.hostPort}`,
                  label: `Open :${m.hostPort}`,
                  icon: ExternalLink,
                  onClick: () => void openUrl(`http://localhost:${m.hostPort}`),
                  separatorBefore: m === mappings[0],
                })),
              ]}
            />
          </ActionGroup>
        );
      },
    },
  ];

  const attentionColumns: Column<Container>[] = [
    { key: "name", header: "Name", priority: 1, render: (c) => c.name },
    {
      key: "state",
      header: "State",
      priority: 1,
      render: (c) => <StatusPill variant="bad" label={c.state} />,
    },
    {
      key: "status",
      header: "Status",
      priority: 2,
      render: (c) => <span className="u-muted">{c.status}</span>,
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (c) => (
        <ActionGroup>
          <Button
            size="sm"
            variant="primary"
            icon={Play}
            loading={busyId === c.id}
            onClick={() => runAction(startContainer, c, "Start")}
          >
            Start
          </Button>
        </ActionGroup>
      ),
    },
  ];

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(detection, dockerBlockedStatus(detection.data, refreshAll))}
      meta={detection.data?.version && <Badge mono>{detection.data.version}</Badge>}
      actions={
        <Button
          variant="secondary"
          icon={RefreshCw}
          loading={containers.refreshing}
          onClick={refreshAll}
        >
          Refresh
        </Button>
      }
    >
      <Stack gap={5}>
        {actionError && (
          <Callout tone="danger" onDismiss={() => setActionError(null)}>
            {actionError}
          </Callout>
        )}

        {stopped.length > 0 && (
          <Section
            title="Needs attention"
            meta={<StatusPill variant="warn" label={`${stopped.length} not running`} />}
            padding="none"
          >
            <DataTable
              columns={attentionColumns}
              rows={stopped}
              rowKey={(c) => c.id}
              caption="Containers that are not running"
            />
          </Section>
        )}

        <Section
          title="Containers"
          padding="none"
          toolbar={
            rows.length > 0 && (
              <Toolbar
                end={
                  <SegmentedControl
                    ariaLabel="Filter by state"
                    value={stateFilter}
                    onChange={setStateFilter}
                    options={[
                      { value: "all", label: "All", count: rows.length },
                      { value: "running", label: "Running", count: runningCount },
                      { value: "stopped", label: "Stopped", count: stoppedCount },
                    ]}
                  />
                }
              >
                <SearchInput
                  value={search}
                  onChange={setSearch}
                  placeholder="Search name, image, or status"
                />
              </Toolbar>
            )
          }
        >
          <DataTable
            columns={columns}
            rows={visible}
            rowKey={(c) => c.id}
            loading={containers.loading}
            defaultSort={{ key: "name", dir: "asc" }}
            caption="Docker containers"
            empty={
              rows.length === 0
                ? {
                    icon: ContainerIcon,
                    title: "No containers",
                    description: "Nothing is running or stopped on this daemon yet.",
                  }
                : {
                    icon: SearchX,
                    title: "No matches",
                    description: "No containers match the current search and filter.",
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
          />
        </Section>
      </Stack>

      {logsFor && <LogsModal container={logsFor} onClose={() => setLogsFor(null)} />}
      {portsFor && (
        <PortsModal
          container={portsFor}
          onClose={() => setPortsFor(null)}
          onChanged={refreshAll}
        />
      )}
    </Page>
  );
}
