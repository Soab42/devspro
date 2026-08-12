import { useCallback, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import { dockerBlockedStatus } from "../../docker/blocked";
import {
  Button,
  Callout,
  Field,
  FormRow,
  Mono,
  Page,
  Section,
  Stack,
  StatusPill,
  TextInput,
  statusOf,
  useResource,
} from "../../../ui";
import {
  ExternalLink,
  Play,
  RefreshCw,
  Square,
  Trash2,
  WandSparkles,
} from "../../../ui/icons";
import {
  DEFAULT_HTTP_PORT,
  DEFAULT_SMTP_PORT,
  MailcatcherStatus,
  detectDocker,
  getStatus,
  remove,
  start,
  stop,
  suggestPort,
} from "../api";

export default function MailCatcherPage() {
  const confirmAction = useConfirm();
  const module = useModule("mailcatcher");

  const detection = useResource(detectDocker);
  const dockerUsable = Boolean(detection.data?.found && detection.data.daemon_reachable);
  const status = useResource(getStatus, { enabled: dockerUsable });

  const [smtpPort, setSmtpPort] = useState(String(DEFAULT_SMTP_PORT));
  const [httpPort, setHttpPort] = useState(String(DEFAULT_HTTP_PORT));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshAll = useCallback(async () => {
    await detection.refresh();
    await status.refresh();
  }, [detection.refresh, status.refresh]);

  async function run(action: () => Promise<MailcatcherStatus>) {
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

  async function handleSuggestPort(target: "smtp" | "http") {
    const suggestion = await suggestPort(
      target === "smtp" ? DEFAULT_SMTP_PORT : DEFAULT_HTTP_PORT,
    );
    if (!suggestion) return;
    if (target === "smtp") setSmtpPort(String(suggestion));
    else setHttpPort(String(suggestion));
  }

  function handleStart() {
    const smtp = Number(smtpPort);
    const http = Number(httpPort);
    if (!smtp || !http) {
      setError("Both ports are required.");
      return;
    }
    void run(() => start(smtp, http));
  }

  async function handleRemove() {
    const ok = await confirmAction(
      "Any caught mail is discarded. Nothing else is affected.",
      { title: "Remove the Mail Catcher container?", confirmLabel: "Remove", danger: true },
    );
    if (ok) void run(remove);
  }

  const data = status.data;
  const configured = data?.configured ?? false;
  const running = data?.running ?? false;
  const stateLabel = running
    ? (data?.state ?? "running")
    : configured
      ? "stopped"
      : "not started";

  return (
    <Page
      title={module.label}
      icon={module.icon}
      width="narrow"
      status={statusOf(detection, dockerBlockedStatus(detection.data, refreshAll))}
      description={
        <>
          Runs{" "}
          <a href="https://github.com/axllent/mailpit" target="_blank" rel="noreferrer">
            Mailpit
          </a>{" "}
          in a container so outgoing email from your local apps lands here instead of
          hitting real SMTP. Point your app's mail config at the SMTP port below, then
          check the web UI to see what it sent. No accounts or credentials — Mailpit has
          none by default.
        </>
      }
      actions={
        <Button
          variant="secondary"
          icon={RefreshCw}
          loading={status.refreshing}
          onClick={refreshAll}
        >
          Refresh
        </Button>
      }
    >
      <Section
        title="Mailpit"
        meta={<StatusPill variant={running ? "ok" : "neutral"} label={stateLabel} />}
      >
        <Stack gap={5}>
          {error && (
            <Callout tone="danger" onDismiss={() => setError(null)}>
              {error}
            </Callout>
          )}

          {!configured ? (
            <>
              <Field label="SMTP port" htmlFor="mc-smtp-port">
                <FormRow>
                  <TextInput
                    id="mc-smtp-port"
                    mono
                    inputMode="numeric"
                    className="field-input-narrow"
                    value={smtpPort}
                    onChange={(e) => setSmtpPort(e.target.value)}
                  />
                  <Button
                    variant="secondary"
                    icon={WandSparkles}
                    onClick={() => handleSuggestPort("smtp")}
                  >
                    Suggest
                  </Button>
                </FormRow>
              </Field>

              <Field label="Web UI port" htmlFor="mc-http-port">
                <FormRow>
                  <TextInput
                    id="mc-http-port"
                    mono
                    inputMode="numeric"
                    className="field-input-narrow"
                    value={httpPort}
                    onChange={(e) => setHttpPort(e.target.value)}
                  />
                  <Button
                    variant="secondary"
                    icon={WandSparkles}
                    onClick={() => handleSuggestPort("http")}
                  >
                    Suggest
                  </Button>
                </FormRow>
              </Field>

              <FormRow>
                <Button variant="primary" icon={Play} loading={busy} onClick={handleStart}>
                  Start
                </Button>
              </FormRow>
            </>
          ) : (
            <>
              {/* Was a readOnly <input> used purely so the value could be
                  selected and copied — a copyable Mono says what it means. */}
              <Field label="SMTP">
                <Mono variant="chip" copyable>
                  {`localhost:${data?.smtp_port ?? ""}`}
                </Mono>
              </Field>

              <Field label="Web UI">
                {running && data?.web_url ? (
                  <FormRow>
                    <Mono variant="chip" copyable>
                      {data.web_url}
                    </Mono>
                    <Button
                      variant="secondary"
                      size="sm"
                      icon={ExternalLink}
                      onClick={() => void openUrl(data.web_url!)}
                    >
                      Open
                    </Button>
                  </FormRow>
                ) : (
                  <Mono variant="chip" copyable>
                    {`localhost:${data?.http_port ?? ""}`}
                  </Mono>
                )}
              </Field>

              <FormRow>
                {running ? (
                  <Button
                    variant="secondary"
                    icon={Square}
                    loading={busy}
                    onClick={() => void run(stop)}
                  >
                    Stop
                  </Button>
                ) : (
                  <Button variant="primary" icon={Play} loading={busy} onClick={handleStart}>
                    Start
                  </Button>
                )}
                <div className="u-flex-1" />
                <Button
                  variant="danger"
                  icon={Trash2}
                  disabled={busy}
                  onClick={handleRemove}
                >
                  Remove
                </Button>
              </FormRow>
            </>
          )}
        </Stack>
      </Section>
    </Page>
  );
}
