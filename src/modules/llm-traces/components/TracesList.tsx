import { useCallback, useEffect, useRef, useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Checkbox,
  type Column,
  DataTable,
  IconButton,
  SearchInput,
  Section,
  Select,
  Stack,
  Toolbar,
} from "../../../ui";
import { ChevronDown, Inbox, Play, Plug, SearchX, Trash2 } from "../../../ui/icons";
import { clearTraces, deleteTraces, listTraces, onIngested, type TraceList, type TraceSummary } from "../api";
import { SOURCE_LABEL, formatClock, formatCost, formatDuration, formatTokens, spanUs } from "../utils";
import "./LlmTraces.css";

const PAGE = 100;
/** The collector returns at most this many in one list; search reaches the rest. */
const MAX_ROWS = 500;
const DEBOUNCE_MS = 250;

interface Props {
  /** Changes when the page's Refresh is pressed. */
  reload: number;
  running: boolean;
  starting: boolean;
  onStart: () => void;
  onConnect: () => void;
  onOpen: (id: string) => void;
  /** After a delete or clear, so the header's counts follow. */
  onChanged: () => void;
}

export function TracesList({ reload, running, starting, onStart, onConnect, onOpen, onChanged }: Props) {
  const confirmAction = useConfirm();
  const [search, setSearch] = useState("");
  const [source, setSource] = useState("");
  const [project, setProject] = useState("");
  const [errorsOnly, setErrorsOnly] = useState(false);
  const [limit, setLimit] = useState(PAGE);
  const [data, setData] = useState<TraceList | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const query = useRef({ search, source, project, errorsOnly, limit });
  query.current = { search, source, project, errorsOnly, limit };

  const load = useCallback(async () => {
    const q = query.current;
    setLoading(true);
    try {
      setData(
        await listTraces({
          search: q.search || undefined,
          source: q.source || undefined,
          project: q.project || undefined,
          errors_only: q.errorsOnly,
          limit: q.limit,
        }),
      );
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    const timer = setTimeout(() => void load(), DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [search, source, project, errorsOnly, limit, reload, load]);

  // New traces appear as they arrive; the event is already throttled.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    void onIngested(() => void load()).then((u) => {
      if (gone) u();
      else unlisten = u;
    });
    return () => {
      gone = true;
      unlisten?.();
    };
  }, [load]);

  async function handleDelete(row: TraceSummary) {
    setBusy(row.id);
    try {
      await deleteTraces([row.id]);
      await load();
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function handleClear() {
    const ok = await confirmAction("Every caught trace, observation and score is deleted. The collector keeps running.", {
      title: "Delete all traces?",
      confirmLabel: "Delete all",
      danger: true,
    });
    if (!ok) return;
    setBusy("*");
    try {
      await clearTraces();
      await load();
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  const columns: Column<TraceSummary>[] = [
    {
      key: "time",
      header: "Time",
      priority: 1,
      width: "1px",
      minWidth: "7rem",
      sortValue: (r) => r.start_us,
      render: (r) => <span className="u-tnum u-nowrap">{formatClock(r.start_us)}</span>,
    },
    {
      key: "name",
      header: "Trace",
      priority: 1,
      sortValue: (r) => r.name ?? "",
      render: (r) => (
        <div className="llmt-cell-name">
          <span className="llmt-cell-title">
            {r.error_count > 0 && <Badge tone="danger">error</Badge>}
            <span className="u-truncate">{r.name ?? "Unnamed trace"}</span>
          </span>
          {(r.input_preview || r.output_preview) && (
            <span className="llmt-cell-preview u-muted">
              {r.input_preview ?? ""}
              {r.output_preview ? ` → ${r.output_preview}` : ""}
            </span>
          )}
        </div>
      ),
    },
    {
      key: "source",
      header: "Source",
      priority: 2,
      width: "1px",
      minWidth: "9rem",
      sortValue: (r) => r.source,
      render: (r) => (
        <Stack gap={1}>
          <Badge tone="info">{SOURCE_LABEL[r.source]}</Badge>
          {r.project && (
            <span className="u-muted u-truncate llmt-cell-project" title={r.project}>
              {r.project}
            </span>
          )}
        </Stack>
      ),
    },
    {
      key: "latency",
      header: "Latency",
      priority: 1,
      width: "1px",
      minWidth: "5rem",
      align: "end",
      sortValue: (r) => spanUs(r.start_us, r.end_us) ?? 0,
      render: (r) => <span className="u-tnum u-nowrap">{formatDuration(spanUs(r.start_us, r.end_us))}</span>,
    },
    {
      key: "tokens",
      header: "Tokens",
      priority: 2,
      width: "1px",
      align: "end",
      sortValue: (r) => r.total_tokens,
      render: (r) =>
        r.total_tokens > 0 ? (
          <span className="u-tnum u-nowrap" title={`${r.input_tokens} in, ${r.output_tokens} out`}>
            {formatTokens(r.total_tokens)}
          </span>
        ) : (
          <span className="u-muted">-</span>
        ),
    },
    {
      key: "cost",
      header: "Cost",
      priority: 3,
      width: "1px",
      align: "end",
      sortValue: (r) => r.cost ?? -1,
      render: (r) => <span className="u-tnum u-nowrap">{formatCost(r.cost)}</span>,
    },
    {
      key: "models",
      header: "Model",
      priority: 3,
      sortValue: (r) => r.models.join(","),
      render: (r) =>
        r.models.length ? (
          <span className="u-mono u-truncate" title={r.models.join(", ")}>
            {r.models.join(", ")}
          </span>
        ) : (
          <span className="u-muted">-</span>
        ),
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (r) => (
        <ActionGroup>
          <IconButton
            icon={Trash2}
            label="Delete trace"
            variant="danger"
            loading={busy === r.id}
            onClick={(e) => {
              e.stopPropagation();
              void handleDelete(r);
            }}
          />
        </ActionGroup>
      ),
    },
  ];

  const filtered = Boolean(search || source || project || errorsOnly);
  const rows = data?.traces ?? [];
  const more = data !== null && rows.length < data.total && limit < MAX_ROWS;
  const capped = data !== null && rows.length < data.total && limit >= MAX_ROWS;

  const empty = filtered
    ? { icon: SearchX, title: "No traces match", description: "Clear the search or filters to see everything." }
    : running
      ? {
          icon: Inbox,
          title: "Waiting for traces",
          description: "The collector is running. Point an app's SDK at it (see Connect) and traces appear here as they arrive.",
          action: (
            <Button variant="secondary" icon={Plug} onClick={onConnect}>
              How to connect
            </Button>
          ),
        }
      : {
          icon: Inbox,
          title: "No traces yet",
          description: "Start the collector, then point a Langfuse, LangSmith or OpenTelemetry SDK at it.",
          action: (
            <Stack direction="row" gap={2}>
              <Button variant="primary" icon={Play} loading={starting} onClick={onStart}>
                Start collector
              </Button>
              <Button variant="secondary" icon={Plug} onClick={onConnect}>
                How to connect
              </Button>
            </Stack>
          ),
        };

  return (
    <Section
      title="Traces"
      padding="none"
      toolbar={
        <Toolbar
          end={
            <Stack direction="row" gap={3} align="center">
              <span className="u-muted u-nowrap">
                {data ? `${data.total.toLocaleString()} trace${data.total === 1 ? "" : "s"}` : ""}
              </span>
              <Button
                variant="danger"
                size="sm"
                icon={Trash2}
                disabled={!data || data.total === 0}
                loading={busy === "*"}
                onClick={() => void handleClear()}
              >
                Clear all
              </Button>
            </Stack>
          }
        >
          <SearchInput value={search} onChange={setSearch} placeholder="Search name, input, user, model…" width={280} />
          <Select
            value={source}
            onChange={setSource}
            size="sm"
            options={[
              { value: "", label: "All sources" },
              { value: "langfuse", label: "Langfuse" },
              { value: "langsmith", label: "LangSmith" },
              { value: "otel", label: "OpenTelemetry" },
            ]}
          />
          {data && data.projects.length > 0 && (
            <Select
              value={project}
              onChange={setProject}
              size="sm"
              searchable={data.projects.length > 8}
              menuWidth="content"
              options={[{ value: "", label: "All projects" }, ...data.projects.map((p) => ({ value: p, label: p }))]}
            />
          )}
          <Checkbox label="Errors only" checked={errorsOnly} onChange={setErrorsOnly} />
        </Toolbar>
      }
      footer={
        more || capped || error ? (
          <Stack gap={3}>
            {error && <Callout tone="danger">{error}</Callout>}
            {capped && (
              <span className="u-muted">
                Showing the newest {MAX_ROWS.toLocaleString()}. Search or filter to reach older traces.
              </span>
            )}
            {more && (
              <Button variant="secondary" icon={ChevronDown} loading={loading} onClick={() => setLimit((l) => l + PAGE)}>
                Load more
              </Button>
            )}
          </Stack>
        ) : undefined
      }
    >
      <DataTable
        columns={columns}
        rows={rows}
        rowKey={(r) => r.id}
        loading={data === null}
        onRowClick={(r) => onOpen(r.id)}
        caption="Caught traces, newest first"
        empty={empty}
      />
    </Section>
  );
}
