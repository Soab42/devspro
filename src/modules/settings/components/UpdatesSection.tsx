import { Fragment, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useSearchParams } from "react-router-dom";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Button, Callout, Meter, Mono, Section, Stack, Switch } from "../../../ui";
import { ExternalLink, RefreshCw } from "../../../ui/icons";
import {
  AUTO_CHECK_KEY,
  checkForUpdates,
  downloadUpdate,
  formatProgress,
  installUpdate,
  onDownloadProgress,
  parseNotes,
  updaterStatus,
} from "../../../app/updater";
import type { DownloadProgress, UpdateCheck, UpdaterStatus } from "../../../app/updater";
import { setSetting } from "../api";

type Phase = "idle" | "downloading" | "installing";

/** `code` and **bold** — all the release notes use. Everything else is text. */
function inline(text: string): ReactNode[] {
  return text.split(/(`[^`]+`|\*\*[^*]+\*\*)/g).map((part, i) => {
    if (part.startsWith("`") && part.endsWith("`") && part.length > 1) return <Mono key={i}>{part.slice(1, -1)}</Mono>;
    if (part.startsWith("**") && part.endsWith("**") && part.length > 3) return <strong key={i}>{part.slice(2, -2)}</strong>;
    return <Fragment key={i}>{part}</Fragment>;
  });
}

function ReleaseNotes({ notes }: { notes: string }) {
  const groups = parseNotes(notes);
  if (groups.length === 0) return null;
  return (
    <Stack gap={3}>
      {groups.map((g, i) => (
        <Stack key={i} gap={1}>
          {g.title && (
            <p>
              <strong>{g.title}</strong>
            </p>
          )}
          {g.items.map((item, j) => (
            <p key={j} className="u-secondary">
              {inline(item)}
            </p>
          ))}
        </Stack>
      ))}
    </Stack>
  );
}

function checkedAgo(iso: string): string {
  const minutes = Math.round((Date.now() - new Date(iso).getTime()) / 60_000);
  if (!Number.isFinite(minutes) || minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  return hours < 24 ? `${hours} h ago` : new Date(iso).toLocaleDateString();
}

export default function UpdatesSection() {
  const [status, setStatus] = useState<UpdaterStatus | null>(null);
  const [check, setCheck] = useState<UpdateCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [phase, setPhase] = useState<Phase>("idle");
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [params, setParams] = useSearchParams();
  const anchor = useRef<HTMLDivElement>(null);

  useEffect(() => {
    updaterStatus()
      .then((s) => {
        setStatus(s);
        setCheck(s.last_check);
        if (s.downloading) setPhase("downloading");
      })
      .catch((e) => setError(String(e)));
    // No event bridge (tests, a torn-down window) just means no live updates.
    const unlisten = onDownloadProgress(setProgress).catch(() => undefined);
    return () => {
      void unlisten.then((fn) => fn?.());
    };
  }, []);

  // The topbar's "Update available" pill links here.
  useEffect(() => {
    if (params.get("section") !== "updates") return;
    anchor.current?.scrollIntoView({ block: "start" });
    setParams({}, { replace: true });
  }, [params, setParams]);

  async function runCheck() {
    setChecking(true);
    setError(null);
    try {
      setCheck(await checkForUpdates());
    } catch (e) {
      setError(String(e));
    } finally {
      setChecking(false);
    }
  }

  async function install() {
    setError(null);
    try {
      if (status?.ready !== check?.latest_version) {
        setPhase("downloading");
        setProgress(null);
        const ready = await downloadUpdate();
        setStatus((s) => (s ? { ...s, ready } : s));
      }
      setPhase("installing");
      // Doesn't come back on success: the app restarts.
      await installUpdate();
    } catch (e) {
      setError(String(e));
      setPhase("idle");
    }
  }

  async function toggleAutoCheck(checked: boolean) {
    setStatus((s) => (s ? { ...s, auto_check: checked } : s));
    try {
      await setSetting(AUTO_CHECK_KEY, checked ? "true" : "false");
    } catch (e) {
      setStatus((s) => (s ? { ...s, auto_check: !checked } : s));
      setError(String(e));
    }
  }

  const websiteButton = status && (
    <Button variant="secondary" icon={ExternalLink} onClick={() => void openUrl(status.release_notes_url)}>
      Release notes
    </Button>
  );
  const elevationPrompt = status?.install_kind === "nsis" ? "the administrator prompt" : "your password";
  const percent = progress?.total ? (progress.downloaded / progress.total) * 100 : 0;

  return (
    <div ref={anchor}>
      <Section
        title="Updates"
        icon={RefreshCw}
        description="DevsPro checks its website for a new version. Updates are signed, and the elevation helper checks the signature again before it installs one."
      >
        <Stack gap={4}>
          {!status ? null : check?.available ? (
            <Callout tone={check.installable && !status.unsupported ? "info" : "warning"}>
              <Stack gap={3}>
                <span>
                  <strong>DevsPro {check.latest_version}</strong> is available. You have {check.current_version}.
                  {(!check.installable || status.unsupported) &&
                    ` ${status.unsupported ?? "There is no package for this kind of install yet."} Download it from the website instead.`}
                </span>
                <ReleaseNotes notes={check.notes} />
              </Stack>
            </Callout>
          ) : check ? (
            <p className="u-muted">
              You're on the latest version, {check.current_version}. Checked {checkedAgo(check.checked_at)}.
            </p>
          ) : (
            <p className="u-muted">
              Version {status.current_version}. {status.auto_check ? "DevsPro checks a little after it starts." : "Not checked yet."}
            </p>
          )}

          {phase === "downloading" && (
            <Meter
              label="Downloading"
              percent={percent}
              valueLabel={progress ? formatProgress(progress) : "Starting…"}
              warningAt={101}
              criticalAt={101}
            />
          )}
          {phase === "installing" && (
            <p className="u-muted">Waiting for {elevationPrompt}… DevsPro restarts once the update is in.</p>
          )}
          {error && (
            <Callout tone="danger" onDismiss={() => setError(null)}>
              {error}
            </Callout>
          )}

          <Stack direction="row" wrap gap={3}>
            {check?.available && check.installable && !status?.unsupported && (
              <Button loading={phase !== "idle"} onClick={() => void install()}>
                {status?.ready === check.latest_version ? "Install and restart" : "Download and install"}
              </Button>
            )}
            <Button
              variant="secondary"
              icon={RefreshCw}
              loading={checking}
              disabled={phase !== "idle"}
              onClick={() => void runCheck()}
            >
              Check for updates
            </Button>
            {websiteButton}
          </Stack>

          {status && (
            <Switch
              checked={status.auto_check}
              onChange={(checked) => void toggleAutoCheck(checked)}
              label="Check for updates automatically"
              description="On start, then every six hours while DevsPro runs. Nothing is installed without you."
            />
          )}
        </Stack>
      </Section>
    </div>
  );
}
