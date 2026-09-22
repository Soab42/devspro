import { useMemo, useState } from "react";
import {
  Badge,
  Callout,
  CodeBlock,
  Column,
  CopyButton,
  DataTable,
  EmptyState,
  Mono,
  Section,
  Spinner,
  Stack,
  StatusPill,
  Tabs,
} from "../../../ui";
import { Send } from "../../../ui/icons";
import type { HeaderPair, HttpResponse } from "../api";
import { displayBody, findHeader, formatBytes, formatElapsed, statusVariant } from "../utils";

interface Props {
  response: HttpResponse | null;
  error: string | null;
  sending: boolean;
}

type HeaderRowView = { key: string; value: string; index: number };

const HEADER_COLUMNS: Column<HeaderRowView>[] = [
  {
    key: "key",
    header: "Header",
    priority: 1,
    mono: true,
    width: "30%",
    sortValue: (r) => r.key,
    render: (r) => r.key,
  },
  {
    key: "value",
    header: "Value",
    priority: 1,
    mono: true,
    wrap: true,
    render: (r) => r.value,
  },
];

function toRows(headers: HeaderPair[]): HeaderRowView[] {
  return headers.map(([key, value], index) => ({ key, value, index }));
}

export default function ResponsePanel({ response, error, sending }: Props) {
  const [tab, setTab] = useState<"body" | "headers">("body");

  const contentType = response ? findHeader(response.headers, "content-type") : null;
  const body = useMemo(
    () => (response ? displayBody(response.body, contentType) : ""),
    [response, contentType],
  );

  const meta = response && (
    <Stack direction="row" gap={3} align="center" wrap>
      <StatusPill
        variant={statusVariant(response.status)}
        label={`${response.status} ${response.status_text}`.trim()}
      />
      <Badge tone="neutral" mono>
        {formatElapsed(response.elapsed_ms)}
      </Badge>
      <Badge tone="neutral" mono>
        {formatBytes(response.size_bytes)}
      </Badge>
      {contentType && (
        <Mono truncate={"24rem"} title={contentType}>
          {contentType}
        </Mono>
      )}
    </Stack>
  );

  return (
    <Section
      title="Response"
      padding="none"
      meta={meta}
      actions={response && tab === "body" && body ? <CopyButton value={body} label="Copy body" /> : undefined}
      toolbar={
        response ? (
          <Tabs
            ariaLabel="Response view"
            active={tab}
            onChange={(id) => setTab(id as "body" | "headers")}
            tabs={[
              { id: "body", label: "Body" },
              { id: "headers", label: `Headers (${response.headers.length})` },
            ]}
          />
        ) : undefined
      }
    >
      {sending ? (
        <EmptyState icon={Send} title="Sending" description={<Spinner />} size="sm" />
      ) : error ? (
        <Callout tone="danger" title="Request failed">
          {error}
        </Callout>
      ) : !response ? (
        <EmptyState
          icon={Send}
          title="No response yet"
          description="Send a request to see its status, headers and body here."
          size="sm"
        />
      ) : tab === "headers" ? (
        <DataTable
          columns={HEADER_COLUMNS}
          rows={toRows(response.headers)}
          rowKey={(r) => `${r.index}`}
          density="compact"
          caption="Response headers"
          empty={{ title: "No headers", description: "The server sent no response headers." }}
        />
      ) : (
        <Stack gap={3}>
          {response.truncated && (
            <Callout tone="warning">
              Body shown is the first {formatBytes(response.body.length)} of{" "}
              {formatBytes(response.size_bytes)}.
            </Callout>
          )}
          {body ? (
            <CodeBlock maxHeight="60vh">{body}</CodeBlock>
          ) : (
            <EmptyState title="Empty body" size="sm" />
          )}
        </Stack>
      )}
    </Section>
  );
}
