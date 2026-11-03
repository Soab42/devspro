import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  Badge,
  Button,
  Callout,
  CodeBlock,
  EmptyState,
  Icon,
  Mono,
  Section,
  Skeleton,
  Stack,
  Toolbar,
  useResource,
} from "../../../ui";
import type { LucideIcon } from "../../../ui/icons";
import {
  Activity,
  ArrowLeft,
  Bot,
  BrainCircuit,
  Coins,
  ListTree,
  Search,
  SearchX,
  Timer,
  Trash2,
  Wrench,
} from "../../../ui/icons";
import { deleteTraces, getTrace, onIngested, type Observation, type TraceDetail } from "../api";
import {
  SOURCE_LABEL,
  buildTree,
  formatClock,
  formatCost,
  formatDuration,
  formatTokens,
  isEmptyValue,
  kindTone,
  prettyValue,
  spanUs,
} from "../utils";
import { Payload } from "./Payload";
import "./LlmTraces.css";

function kindIcon(kind: string): LucideIcon {
  switch (kind) {
    case "generation":
    case "embedding":
      return BrainCircuit;
    case "tool":
      return Wrench;
    case "agent":
      return Bot;
    case "retriever":
      return Search;
    case "event":
      return Activity;
    default:
      return ListTree;
  }
}

/** The key under which the trace itself, rather than an observation, is selected. */
const TRACE_ROW = "";

export function TraceView({ id, reload = 0, onBack }: { id: string; reload?: number; onBack: () => void }) {
  const confirmAction = useConfirm();
  // Wrapped, because useResource's `data === null` already means "not loaded
  // yet", and the command's null means "no such trace".
  const fetchTrace = useCallback(() => getTrace(id).then((found) => ({ found })), [id]);
  const trace = useResource(fetchTrace);
  const refreshOnReload = trace.refresh;
  useEffect(() => {
    if (reload > 0) void refreshOnReload();
  }, [reload, refreshOnReload]);
  const [selected, setSelected] = useState<string>(TRACE_ROW);
  const [deleting, setDeleting] = useState(false);

  // A trace still being written (an agent mid-run) fills in as it arrives.
  const refresh = trace.refresh;
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    void onIngested(() => void refresh()).then((u) => {
      if (gone) u();
      else unlisten = u;
    });
    return () => {
      gone = true;
      unlisten?.();
    };
  }, [refresh]);

  const detail = trace.data === null ? undefined : trace.data.found;
  const rows = useMemo(
    () => (detail ? buildTree(detail.observations, detail.summary.start_us, detail.summary.end_us) : []),
    [detail],
  );

  async function handleDelete() {
    const ok = await confirmAction("Its observations and scores go with it.", {
      title: "Delete this trace?",
      confirmLabel: "Delete",
      danger: true,
    });
    if (!ok) return;
    setDeleting(true);
    try {
      await deleteTraces([id]);
      onBack();
    } finally {
      setDeleting(false);
    }
  }

  const back = (
    <Button variant="ghost" icon={ArrowLeft} onClick={onBack}>
      All traces
    </Button>
  );

  if (trace.error) {
    return (
      <Stack gap={4}>
        <Toolbar>{back}</Toolbar>
        <Callout tone="danger" title="Could not read the trace">
          {trace.error}
        </Callout>
      </Stack>
    );
  }
  if (detail === undefined) {
    return (
      <Stack gap={4}>
        <Toolbar>{back}</Toolbar>
        <Skeleton height="16rem" />
      </Stack>
    );
  }
  if (detail === null) {
    return (
      <Stack gap={4}>
        <Toolbar>{back}</Toolbar>
        <EmptyState
          icon={SearchX}
          title="This trace is gone"
          description="It was deleted, or trimmed to keep the store under its size limit."
        />
      </Stack>
    );
  }

  const s = detail.summary;
  const selectedObs = detail.observations.find((o) => o.id === selected) ?? null;

  return (
    <Stack gap={4}>
      <Toolbar
        end={
          <Button variant="danger" icon={Trash2} loading={deleting} onClick={() => void handleDelete()}>
            Delete
          </Button>
        }
      >
        {back}
      </Toolbar>

      <div className="llmt-trace-head">
        <div className="llmt-trace-title">{s.name ?? "Unnamed trace"}</div>
        <Stack direction="row" gap={2} wrap align="center">
          <Badge tone="info">{SOURCE_LABEL[s.source]}</Badge>
          {s.project && <Badge mono>{s.project}</Badge>}
          <Badge icon={Timer}>{formatDuration(spanUs(s.start_us, s.end_us))}</Badge>
          {s.total_tokens > 0 && (
            <Badge>
              {formatTokens(s.input_tokens)} in · {formatTokens(s.output_tokens)} out
            </Badge>
          )}
          {s.cost !== null && <Badge icon={Coins}>{formatCost(s.cost)}</Badge>}
          {s.error_count > 0 && <Badge tone="danger">{s.error_count} error{s.error_count === 1 ? "" : "s"}</Badge>}
          {s.tags.map((t) => (
            <Badge key={t} tone="accent">
              {t}
            </Badge>
          ))}
          <span className="u-muted">{formatClock(s.start_us)}</span>
        </Stack>
      </div>

      <div className="llmt-trace">
        <Section title="Timeline" icon={ListTree} padding="none" meta={<span className="u-muted">{rows.length} observations</span>}>
          <div className="llmt-tree" role="tree" aria-label="Observations">
            <TreeButton
              active={selected === TRACE_ROW}
              depth={0}
              offset={0}
              width={1}
              icon={ListTree}
              name={s.name ?? "Trace"}
              kind="trace"
              meta={formatDuration(spanUs(s.start_us, s.end_us))}
              error={false}
              onSelect={() => setSelected(TRACE_ROW)}
            />
            {rows.map((r) => {
              const tokens = r.obs.total_tokens ?? sumTokens(r.obs);
              return (
                <TreeButton
                  key={r.obs.id}
                  active={selected === r.obs.id}
                  depth={r.depth + 1}
                  offset={r.offset}
                  width={r.width}
                  icon={kindIcon(r.obs.kind)}
                  name={r.obs.name ?? r.obs.kind}
                  kind={r.obs.kind}
                  meta={[
                    formatDuration(spanUs(r.obs.start_us, r.obs.end_us)),
                    tokens ? `${formatTokens(tokens)} tok` : null,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                  error={r.obs.level === "ERROR"}
                  onSelect={() => setSelected(r.obs.id)}
                />
              );
            })}
          </div>
        </Section>

        <div className="llmt-detail">
          {selectedObs ? <ObservationPanel obs={selectedObs} /> : <TracePanel detail={detail} />}
        </div>
      </div>
    </Stack>
  );
}

function sumTokens(o: Observation): number | null {
  if (o.input_tokens === null && o.output_tokens === null) return null;
  return (o.input_tokens ?? 0) + (o.output_tokens ?? 0);
}

interface TreeButtonProps {
  active: boolean;
  depth: number;
  offset: number;
  width: number;
  icon: LucideIcon;
  name: string;
  kind: string;
  meta: string;
  error: boolean;
  onSelect: () => void;
}

/** One row of the waterfall: indented by depth, its bar placed on the trace's timeline. */
function TreeButton({ active, depth, offset, width, icon, name, kind, meta, error, onSelect }: TreeButtonProps) {
  const cls = ["llmt-row", active && "llmt-row-active", error && "llmt-row-error"].filter(Boolean).join(" ");
  return (
    <button type="button" role="treeitem" aria-selected={active} className={cls} onClick={onSelect} title={name}>
      <span className="llmt-row-label" style={{ paddingInlineStart: `calc(${depth} * var(--space-4))` }}>
        <Icon icon={icon} size="xs" className={`llmt-kind-${kindTone(kind)}`} />
        <span className="llmt-row-name">{name}</span>
      </span>
      <span className="llmt-row-meta u-tnum">{meta}</span>
      <span className="llmt-bar-track" aria-hidden="true">
        <span
          className={`llmt-bar llmt-bar-${error ? "danger" : kindTone(kind)}`}
          style={{ left: `${(offset * 100).toFixed(3)}%`, width: `${(width * 100).toFixed(3)}%` }}
        />
      </span>
    </button>
  );
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="llmt-fact">
      <span className="llmt-label">{label}</span>
      <span>{children}</span>
    </div>
  );
}

function TracePanel({ detail }: { detail: TraceDetail }) {
  const s = detail.summary;
  return (
    <Section title="Trace" description={s.models.length > 0 ? `Models: ${s.models.join(", ")}` : undefined}>
      <Stack gap={4}>
        <div className="llmt-facts">
          <Fact label="Trace ID">
            <Mono copyable truncate>
              {s.id}
            </Mono>
          </Fact>
          {s.user_id && <Fact label="User">{s.user_id}</Fact>}
          {s.session_id && (
            <Fact label="Session">
              <Mono truncate>{s.session_id}</Mono>
            </Fact>
          )}
          {detail.environment && <Fact label="Environment">{detail.environment}</Fact>}
          {detail.release && <Fact label="Release">{detail.release}</Fact>}
          {detail.version && <Fact label="Version">{detail.version}</Fact>}
          <Fact label="Observations">
            {s.observation_count} ({s.generation_count} LLM call{s.generation_count === 1 ? "" : "s"})
          </Fact>
          {s.total_tokens > 0 && <Fact label="Tokens">{s.total_tokens.toLocaleString()}</Fact>}
        </div>
        {detail.scores.length > 0 && (
          <div className="llmt-payload">
            <span className="llmt-label">Scores</span>
            <Stack direction="row" gap={2} wrap>
              {detail.scores.map((score) => (
                <Badge key={score.id} tone="success" mono>
                  {score.name}: {score.value ?? score.string_value ?? "-"}
                  {score.comment ? ` (${score.comment})` : ""}
                </Badge>
              ))}
            </Stack>
          </div>
        )}
        <Payload label="Input" value={detail.input} />
        <Payload label="Output" value={detail.output} />
        {!isEmptyValue(detail.metadata) && <Payload label="Metadata" value={detail.metadata} />}
      </Stack>
    </Section>
  );
}

function ObservationPanel({ obs }: { obs: Observation }) {
  const duration = spanUs(obs.start_us, obs.end_us);
  const ttft = spanUs(obs.start_us, obs.completion_start_us);
  return (
    <Section
      title={obs.name ?? obs.kind}
      icon={kindIcon(obs.kind)}
      meta={<Badge tone={kindTone(obs.kind)}>{obs.kind}</Badge>}
    >
      <Stack gap={4}>
        {obs.level === "ERROR" && (
          <Callout tone="danger" title="Failed">
            {obs.status_message ?? "The SDK marked this observation as an error."}
          </Callout>
        )}
        {obs.level === "WARNING" && obs.status_message && <Callout tone="warning">{obs.status_message}</Callout>}
        <div className="llmt-facts">
          {obs.model && (
            <Fact label="Model">
              <Mono>{obs.model}</Mono>
            </Fact>
          )}
          <Fact label="Duration">{formatDuration(duration)}</Fact>
          {ttft !== null && <Fact label="First token">{formatDuration(ttft)}</Fact>}
          {(obs.input_tokens !== null || obs.output_tokens !== null || obs.total_tokens !== null) && (
            <Fact label="Tokens">
              {formatTokens(obs.input_tokens)} in · {formatTokens(obs.output_tokens)} out
              {obs.total_tokens !== null ? ` · ${formatTokens(obs.total_tokens)} total` : ""}
            </Fact>
          )}
          {obs.cost !== null && <Fact label="Cost">{formatCost(obs.cost)}</Fact>}
          {obs.start_us !== null && <Fact label="Started">{formatClock(obs.start_us)}</Fact>}
          <Fact label="ID">
            <Mono copyable truncate>
              {obs.id}
            </Mono>
          </Fact>
          {obs.level && obs.level !== "DEFAULT" && obs.level !== "ERROR" && <Fact label="Level">{obs.level}</Fact>}
        </div>
        <Payload label="Input" value={obs.input} />
        <Payload label="Output" value={obs.output} />
        {!isEmptyValue(obs.model_parameters) && (
          <div className="llmt-payload">
            <span className="llmt-label">Model parameters</span>
            <CodeBlock maxHeight="20vh">{prettyValue(obs.model_parameters)}</CodeBlock>
          </div>
        )}
        {!isEmptyValue(obs.metadata) && <Payload label="Metadata" value={obs.metadata} />}
        {!isEmptyValue(obs.attributes) && (
          <Section title="OpenTelemetry attributes" collapsible defaultCollapsed headingLevel={3} padding="none">
            <CodeBlock maxHeight="40vh">{prettyValue(obs.attributes)}</CodeBlock>
          </Section>
        )}
      </Stack>
    </Section>
  );
}
