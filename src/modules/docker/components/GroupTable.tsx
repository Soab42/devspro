import { memo, useMemo } from "react";
import {
  ActionGroup,
  Checkbox,
  Column,
  DataTable,
  IconButton,
  Menu,
  Mono,
  StatusPill,
} from "../../../ui";
import {
  ExternalLink,
  Network,
  Pencil,
  Play,
  RotateCw,
  ScrollText,
  Square,
} from "../../../ui/icons";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Container } from "../api";
import { ContainerGroup, parsePortMappings } from "../utils";

export interface GroupTableProps {
  group: ContainerGroup;
  /** Ids selected *within this group* — see the note on memoisation below. */
  selectedIds: string[];
  busyId: string | null;
  onToggleContainer: (id: string, checked: boolean) => void;
  onToggleGroup: (group: ContainerGroup, checked: boolean) => void;
  onLifecycle: (action: "start" | "stop" | "restart", container: Container) => void;
  onViewLogs: (container: Container) => void;
  onManagePorts: (container: Container) => void;
  onEditCompose: (container: Container) => void;
}

/**
 * One compose stack's (or Standalone's) container table.
 *
 * Extracted from `DockerPage` and memoised for one reason: the columns array was
 * being rebuilt inline in the page's JSX, so every page render handed `DataTable`
 * a new `columns` identity. That invalidates its `sortRows` memo *and* every
 * cell's render closure — so a single keystroke in the filter box re-sorted and
 * re-rendered every row of every group. On a machine with a few stacks that is
 * hundreds of cells, each with a StatusPill, a Mono and a Menu, per character
 * typed.
 *
 * The memo only holds if its props are stable, which is why the page passes
 * `selectedIds` (this group's ids, memoised) rather than the whole selection
 * `Set` — the Set gets a new identity on every toggle, which would re-render
 * every group's table when one checkbox in one of them changed.
 */
export const GroupTable = memo(function GroupTable({
  group,
  selectedIds,
  busyId,
  onToggleContainer,
  onToggleGroup,
  onLifecycle,
  onViewLogs,
  onManagePorts,
  onEditCompose,
}: GroupTableProps) {
  const selected = useMemo(() => new Set(selectedIds), [selectedIds]);

  /* Parsed once per container, not once per render of the actions cell. The
     regex work is small, but it was running on every cell render for every row,
     which is exactly the kind of per-frame cost that adds up on a page holding
     several tables. */
  const mappings = useMemo(
    () => new Map(group.containers.map((c) => [c.id, parsePortMappings(c.ports)])),
    [group.containers],
  );

  const columns = useMemo<Column<Container>[]>(() => {
    const allSelected = group.containers.length > 0 && group.containers.every((c) => selected.has(c.id));
    const someSelected = group.containers.some((c) => selected.has(c.id));

    return [
      {
        key: "select",
        width: "1px",
        header: (
          <Checkbox
            checked={allSelected}
            indeterminate={someSelected && !allSelected}
            onChange={(checked) => onToggleGroup(group, checked)}
            label={`Select all in ${group.label}`}
            hideLabel
          />
        ),
        headerLabel: `Select all in ${group.label}`,
        render: (c) => (
          <Checkbox
            checked={selected.has(c.id)}
            onChange={(checked) => onToggleContainer(c.id, checked)}
            label={`Select ${c.name}`}
            hideLabel
          />
        ),
      },
      {
        key: "name",
        header: "Name",
        priority: 1,
        wrap: true,
        sortValue: (c) => c.name,
        render: (c) => (
          <div className="dt-cell-stack">
            <span className="dt-strong">{c.name}</span>
            {c.compose_service && <Mono className="dt-cell-sub">{c.compose_service}</Mono>}
          </div>
        ),
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
        render: (c) =>
          c.ports ? <Mono truncate>{c.ports}</Mono> : <span className="u-muted">—</span>,
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
          const published = running ? (mappings.get(c.id) ?? []) : [];

          return (
            <ActionGroup>
              {running ? (
                <IconButton
                  icon={Square}
                  label="Stop"
                  loading={busy}
                  onClick={() => onLifecycle("stop", c)}
                />
              ) : (
                <IconButton
                  icon={Play}
                  label="Start"
                  loading={busy}
                  onClick={() => onLifecycle("start", c)}
                />
              )}
              <IconButton
                icon={RotateCw}
                label="Restart"
                disabled={busy}
                onClick={() => onLifecycle("restart", c)}
              />
              {/* Six inline buttons per row made the action column wider than the
                  data; only the lifecycle verbs stay inline now. */}
              <Menu
                items={[
                  {
                    key: "logs",
                    label: "View logs",
                    icon: ScrollText,
                    onClick: () => onViewLogs(c),
                  },
                  {
                    key: "ports",
                    label: "Manage ports",
                    icon: Network,
                    onClick: () => onManagePorts(c),
                  },
                  /* Only for a compose-managed container: a plain `docker run`
                     container has no file behind it to edit. */
                  ...(c.compose_project
                    ? [
                        {
                          key: "edit",
                          label: "Edit compose…",
                          icon: Pencil,
                          onClick: () => onEditCompose(c),
                        },
                      ]
                    : []),
                  ...published.map((m) => ({
                    key: `open-${m.hostPort}`,
                    label: `Open :${m.hostPort}`,
                    icon: ExternalLink,
                    onClick: () => void openUrl(`http://localhost:${m.hostPort}`),
                    separatorBefore: m === published[0],
                  })),
                ]}
              />
            </ActionGroup>
          );
        },
      },
    ];
  }, [
    group,
    selected,
    busyId,
    mappings,
    onToggleContainer,
    onToggleGroup,
    onLifecycle,
    onViewLogs,
    onManagePorts,
    onEditCompose,
  ]);

  return (
    <DataTable
      columns={columns}
      rows={group.containers}
      rowKey={(c) => c.id}
      defaultSort={{ key: "name", dir: "asc" }}
      caption={`Containers in ${group.label}`}
    />
  );
});
