import { useEffect, useMemo, useState } from "react";
import {
  Badge,
  Callout,
  CodeBlock,
  Column,
  CopyButton,
  DataTable,
  Icon,
  Modal,
  ModalFooter,
  Button,
  Mono,
  Spinner,
  Stack,
  StatusPill,
  StatusVariant,
  Tabs,
} from "../../ui";
import { ChevronDown, ChevronRight, ScrollText, SquareTerminal, Table2 } from "../../ui/icons";
import { LogRun, RunStatus, failureWindow, formatMs, parseLaravelLog, summarize } from "./laravelLog";
import { LaravelLogEntry, fetchLaravelLogErrors } from "./laravelLogErrors";

export interface LogViewerModalProps {
  title: string;
  /** Called on mount and then every `intervalMs`. */
  fetchLogs: (lines: number) => Promise<string>;
  onClose: () => void;
  lines?: number;
  intervalMs?: number;
  /**
   * Adds a Table tab that reads Laravel's `queue:work` / `schedule:run`
   * output into one row per job. The raw Console tab stays the default and
   * is always there — the table is a reading of it, not a replacement.
   */
  laravelTable?: boolean;
  /**
   * The project the command runs in. With it, a failed row's details also
   * show what Laravel logged for the failure — `queue:work` itself prints
   * only "FAIL", never the exception.
   */
  workingDir?: string;
}

type View = "console" | "table";

const STATUS: Record<RunStatus, { variant: StatusVariant; label: string }> = {
  running: { variant: "pending", label: "running" },
  done: { variant: "ok", label: "done" },
  failed: { variant: "bad", label: "failed" },
  // Started, then the log moved on without an end line: worker killed, or the
  // start scrolled out of the tail.
  unknown: { variant: "neutral", label: "no result" },
};

/** Rows worth opening: a failure to explain, or something the run printed. */
const hasDetails = (r: LogRun) => r.status === "failed" || r.status === "unknown" || r.output.length > 0;

function columns(expanded: ReadonlySet<string>): Column<LogRun>[] {
  return [
    {
      key: "toggle",
      header: "",
      headerLabel: "Details",
      width: "2.25rem",
      render: (r) =>
        hasDetails(r) ? <Icon icon={expanded.has(r.key) ? ChevronDown : ChevronRight} size="sm" /> : null,
    },
    {
      key: "started",
      header: "Started",
      mono: true,
      width: "11rem",
      render: (r) => r.started ?? "—",
      sortValue: (r) => r.started ?? r.finished ?? "",
    },
    {
      key: "name",
      header: "Job / command",
      wrap: true,
      render: (r) => (
        <Stack gap={1}>
          <Mono title={r.name}>{r.name}</Mono>
          {r.description && <span className="u-muted">{r.description}</span>}
          {r.queue && (
            <span className="u-muted">
              #{r.jobId} on {r.connection}/{r.queue}
            </span>
          )}
        </Stack>
      ),
      sortValue: (r) => r.name,
    },
    {
      key: "status",
      header: "Status",
      width: "8rem",
      render: (r) => <StatusPill variant={STATUS[r.status].variant} label={STATUS[r.status].label} />,
      sortValue: (r) => r.status,
    },
    {
      key: "finished",
      header: "Finished",
      mono: true,
      width: "11rem",
      priority: 3,
      render: (r) => r.finished ?? "—",
      sortValue: (r) => r.finished ?? "",
    },
    {
      key: "duration",
      header: "Duration",
      mono: true,
      align: "end",
      width: "7rem",
      priority: 2,
      render: (r) => r.duration ?? "—",
      sortValue: (r) => r.durationMs ?? -1,
    },
  ];
}

/**
 * Shared by the Docker, Workers and Scheduled log viewers, which were
 * byte-for-byte identical apart from their fetcher and poll interval.
 */
export function LogViewerModal({
  title,
  fetchLogs,
  onClose,
  lines = 300,
  intervalMs = 3000,
  laravelTable = false,
  workingDir,
}: LogViewerModalProps) {
  const [logs, setLogs] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>("console");

  useEffect(() => {
    let active = true;

    async function load() {
      try {
        const text = await fetchLogs(lines);
        if (!active) return;
        setLogs(text);
        setError(null);
      } catch (e) {
        if (active) setError(e instanceof Error ? e.message : String(e));
      }
    }

    void load();
    const interval = setInterval(load, intervalMs);
    return () => {
      active = false;
      clearInterval(interval);
    };
  }, [fetchLogs, lines, intervalMs]);

  const parsed = useMemo(() => (laravelTable ? parseLaravelLog(logs) : null), [laravelTable, logs]);
  // Newest first: the table is for "what just happened", the console for the story.
  const rows = useMemo(() => (parsed ? [...parsed.runs].reverse() : []), [parsed]);

  return (
    <Modal
      size="full"
      fill
      title={title}
      icon={ScrollText}
      description={`Last ${lines} lines, refreshing every ${Math.round(intervalMs / 1000)}s.`}
      onClose={onClose}
      footer={
        <ModalFooter start={<CopyButton value={logs} label="Copy logs" size="md" />}>
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
        </ModalFooter>
      }
    >
      {/* Only the log or the table scrolls; the tabs stay put above it. */}
      <Stack gap={4} fill>
        {laravelTable && (
          <Tabs
            tabs={[
              { id: "console", label: "Console", icon: SquareTerminal },
              { id: "table", label: `Table (${rows.length})`, icon: Table2 },
            ]}
            active={view}
            onChange={(id) => setView(id as View)}
            ariaLabel="Log view"
          />
        )}
        {/* A failed poll keeps the last good output on screen rather than
            replacing it — stale logs still beat an empty panel. */}
        {error && <Callout tone="danger">{error}</Callout>}
        {view === "table" && parsed ? (
          <RunTable runs={rows} otherLines={parsed.otherLines} workingDir={workingDir} />
        ) : (
          <CodeBlock fill autoScrollBottom>
            {logs || "(no output yet)"}
          </CodeBlock>
        )}
      </Stack>
    </Modal>
  );
}

function RunTable({ runs, otherLines, workingDir }: { runs: LogRun[]; otherLines: number; workingDir?: string }) {
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const cols = useMemo(() => columns(expanded), [expanded]);
  const s = summarize(runs);

  const toggle = (r: LogRun) => {
    if (!hasDetails(r)) return;
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(r.key)) next.delete(r.key);
      else next.add(r.key);
      return next;
    });
  };

  return (
    <Stack gap={3} fill>
      {runs.length > 0 && (
        <Stack direction="row" gap={2} wrap align="center">
          <Badge>{s.total} runs</Badge>
          <Badge tone="success">{s.done} done</Badge>
          <Badge tone={s.failed ? "danger" : "neutral"}>{s.failed} failed</Badge>
          {s.running > 0 && <Badge tone="info">{s.running} running</Badge>}
          {s.averageMs !== undefined && <Badge mono>avg {formatMs(s.averageMs)}</Badge>}
          {s.failed > 0 && <span className="u-muted">Click a failed run to see why it failed.</span>}
        </Stack>
      )}
      <DataTable
        columns={cols}
        rows={runs}
        rowKey={(r) => r.key}
        density="compact"
        fillHeight
        onRowClick={toggle}
        expandedKeys={expanded}
        renderExpanded={(r) => <RunDetails run={r} workingDir={workingDir} />}
        caption="Runs read from the log"
        empty={{
          size: "sm",
          title: "No Laravel job lines in this output",
          description:
            "The table reads php artisan queue:work and schedule:run output. Everything the command printed is in Console.",
        }}
      />
      {otherLines > 0 && runs.length > 0 && (
        <span className="u-muted">
          {otherLines} {otherLines === 1 ? "line is" : "lines are"} not a run (errors, notices) — shown under the run
          they followed, and in Console.
        </span>
      )}
    </Stack>
  );
}

function RunDetails({ run, workingDir }: { run: LogRun; workingDir?: string }) {
  const explain = run.status === "failed" || run.status === "unknown";
  return (
    <Stack gap={3}>
      {run.output.length > 0 && (
        <Stack gap={2}>
          <span className="u-muted">Printed by the command after this run started</span>
          <CodeBlock maxHeight="16rem">{run.output.join("\n")}</CodeBlock>
        </Stack>
      )}
      {explain && <FailureReason run={run} workingDir={workingDir} />}
    </Stack>
  );
}

type Lookup =
  | { state: "loading" }
  | { state: "done"; entries: LaravelLogEntry[] }
  | { state: "error"; message: string };

function FailureReason({ run, workingDir }: { run: LogRun; workingDir?: string }) {
  const span = failureWindow(run);
  const from = span?.from;
  const to = span?.to;
  const [lookup, setLookup] = useState<Lookup>({ state: "loading" });

  useEffect(() => {
    if (!workingDir || !from || !to) return;
    let active = true;
    setLookup({ state: "loading" });
    fetchLaravelLogErrors(workingDir, from, to)
      .then((entries) => active && setLookup({ state: "done", entries }))
      .catch((e) => active && setLookup({ state: "error", message: e instanceof Error ? e.message : String(e) }));
    return () => {
      active = false;
    };
    // `run.finished` arriving moves `to`, which re-runs the lookup.
  }, [workingDir, from, to]);

  if (!workingDir || !from || !to) {
    return (
      <Callout tone="info">
        Laravel's queue worker prints only "FAIL", not the reason. The exception is in the project's
        storage/logs/laravel.log, and the job in php artisan queue:failed.
      </Callout>
    );
  }
  if (lookup.state === "loading") return <Spinner />;
  if (lookup.state === "error") return <Callout tone="warning">Could not read the Laravel log: {lookup.message}</Callout>;
  if (lookup.entries.length === 0) {
    return (
      <Callout tone="info" title="Nothing in the Laravel log for this run">
        No error was logged between <Mono>{from}</Mono> and <Mono>{to}</Mono>. If the app reports elsewhere (a
        custom log channel, Sentry), look there; php artisan queue:failed lists the failed job.
      </Callout>
    );
  }
  // Stamps compare as strings. Nothing near the end (a run cut short has no
  // end) leaves every entry in the window as a candidate.
  const likelyFrom = span?.likelyFrom ?? from;
  const nearEnd = lookup.entries.filter((e) => e.at >= likelyFrom);
  const likely = nearEnd.length ? nearEnd : lookup.entries;
  const earlier = nearEnd.length ? lookup.entries.filter((e) => e.at < likelyFrom) : [];

  return (
    <Stack gap={3}>
      <span className="u-muted">
        {likely.length === 1 ? "Most likely cause" : "Most likely causes"}, from the project's Laravel log — matched by
        time, as the worker itself prints only "FAIL".
      </span>
      {likely.map((entry, i) => (
        <LogEntry key={`${entry.at}-${i}`} entry={entry} />
      ))}
      {earlier.length > 0 && (
        <Stack gap={2}>
          <span className="u-muted">
            Also logged while this run was going — with several jobs in the same second these may be a neighbour's:
          </span>
          {earlier.map((entry, i) => (
            <LogEntry key={`${entry.at}-${i}`} entry={entry} quiet />
          ))}
        </Stack>
      )}
    </Stack>
  );
}

function LogEntry({ entry, quiet = false }: { entry: LaravelLogEntry; quiet?: boolean }) {
  return (
    <Stack gap={2}>
      <Callout tone={quiet ? "warning" : "danger"} title={entry.message}>
        <Mono>{entry.at}</Mono> · {entry.level} · <Mono>{entry.file}</Mono>
      </Callout>
      <CodeBlock maxHeight={quiet ? "10rem" : "22rem"} wrap={false}>
        {entry.text}
      </CodeBlock>
    </Stack>
  );
}
