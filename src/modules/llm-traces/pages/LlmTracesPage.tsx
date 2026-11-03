import { useState } from "react";
import { useModule } from "../../../app/ModuleRegistry";
import { Badge, Button, Callout, Page, Stack, StatusPill, Tabs, statusOf, useResource } from "../../../ui";
import { ListTree, Play, Plug, RefreshCw, Square } from "../../../ui/icons";
import { getStatus, setOptions, start, stop, type TracesStatus } from "../api";
import { ConnectTab } from "../components/ConnectTab";
import { TraceView } from "../components/TraceView";
import { TracesList } from "../components/TracesList";

type Tab = "traces" | "connect";

export default function LlmTracesPage() {
  const module = useModule("llm-traces");
  const status = useResource(getStatus, { pollMs: 10000 });
  const [tab, setTab] = useState<Tab>("traces");
  const [openId, setOpenId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // Bumped by Refresh, so the list (or the open trace) reloads too: traces can
  // arrive without an event, written by another DevsPro or a seed script.
  const [reload, setReload] = useState(0);
  const [error, setError] = useState<string | null>(null);

  async function run(action: () => Promise<TracesStatus>) {
    setBusy(true);
    setError(null);
    try {
      status.setData(await action());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  const data = status.data;
  const running = data?.running ?? false;
  const startOnCurrentPort = () => void run(() => start(data?.port ?? 4318));

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(status)}
      description={
        <>
          A local Langfuse and LangSmith: point an LLM app's SDK here and read its traces, with the prompts, responses,
          tool calls, tokens and timings, without an account and without anything leaving this machine. Beta.
        </>
      }
      meta={
        data && (
          <Stack direction="row" gap={2} align="center">
            <StatusPill
              variant={running ? "ok" : data.conflict ? "warn" : "neutral"}
              label={running ? `collecting on :${data.port}` : data.conflict ? "port in use" : "off"}
            />
            <Badge>
              {data.stats.traces.toLocaleString()} trace{data.stats.traces === 1 ? "" : "s"}
            </Badge>
          </Stack>
        )
      }
      actions={
        <>
          <Button
            variant="secondary"
            icon={RefreshCw}
            loading={status.refreshing}
            onClick={() => {
              void status.refresh();
              setReload((n) => n + 1);
            }}
          >
            Refresh
          </Button>
          {data &&
            (running ? (
              <Button variant="secondary" icon={Square} loading={busy} onClick={() => void run(stop)}>
                Stop
              </Button>
            ) : (
              <Button variant="primary" icon={Play} loading={busy} onClick={startOnCurrentPort}>
                Start
              </Button>
            ))}
        </>
      }
      tabs={
        <Tabs
          ariaLabel="LLM Traces views"
          active={tab}
          onChange={(id) => setTab(id as Tab)}
          tabs={[
            { id: "traces", label: "Traces", icon: ListTree },
            { id: "connect", label: "Connect", icon: Plug },
          ]}
        />
      }
    >
      <Stack gap={5}>
        {error && (
          <Callout tone="danger" onDismiss={() => setError(null)}>
            {error}
          </Callout>
        )}
        {tab === "traces" &&
          (openId ? (
            <TraceView
              id={openId}
              reload={reload}
              onBack={() => {
                setOpenId(null);
                void status.refresh();
              }}
            />
          ) : (
            <TracesList
              reload={reload}
              running={running}
              starting={busy}
              onStart={startOnCurrentPort}
              onConnect={() => setTab("connect")}
              onOpen={setOpenId}
              onChanged={() => void status.refresh()}
            />
          ))}
        {tab === "connect" && (
          <ConnectTab
            status={data}
            busy={busy}
            onStart={(port) => void run(() => start(port))}
            onStop={() => void run(stop)}
            onOptions={(startWithApp, maxTraces, docker) => void run(() => setOptions(startWithApp, maxTraces, docker))}
          />
        )}
      </Stack>
    </Page>
  );
}
