import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import {
  Button,
  Callout,
  Checkbox,
  Field,
  FormRow,
  Meter,
  Mono,
  Page,
  Section,
  SegmentedControl,
  Stack,
  StatusPill,
  Switch,
  TextInput,
  statusOf,
  useResource,
} from "../../../ui";
import type { StatusVariant } from "../../../ui";
import {
  Container,
  Download,
  ExternalLink,
  Package,
  Play,
  RefreshCw,
  Square,
  Trash2,
  WandSparkles,
} from "../../../ui/icons";
import {
  Backend,
  InstallProgress,
  MailcatcherStatus,
  getStatus,
  install,
  onInstallProgress,
  removeContainer,
  setOptions,
  start,
  stop,
  suggestPort,
  uninstall,
} from "../api";

const BACKEND_LABEL: Record<Backend, string> = { native: "Built-in", docker: "Docker" };

function pillFor(data: MailcatcherStatus | null): { variant: StatusVariant; label: string } {
  switch (data?.state) {
    case "running":
      return { variant: "ok", label: `running · ${BACKEND_LABEL[data.backend]}` };
    case "external":
      return { variant: "info", label: "running outside DevsPro" };
    case "conflict":
      return { variant: "warn", label: "port in use" };
    default:
      return { variant: "neutral", label: "off" };
  }
}

function mb(bytes: number) {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export default function MailCatcherPage() {
  const confirmAction = useConfirm();
  const module = useModule("mailcatcher");
  const status = useResource(getStatus);
  const data = status.data;

  const [backend, setBackend] = useState<Backend>("native");
  const [smtpPort, setSmtpPort] = useState("1025");
  const [httpPort, setHttpPort] = useState("8025");
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [deleteMail, setDeleteMail] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The form starts from what the backend last used, once it's known — and again
  // whenever a start/stop moves the ports, but never mid-edit otherwise.
  useEffect(() => {
    if (!data) return;
    setBackend(data.backend);
    setSmtpPort(String(data.smtp_port));
    setHttpPort(String(data.http_port));
  }, [data?.backend, data?.smtp_port, data?.http_port]);

  async function run(action: () => Promise<MailcatcherStatus>) {
    setBusy(true);
    setError(null);
    try {
      status.setData(await action());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }

  async function installWithProgress() {
    setProgress({ downloaded: 0, total: null });
    const unlisten = await onInstallProgress(setProgress);
    try {
      return await install();
    } finally {
      unlisten();
    }
  }

  async function handleSuggestPort(target: "smtp" | "http") {
    const suggestion = await suggestPort(target === "smtp" ? 1025 : 8025);
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
    void run(async () => {
      if (backend === "native" && !data?.native_available) await installWithProgress();
      return start(backend, smtp, http);
    });
  }

  async function handleUninstall() {
    const ok = await confirmAction(
      deleteMail
        ? "Stops the built-in Mailpit if it's running, deletes it, and deletes the mail it caught."
        : "Stops the built-in Mailpit if it's running and deletes it. Caught mail is kept for a later reinstall.",
      { title: "Uninstall the built-in Mailpit?", confirmLabel: "Uninstall", danger: true },
    );
    if (ok) void run(() => uninstall(deleteMail));
  }

  async function handleRemoveContainer() {
    const ok = await confirmAction("Any mail it caught is discarded. Nothing else is affected.", {
      title: "Remove the Mail Catcher container?",
      confirmLabel: "Remove",
      danger: true,
    });
    if (ok) void run(removeContainer);
  }

  const pill = pillFor(data);
  const state = data?.state ?? "off";
  const showsForm = state === "off" || state === "conflict";
  const needsDownload = Boolean(data && !data.native_available && backend === "native");
  const percent =
    progress?.total ? Math.round((progress.downloaded / progress.total) * 100) : 0;

  const connection = data && (state === "running" || state === "external") && (
    <>
      <Field label="SMTP">
        <Mono variant="chip" copyable>
          {`localhost:${data.smtp_port}`}
        </Mono>
      </Field>
      {data.web_url && (
        <Field label="Web UI">
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
        </Field>
      )}
    </>
  );

  return (
    <Page
      title={module.label}
      icon={module.icon}
      width="narrow"
      status={statusOf(status)}
      description={
        <>
          Runs{" "}
          <a href="https://github.com/axllent/mailpit" target="_blank" rel="noreferrer">
            Mailpit
          </a>{" "}
          so outgoing email from your local apps lands here instead of hitting real SMTP.
          It runs built in, or in Docker if you have it. It's off until you start it, and
          DevsPro only ever stops a Mailpit it started itself.
        </>
      }
      actions={
        <Button
          variant="secondary"
          icon={RefreshCw}
          loading={status.refreshing}
          onClick={() => void status.refresh()}
        >
          Refresh
        </Button>
      }
    >
      <Stack gap={5}>
        <Section title="Mailpit" meta={<StatusPill variant={pill.variant} label={pill.label} />}>
          <Stack gap={5}>
            {error && (
              <Callout tone="danger" onDismiss={() => setError(null)}>
                {error}
              </Callout>
            )}

            {state === "external" && (
              <Callout tone="info" title="Running outside DevsPro">
                A Mailpit that DevsPro didn't start is already on these ports, so this page
                connects to it as it is. Stop or change it wherever it was started.
              </Callout>
            )}

            {state === "conflict" && data?.conflict && (
              <Callout tone="warning" title="Port in use">
                {data.conflict} Pick other ports below — DevsPro won't stop a program it
                didn't start.
              </Callout>
            )}

            {connection}

            {state === "running" && (
              <FormRow>
                <Button
                  variant="secondary"
                  icon={Square}
                  loading={busy}
                  onClick={() => void run(stop)}
                >
                  Stop
                </Button>
              </FormRow>
            )}

            {showsForm && (
              <>
                <Field label="Run with">
                  <SegmentedControl<Backend>
                    ariaLabel="Run Mailpit with"
                    value={backend}
                    onChange={setBackend}
                    options={[
                      { value: "native", label: "Built-in", icon: Package },
                      { value: "docker", label: "Docker", icon: Container },
                    ]}
                  />
                </Field>

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

                {needsDownload && !progress && (
                  <Callout tone="info" icon={Download}>
                    Downloads Mailpit {data!.native_version} (about 10 MB) from its GitHub
                    release the first time, checks it against the published checksum, and
                    keeps it for next time.
                  </Callout>
                )}

                {progress && (
                  <Meter
                    variant="inline"
                    percent={percent}
                    valueLabel={
                      progress.total
                        ? `${mb(progress.downloaded)} of ${mb(progress.total)}`
                        : mb(progress.downloaded)
                    }
                  />
                )}

                <FormRow>
                  <Button variant="primary" icon={Play} loading={busy} onClick={handleStart}>
                    {needsDownload ? "Install and start" : "Start"}
                  </Button>
                </FormRow>
              </>
            )}
          </Stack>
        </Section>

        <Section
          title="When DevsPro opens and closes"
          description="Both apply to a Mailpit DevsPro started. One running outside DevsPro is never touched."
        >
          <Stack gap={4}>
            <Switch
              label="Start with DevsPro"
              description={`Starts the ${BACKEND_LABEL[data?.backend ?? "native"]} Mailpit on the last-used ports when DevsPro opens, unless something is already there.`}
              checked={data?.start_with_app ?? false}
              disabled={!data || busy}
              onChange={(on) => void run(() => setOptions(on, data!.keep_running))}
            />
            <Switch
              label="Keep running after DevsPro closes"
              description="Off: quitting DevsPro stops it. On: it keeps catching mail in the background, and you can stop it here next time."
              checked={data?.keep_running ?? false}
              disabled={!data || busy}
              onChange={(on) => void run(() => setOptions(data!.start_with_app, on))}
            />
          </Stack>
        </Section>

        {(data?.native_downloaded || data?.docker_configured) && (
          <Section title="Installed by DevsPro">
            <Stack gap={5}>
              {data.native_downloaded && (
                <Field label={`Built-in Mailpit ${data.native_version}`}>
                  <FormRow>
                    <Checkbox
                      label="Also delete caught mail"
                      checked={deleteMail}
                      onChange={setDeleteMail}
                    />
                    <div className="u-flex-1" />
                    <Button
                      variant="danger"
                      icon={Trash2}
                      disabled={busy}
                      onClick={handleUninstall}
                    >
                      Uninstall
                    </Button>
                  </FormRow>
                </Field>
              )}
              {data.docker_configured && (
                <Field label="Docker container">
                  <FormRow>
                    <div className="u-flex-1" />
                    <Button
                      variant="danger"
                      icon={Trash2}
                      disabled={busy}
                      onClick={handleRemoveContainer}
                    >
                      Remove container
                    </Button>
                  </FormRow>
                </Field>
              )}
            </Stack>
          </Section>
        )}
      </Stack>
    </Page>
  );
}
