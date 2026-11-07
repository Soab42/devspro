import { useCallback, useEffect, useState } from "react";
import {
  Badge,
  Button,
  Callout,
  CodeBlock,
  type Column,
  CopyButton,
  DataTable,
  Field,
  FormRow,
  Mono,
  Section,
  SegmentedControl,
  Select,
  Stack,
  StatusPill,
  Switch,
  TextInput,
  useResource,
} from "../../../ui";
import { Activity, Container, Inbox, Monitor, Play, Plug, Square, WandSparkles } from "../../../ui/icons";
import { onIngested, recentRequests, suggestPort, type RequestLog, type TracesStatus } from "../api";
import { SOURCE_LABEL, formatBytes, formatClock } from "../utils";
import "./LlmTraces.css";

type Sdk = "langfuse" | "langsmith" | "otel";

interface Snippet {
  env: string;
  notes: string[];
}

/** What to put in an app's environment, with the collector's real address. */
export function snippets(baseUrl: string): Record<Sdk, Snippet> {
  return {
    langfuse: {
      env: [
        `LANGFUSE_BASE_URL=${baseUrl}`,
        `LANGFUSE_HOST=${baseUrl}`,
        "LANGFUSE_PUBLIC_KEY=pk-lf-local",
        "LANGFUSE_SECRET_KEY=sk-lf-local",
      ].join("\n"),
      notes: [
        "Python v3 and later read LANGFUSE_BASE_URL (LANGFUSE_HOST still works), and so does JS v4 and later. Python v2 reads LANGFUSE_HOST, and JS v3 reads LANGFUSE_BASEURL.",
        "Any keys work. The public key names the project here, so give each app its own: pk-lf-checkout, pk-lf-support-bot.",
        "Prompt management, datasets and media uploads aren't part of this beta, so calls to them fail and are logged by the SDK. Tracing is unaffected.",
      ],
    },
    langsmith: {
      env: [
        "LANGSMITH_TRACING=true",
        `LANGSMITH_ENDPOINT=${baseUrl}`,
        "LANGSMITH_API_KEY=local",
        "LANGSMITH_PROJECT=my-app",
      ].join("\n"),
      notes: [
        "Works with @traceable, wrap_openai and LangChain or LangGraph in Python and JS. The older LANGCHAIN_TRACING_V2 and LANGCHAIN_ENDPOINT names work too.",
        "Any API key works. LANGSMITH_PROJECT groups traces here.",
      ],
    },
    otel: {
      env: [
        `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=${baseUrl}/v1/traces`,
        "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL=http/protobuf",
        "OTEL_SERVICE_NAME=my-app",
      ].join("\n"),
      notes: [
        "Any OTLP/HTTP exporter, protobuf or JSON: OpenLLMetry, OpenInference (Arize Phoenix), the Vercel AI SDK, Pydantic AI and Logfire, or spans following the GenAI semantic conventions.",
        "OTLP over gRPC (port 4317) isn't supported. OTEL_SERVICE_NAME names the project here.",
      ],
    },
  };
}

const SDK_LABEL: Record<Sdk, string> = { langfuse: "Langfuse", langsmith: "LangSmith", otel: "OpenTelemetry" };

type Where = "host" | "docker";

/**
 * The same variables as a docker-compose service. On Linux the container
 * needs `host.docker.internal` mapped to the host (Docker Desktop has it
 * built in, where the line is harmless).
 */
export function composeFragment(env: string): string {
  const vars = env.split("\n").map((line) => {
    const eq = line.indexOf("=");
    return `      ${line.slice(0, eq)}: "${line.slice(eq + 1)}"`;
  });
  return [
    "services:",
    "  your-app:",
    "    environment:",
    ...vars,
    "    extra_hosts:",
    '      - "host.docker.internal:host-gateway"',
  ].join("\n");
}

function isThisMachine(client: string): boolean {
  return client === "::1" || client.startsWith("127.");
}

const MAX_TRACE_OPTIONS = [500, 2000, 10000, 50000].map((n) => ({
  value: String(n),
  label: `Newest ${n.toLocaleString()} traces`,
}));

interface Props {
  status: TracesStatus | null;
  busy: boolean;
  onStart: (port: number) => void;
  onStop: () => void;
  onOptions: (startWithApp: boolean, maxTraces: number, docker: boolean) => void;
}

export function ConnectTab({ status, busy, onStart, onStop, onOptions }: Props) {
  const [port, setPort] = useState("4318");
  const [sdk, setSdk] = useState<Sdk>("langfuse");
  const [where, setWhere] = useState<Where>("host");
  const [portError, setPortError] = useState<string | null>(null);

  useEffect(() => {
    if (status) setPort(String(status.port));
  }, [status?.port]);

  const fetchRecent = useCallback(() => recentRequests(), []);
  const recent = useResource(fetchRecent, { pollMs: 5000 });
  const refreshRecent = recent.refresh;
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    void onIngested(() => void refreshRecent()).then((u) => {
      if (gone) u();
      else unlisten = u;
    });
    return () => {
      gone = true;
      unlisten?.();
    };
  }, [refreshRecent]);

  function handleStart() {
    const n = Number(port);
    if (!Number.isInteger(n) || n < 1 || n > 65535) {
      setPortError("Enter a port between 1 and 65535.");
      return;
    }
    setPortError(null);
    onStart(n);
  }

  async function handleSuggest() {
    const free = await suggestPort(4318);
    if (free) setPort(String(free));
  }

  const running = status?.running ?? false;
  const shownPort = status ? (running ? status.port : Number(port) || status.port) : 4318;
  const baseUrl = `http://localhost:${shownPort}`;
  const snippet = snippets(where === "docker" ? `http://host.docker.internal:${shownPort}` : baseUrl)[sdk];
  const snippetText = where === "docker" ? composeFragment(snippet.env) : snippet.env;
  const dockerAddresses = status?.docker_addresses ?? [];

  // Two requests can share a microsecond; the position keeps keys unique.
  const logRows = (recent.data ?? []).map((r, i) => ({ ...r, key: `${i}-${r.time_us}` }));
  const columns: Column<RequestLog & { key: string }>[] = [
    {
      key: "time",
      header: "Time",
      priority: 1,
      width: "1px",
      minWidth: "7rem",
      render: (r) => <span className="u-tnum u-nowrap">{formatClock(r.time_us)}</span>,
    },
    {
      key: "request",
      header: "Request",
      priority: 1,
      mono: true,
      render: (r) => `${r.method} ${r.path}`,
    },
    {
      key: "status",
      header: "Status",
      priority: 1,
      width: "1px",
      minWidth: "5rem",
      render: (r) => (
        <StatusPill variant={r.status < 300 ? "ok" : r.status === 404 ? "warn" : "bad"} label={String(r.status)} size="sm" />
      ),
    },
    {
      key: "client",
      header: "From",
      priority: 2,
      width: "1px",
      minWidth: "8rem",
      render: (r) =>
        isThisMachine(r.client) ? (
          <span className="u-muted u-nowrap">this machine</span>
        ) : (
          <Badge tone="accent" mono>
            {r.client}
          </Badge>
        ),
    },
    {
      key: "source",
      header: "Protocol",
      priority: 2,
      width: "1px",
      minWidth: "9rem",
      render: (r) => (r.source ? <Badge tone="info">{SOURCE_LABEL[r.source]}</Badge> : <span className="u-muted">-</span>),
    },
    {
      key: "note",
      header: "Result",
      priority: 2,
      render: (r) => <span className={r.status < 300 ? "" : "u-muted"}>{r.note || "-"}</span>,
    },
  ];

  return (
    <Stack gap={5}>
      <Section
        title="Collector"
        icon={Plug}
        meta={
          <StatusPill
            variant={running ? "ok" : status?.conflict ? "warn" : "neutral"}
            label={running ? `listening on ${status?.port}` : status?.conflict ? "port in use" : "off"}
          />
        }
      >
        <Stack gap={5}>
          {!running && status?.conflict && (
            <Callout tone="warning" title="Port in use">
              {status.conflict} Pick another port, or stop that program if it's yours. DevsPro won't stop a program it
              didn't start.
            </Callout>
          )}
          {running ? (
            <>
              <Field label="Address">
                <FormRow>
                  <Mono variant="chip" copyable>
                    {baseUrl}
                  </Mono>
                  <Button variant="secondary" icon={Square} loading={busy} onClick={onStop}>
                    Stop
                  </Button>
                </FormRow>
              </Field>
            </>
          ) : (
            <Field label="Port" htmlFor="llmt-port" error={portError ?? undefined}>
              <FormRow>
                <TextInput
                  id="llmt-port"
                  mono
                  inputMode="numeric"
                  className="field-input-narrow"
                  value={port}
                  onChange={(e) => setPort(e.target.value)}
                />
                <Button variant="secondary" icon={WandSparkles} onClick={() => void handleSuggest()}>
                  Suggest
                </Button>
                <Button variant="primary" icon={Play} loading={busy} onClick={handleStart}>
                  Start
                </Button>
              </FormRow>
            </Field>
          )}
          <Switch
            label="Start with DevsPro"
            description="Starts the collector on this port whenever DevsPro opens. It always stops when DevsPro quits."
            checked={status?.start_with_app ?? false}
            disabled={!status || busy}
            onChange={(on) => status && onOptions(on, status.max_traces, status.docker)}
          />
          <Switch
            label="Accept traces from Docker containers"
            description={
              status?.docker && running && dockerAddresses.length > 0
                ? `Also listening on Docker's networks (${dockerAddresses.join(", ")}), which only this machine's containers can reach. New networks are picked up within 10 seconds.`
                : "Also listens on Docker's networks, which only this machine's containers can reach, so an app in a container can send traces to host.docker.internal."
            }
            checked={status?.docker ?? true}
            disabled={!status || busy}
            onChange={(on) => status && onOptions(status.start_with_app, status.max_traces, on)}
          />
          <Field
            label="Keep"
            hint={
              status
                ? `${status.stats.traces.toLocaleString()} traces and ${status.stats.observations.toLocaleString()} observations stored, ${formatBytes(status.stats.bytes)} on disk. Older traces are deleted as new ones arrive.`
                : undefined
            }
          >
            <Select
              value={String(status?.max_traces ?? 2000)}
              disabled={!status || busy}
              onChange={(v) => status && onOptions(status.start_with_app, Number(v), status.docker)}
              options={MAX_TRACE_OPTIONS}
            />
          </Field>
        </Stack>
      </Section>

      <Section
        title="Connect your app"
        description="Set these in the app's environment (its .env file or docker-compose.yml) and restart it. Nothing leaves this machine."
      >
        <Stack gap={4}>
          <Stack direction="row" gap={3} wrap align="center">
            <SegmentedControl<Sdk>
              ariaLabel="SDK"
              value={sdk}
              onChange={setSdk}
              options={(Object.keys(SDK_LABEL) as Sdk[]).map((value) => ({ value, label: SDK_LABEL[value] }))}
            />
            <SegmentedControl<Where>
              ariaLabel="Where the app runs"
              value={where}
              onChange={setWhere}
              options={[
                { value: "host", label: "On this machine", icon: Monitor },
                { value: "docker", label: "In Docker", icon: Container },
              ]}
            />
          </Stack>
          {where === "docker" && status && !status.docker && (
            <Callout tone="warning" title="Docker access is off">
              Turn on "Accept traces from Docker containers" above, or containers can't reach the collector.
            </Callout>
          )}
          <div className="llmt-snippet">
            <CodeBlock wrap={false}>{snippetText}</CodeBlock>
            <div className="llmt-snippet-copy">
              <CopyButton value={snippetText} label={where === "docker" ? "Copy compose snippet" : "Copy environment"} />
            </div>
          </div>
          <ul className="llmt-notes">
            {where === "docker" && (
              <>
                <li>
                  With docker run, pass the same variables with -e, plus --add-host=host.docker.internal:host-gateway.
                  Docker Desktop (macOS, Windows) knows host.docker.internal already.
                </li>
                <li>
                  If a firewall is on (ufw, firewalld), allow the port from Docker's networks, for example sudo ufw
                  allow from 172.16.0.0/12 to any port {shownPort} proto tcp.
                </li>
              </>
            )}
            {snippet.notes.map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
        </Stack>
      </Section>

      <Section
        title="Recent requests"
        icon={Activity}
        padding="none"
        description="The last 30 requests the collector answered, newest first. Use this to check that an app is reaching it."
      >
        <DataTable
          columns={columns}
          rows={logRows}
          rowKey={(r) => r.key}
          loading={recent.loading}
          density="compact"
          caption="Recent collector requests"
          empty={{
            icon: Inbox,
            title: "No requests yet",
            description: running
              ? "Nothing has reached the collector since it started."
              : "Start the collector, then run your app.",
          }}
        />
      </Section>
    </Stack>
  );
}
