import { useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import CommandModal from "../components/CommandModal";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Column,
  DataTable,
  IconButton,
  Mono,
  Page,
  SearchInput,
  Section,
  SegmentedControl,
  Stack,
  Switch,
  Toolbar,
  statusOf,
  useResource,
} from "../../../ui";
import { Maximize2, Network, OctagonX, RefreshCw, Square } from "../../../ui/icons";
import { killProcess, listPorts, PortEntry } from "../api";

type Category = "next" | "node" | "php" | "python" | "other" | "unowned";

const CATEGORY_LABELS: Record<Category, string> = {
  next: "Next.js",
  node: "Node.js",
  php: "PHP",
  python: "Python",
  other: "Other (mine)",
  unowned: "No owner shown",
};

// Checked in this order — a Next.js dev server IS a node process (its command
// line contains "next" as well as being launched by "node"), so the more
// specific match has to win before the generic "node" one does.
function categorize(p: PortEntry): Category {
  const haystack = `${p.process_name ?? ""} ${p.command ?? ""}`.toLowerCase();
  if (!p.pid) return "unowned";
  if (haystack.includes("next")) return "next";
  if (haystack.includes("node")) return "node";
  if (haystack.includes("php")) return "php";
  if (
    haystack.includes("python") ||
    haystack.includes("gunicorn") ||
    haystack.includes("uvicorn")
  )
    return "python";
  return "other";
}

export default function ProcessesPage() {
  const confirmAction = useConfirm();
  const module = useModule("processes");
  const ports = useResource(listPorts);

  const [filter, setFilter] = useState("");
  const [category, setCategory] = useState<Category | "all">("all");
  const [ownerOnly, setOwnerOnly] = useState(true);
  const [commandFor, setCommandFor] = useState<string | null>(null);
  const [busyPid, setBusyPid] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  async function handleKill(entry: PortEntry, force: boolean) {
    if (!entry.pid) return;
    const label = entry.process_name ?? `pid ${entry.pid}`;

    const ok = await confirmAction(
      <>
        <p>
          {label} on port <Mono variant="chip">{entry.port}</Mono> (pid{" "}
          <Mono variant="chip">{entry.pid}</Mono>).
        </p>
        {force && <p>This cannot be undone and may lose unsaved work in that process.</p>}
      </>,
      {
        title: force ? "Force kill this process?" : "Stop this process?",
        confirmLabel: force ? "Force kill" : "Stop",
        danger: true,
      },
    );
    if (!ok) return;

    setBusyPid(entry.pid);
    setMessage(null);
    setError(null);
    try {
      setMessage(await killProcess(entry.pid, force));
      await ports.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyPid(null);
    }
  }

  const rows = ports.data ?? [];
  const query = filter.trim().toLowerCase();
  const ownerFiltered = ownerOnly ? rows.filter((p) => p.pid) : rows;

  const counts = ownerFiltered.reduce<Record<string, number>>((acc, p) => {
    const c = categorize(p);
    acc[c] = (acc[c] ?? 0) + 1;
    return acc;
  }, {});

  const visible = ownerFiltered.filter((p) => {
    if (category !== "all" && categorize(p) !== category) return false;
    if (!query) return true;
    return (
      String(p.port).includes(query) ||
      (p.process_name ?? "").toLowerCase().includes(query) ||
      (p.command ?? "").toLowerCase().includes(query)
    );
  });

  const categoryOptions = [
    { value: "all" as const, label: "All", count: ownerFiltered.length },
    ...(Object.keys(CATEGORY_LABELS) as Category[])
      .filter((c) => counts[c])
      .map((c) => ({ value: c, label: CATEGORY_LABELS[c], count: counts[c] })),
  ];

  const columns: Column<PortEntry>[] = [
    {
      key: "port",
      header: "Port",
      priority: 1,
      mono: true,
      align: "end",
      width: "1px",
      sortValue: (p) => p.port,
      render: (p) => p.port,
    },
    {
      key: "process",
      header: "Process",
      priority: 1,
      sortValue: (p) => p.process_name ?? "",
      render: (p) =>
        p.process_name ? (
          /* Was a green "ok" StatusPill, which claimed the process was healthy
             when it only meant it had a name. A neutral badge is the honest
             equivalent. */
          <Badge tone="neutral">{p.process_name}</Badge>
        ) : (
          <span className="u-muted">unknown</span>
        ),
    },
    {
      key: "pid",
      header: "PID",
      priority: 2,
      mono: true,
      align: "end",
      sortValue: (p) => p.pid ?? 0,
      render: (p) => p.pid ?? "—",
    },
    {
      key: "address",
      header: "Address",
      priority: 3,
      mono: true,
      sortValue: (p) => p.address,
      render: (p) => <Mono truncate>{p.address}</Mono>,
    },
    {
      key: "command",
      header: "Command",
      priority: 3,
      render: (p) =>
        p.command ? (
          <span className="dt-cell-inline">
            {/* Explicit width: the cell holds a button, so it opts out of the
                table's own truncation and can't bound this itself. */}
            <Mono truncate={360}>{p.command}</Mono>
            {/* CSS truncation plus an always-available expand, replacing the
                old character-count heuristic that decided per row whether a
                "See more" link was warranted. */}
            <IconButton
              icon={Maximize2}
              label="Show full command"
              size="sm"
              onClick={() => setCommandFor(p.command!)}
            />
          </span>
        ) : (
          "—"
        ),
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (p) =>
        p.pid ? (
          <ActionGroup>
            <IconButton
              icon={Square}
              label="Stop"
              loading={busyPid === p.pid}
              onClick={() => handleKill(p, false)}
            />
            <IconButton
              icon={OctagonX}
              label="Force kill"
              variant="danger"
              disabled={busyPid === p.pid}
              onClick={() => handleKill(p, true)}
            />
          </ActionGroup>
        ) : null,
    },
  ];

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(ports)}
      /* Moved up from a trailing paragraph — explaining what the default
         filter hides only made sense before you'd scrolled past the table. */
      description={
        <>
          Only processes owned by your own user can be stopped from here — no elevation
          involved, on purpose. <strong>Owner only</strong> (on by default) hides
          everything else, like Docker's proxy for published container ports, which is
          managed from the Docker page instead.
        </>
      }
      actions={
        <Button
          variant="secondary"
          icon={RefreshCw}
          loading={ports.refreshing}
          onClick={ports.refresh}
        >
          Refresh
        </Button>
      }
    >
      <Stack gap={5}>
        {message && (
          <Callout tone="success" onDismiss={() => setMessage(null)}>
            {message}
          </Callout>
        )}
        {error && (
          <Callout tone="danger" onDismiss={() => setError(null)}>
            {error}
          </Callout>
        )}

        <Section
          title="Listening ports"
          padding="none"
          toolbar={
            <Toolbar
              end={
                <Switch
                  checked={ownerOnly}
                  onChange={setOwnerOnly}
                  label="Owner only"
                />
              }
            >
              <SearchInput
                value={filter}
                onChange={setFilter}
                placeholder="Filter by port, name, or command"
              />
              <SegmentedControl
                ariaLabel="Filter by process type"
                value={category}
                onChange={setCategory}
                options={categoryOptions}
              />
            </Toolbar>
          }
        >
          <DataTable
            columns={columns}
            rows={visible}
            rowKey={(p) => `${p.address}:${p.port}`}
            loading={ports.loading}
            defaultSort={{ key: "port", dir: "asc" }}
            caption="Processes listening on local ports"
            empty={{
              icon: Network,
              title: "No listening ports",
              description: "Nothing matches the current search and filter.",
            }}
          />
        </Section>
      </Stack>

      {commandFor && (
        <CommandModal command={commandFor} onClose={() => setCommandFor(null)} />
      )}
    </Page>
  );
}
