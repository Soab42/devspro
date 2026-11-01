import { useCallback, useEffect, useState } from "react";
import { useModule } from "../../../app/ModuleRegistry";
import {
  Button,
  Callout,
  CodeBlock,
  CopyButton,
  EmptyState,
  Mono,
  Page,
  Section,
  SelectOption,
  Select,
  Stack,
  TextInput,
  useResource,
} from "../../../ui";
import { RefreshCw, ScrollText } from "../../../ui/icons";
import { listLogSources, tailLog, LogSource } from "../api";
import "./LogsPage.css";

const KIND_LABEL: Record<LogSource["kind"], string> = {
  access: "access.log",
  error: "error.log",
  laravel: "laravel.log",
};

function sourceLabel(source: LogSource): string {
  return `${source.site_name} — ${KIND_LABEL[source.kind]}`;
}

export default function LogsPage() {
  const module = useModule("logs");
  const sources = useResource(listLogSources);

  const [selectedId, setSelectedId] = useState("");
  const [lines, setLines] = useState(200);
  const [output, setOutput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const list = sources.data ?? [];

  // Keep the selection valid as sources come and go.
  useEffect(() => {
    if (list.length === 0) return;
    if (!list.some((s) => s.id === selectedId)) setSelectedId(list[0].id);
  }, [list, selectedId]);

  const loadLog = useCallback(async () => {
    if (!selectedId) return;
    setError(null);
    setLoading(true);
    try {
      setOutput(await tailLog(selectedId, lines));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [selectedId, lines]);

  useEffect(() => {
    setOutput("");
    void loadLog();
  }, [selectedId]); // eslint-disable-line react-hooks/exhaustive-deps

  const options: SelectOption[] = list.map((s) => ({
    value: s.id,
    label: sourceLabel(s),
  }));
  const selected = list.find((s) => s.id === selectedId);

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={sources.loading ? { kind: "loading" } : { kind: "ready" }}
      /* The log content IS the page, so its controls belong in the header
         rather than stacked in a card above it. */
      actions={
        list.length > 0 && (
          <>
            <Select
              id="log-source"
              value={selectedId}
              onChange={setSelectedId}
              options={options}
              aria-label="Log source"
            />
            <TextInput
              id="log-lines"
              type="number"
              min={10}
              max={5000}
              value={lines}
              onChange={(e) => setLines(Number(e.target.value) || 200)}
              className="field-input-narrow"
              aria-label="Number of lines to show"
            />
            <Button
              variant="secondary"
              icon={RefreshCw}
              loading={loading}
              disabled={!selectedId}
              onClick={loadLog}
            >
              Refresh
            </Button>
          </>
        )
      }
    >
      {list.length === 0 ? (
        <Section padding="none">
          <EmptyState
            size="md"
            icon={ScrollText}
            title="No log files yet"
            description="Add a site under Web Server & SSL and generate some traffic first."
          />
        </Section>
      ) : (
        <Stack gap={4} className="logs-stack">
          {error && <Callout tone="danger">{error}</Callout>}

          <div className="logs-meta">
            {selected && <Mono truncate>{selected.path}</Mono>}
            <CopyButton value={output} label="Copy log output" size="md" />
          </div>

          <CodeBlock maxHeight="100%" autoScrollBottom className="logs-output">
            {output || "(no output yet)"}
          </CodeBlock>
        </Stack>
      )}
    </Page>
  );
}
