import { useState } from "react";
import { useModule } from "../../../app/ModuleRegistry";
import {
  Button,
  Callout,
  Field,
  FormGrid,
  FormRow,
  Page,
  Section,
  Select,
  Stack,
  Switch,
  TextArea,
  TextInput,
  Toolbar,
  statusOf,
  useResource,
} from "../../../ui";
import { Send, WandSparkles } from "../../../ui/icons";
import { listSites } from "../../nginx-ssl/api";
import {
  HttpRequestRecord,
  HttpResponse,
  clearHistory,
  deleteHistory,
  listHistory,
  saveHistory,
  sendRequest,
} from "../api";
import HeadersEditor from "../components/HeadersEditor";
import HistorySection from "../components/HistorySection";
import ResponsePanel from "../components/ResponsePanel";
import {
  BODY_METHODS,
  HeaderRow,
  METHODS,
  newHeaderRow,
  pairsToRows,
  prettyJson,
  rowsToPairs,
  siteUrl,
} from "../utils";

const METHOD_OPTIONS = METHODS.map((m) => ({ value: m, label: m }));
const TIMEOUT_MS = 30_000;

export default function HttpPage() {
  const module = useModule("http");

  const [method, setMethod] = useState<string>("GET");
  const [url, setUrl] = useState("");
  const [headers, setHeaders] = useState<HeaderRow[]>(() => [newHeaderRow()]);
  const [body, setBody] = useState("");
  const [followRedirects, setFollowRedirects] = useState(true);
  const [bodyError, setBodyError] = useState<string | null>(null);

  const [sending, setSending] = useState(false);
  const [response, setResponse] = useState<HttpResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  // History is the only stored data on the page; sites are a convenience and
  // must not block it if nginx is not set up.
  const history = useResource(() => listHistory(100));
  const sites = useResource(listSites);
  const siteOptions = (sites.data ?? [])
    .filter((s) => s.enabled)
    .map((s) => ({ value: s.hostname, label: s.hostname }));

  const hasBody = BODY_METHODS.has(method);

  async function send() {
    if (!url.trim() || sending) return;
    setSending(true);
    setError(null);
    setBodyError(null);
    const pairs = rowsToPairs(headers);
    const payload = hasBody && body.length > 0 ? body : null;
    try {
      const res = await sendRequest({
        method,
        url,
        headers: pairs,
        body: payload,
        followRedirects,
        timeoutMs: TIMEOUT_MS,
      });
      setResponse(res);
    } catch (e) {
      setResponse(null);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSending(false);
    }
    // Saved regardless of outcome: a failed request is exactly the one you
    // want to tweak and retry. A history write failing must not hide the
    // response, so it is best-effort.
    try {
      await saveHistory({ method, url, headers: pairs, body: payload });
      await history.refresh();
    } catch {
      /* ignore */
    }
  }

  function load(record: HttpRequestRecord) {
    setMethod(record.method);
    setUrl(record.url);
    setHeaders(pairsToRows(record.headers));
    setBody(record.body ?? "");
    setBodyError(null);
  }

  function prettyPrintBody() {
    const pretty = prettyJson(body);
    if (pretty === null) {
      setBodyError("Body is not valid JSON.");
      return;
    }
    setBody(pretty);
    setBodyError(null);
  }

  return (
    <Page
      title={module.label}
      icon={module.icon}
      description="Send a request to any URL, including your local HTTPS sites: the app's own root CA is trusted here even if it is not installed system-wide."
      status={statusOf(history)}
    >
      <Stack gap={5}>
        {history.error && history.data && (
          <Callout tone="warning" title="History could not be refreshed">
            {history.error}
          </Callout>
        )}

        <Section
          title="Request"
          toolbar={
            <Toolbar
              end={
                <Switch checked={followRedirects} onChange={setFollowRedirects} label="Follow redirects" />
              }
            >
              <Select
                aria-label="Fill URL from a site"
                size="sm"
                value=""
                placeholder={siteOptions.length ? "Sites" : "No sites"}
                disabled={siteOptions.length === 0}
                options={siteOptions}
                onChange={(hostname) => setUrl(siteUrl(hostname))}
              />
            </Toolbar>
          }
        >
          <Stack gap={5}>
            <FormRow>
              <Select aria-label="Method" value={method} onChange={setMethod} options={METHOD_OPTIONS} />
              <Field>
                <TextInput
                  mono
                  aria-label="URL"
                  placeholder="https://app.test/api/health"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void send();
                  }}
                  spellCheck={false}
                />
              </Field>
              <Button
                variant="primary"
                icon={Send}
                loading={sending}
                disabled={!url.trim()}
                onClick={() => void send()}
              >
                Send
              </Button>
            </FormRow>

            <FormGrid columns={2}>
              <Field label="Headers">
                <HeadersEditor rows={headers} onChange={setHeaders} disabled={sending} />
              </Field>
              <Field
                label="Body"
                hint={hasBody ? undefined : `${method} requests are sent without a body.`}
                error={bodyError}
              >
                <Stack gap={2}>
                  <TextArea
                    mono
                    rows={8}
                    aria-label="Request body"
                    placeholder={hasBody ? '{"key": "value"}' : ""}
                    value={body}
                    disabled={!hasBody || sending}
                    spellCheck={false}
                    onChange={(e) => {
                      setBody(e.target.value);
                      setBodyError(null);
                    }}
                  />
                  <div>
                    <Button
                      variant="ghost"
                      size="sm"
                      icon={WandSparkles}
                      disabled={!hasBody || !body.trim()}
                      onClick={prettyPrintBody}
                    >
                      JSON
                    </Button>
                  </div>
                </Stack>
              </Field>
            </FormGrid>
          </Stack>
        </Section>

        <ResponsePanel response={response} error={error} sending={sending} />

        <HistorySection
          records={history.data ?? []}
          loading={history.loading}
          onLoad={load}
          onDelete={async (id) => {
            await deleteHistory(id);
            await history.refresh();
          }}
          onClear={async () => {
            await clearHistory();
            await history.refresh();
          }}
        />
      </Stack>
    </Page>
  );
}
