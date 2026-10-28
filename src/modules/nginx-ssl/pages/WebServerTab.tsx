import { useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import { Button, Callout, Mono, Section, Stack, StatusPill } from "../../../ui";
import { Check, Download, RefreshCw, Server, X } from "../../../ui/icons";
import type { Resource } from "../../../ui";
import { BinarySource, forgetServerPath, setWebServer, WebServer } from "../api";

/** What both detections have in common — all this tab reads. */
interface Detection {
  found: boolean;
  binary_path: string | null;
  source: BinarySource | null;
  version: string | null;
  config_path: string | null;
  package_manager: string | null;
  /** Apache only: whether its pid file exists right now. */
  running?: boolean;
}

/** How the binary path was found, as the tab words it. */
const SOURCE_TEXT: Record<BinarySource, string> = {
  running: "found from the running process",
  saved: "remembered from when it last ran",
  search: "found in a usual install location",
};

/**
 * One tab per web server, nginx and Apache alike: is it installed, where is its config,
 * install it if not — and whether DevsPro's sites are written for it. Both servers get
 * exactly the same tab so neither is the second-class one.
 */
export default function WebServerTab<T extends Detection>({
  server,
  label,
  detection,
  install,
  notFoundHint,
  active,
  onSwitched,
}: {
  server: WebServer;
  label: string;
  detection: Resource<T>;
  install: () => Promise<string>;
  /** Shown when it isn't installed and there is no package manager to install it with. */
  notFoundHint: string;
  /** The server sites are written for now; `null` while it loads. */
  active: WebServer | null;
  onSwitched: () => void;
}) {
  const confirmAction = useConfirm();
  const [busy, setBusy] = useState<"install" | "switch" | null>(null);
  // Success and failure stay separate states with distinct tones, so a failed
  // install never reads like a successful one.
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const data = detection.data;
  const isActive = active === server;

  async function run(kind: "install" | "switch", action: () => Promise<string>) {
    setBusy(kind);
    setMessage(null);
    setError(null);
    try {
      setMessage(await action());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function handleInstall() {
    await run("install", install);
    await detection.refresh();
  }

  async function handleForget() {
    setMessage(null);
    setError(null);
    try {
      await forgetServerPath(server);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    await detection.refresh();
  }

  async function handleSwitch() {
    const ok = await confirmAction(
      `Every site will be rewritten as ${label} config and ${label} will be reloaded. Only one web server can own ports 80 and 443, so stop the other one afterwards.`,
      { title: `Serve sites with ${label}?`, confirmLabel: `Use ${label}` },
    );
    if (!ok) return;
    await run("switch", () => setWebServer(server));
    onSwitched();
  }

  return (
    <Section
      title={label}
      icon={Server}
      meta={
        data && (
          <>
            <StatusPill variant={data.found ? "ok" : "bad"} label={data.found ? "detected" : "not found"} />
            {data.found && data.running !== undefined && (
              <StatusPill variant={data.running ? "ok" : "warn"} label={data.running ? "running" : "not running"} />
            )}
            {isActive && <StatusPill variant="ok" label="serves your sites" />}
          </>
        )
      }
      actions={
        <Button variant="secondary" icon={RefreshCw} loading={detection.refreshing} onClick={detection.refresh}>
          Recheck
        </Button>
      }
    >
      <Stack gap={4}>
        {data?.found && (
          <p className="u-muted">
            {data.version ?? label} — config at{" "}
            <Mono variant="chip" copyable>
              {data.config_path ?? "unknown"}
            </Mono>
          </p>
        )}

        {data?.found && data.binary_path && (
          <p className="u-muted">
            Binary{" "}
            <Mono variant="chip" copyable>
              {data.binary_path}
            </Mono>
            {data.source && ` — ${SOURCE_TEXT[data.source]}.`}{" "}
            {data.source === "saved" && (
              <Button variant="ghost" size="sm" icon={X} onClick={handleForget}>
                Forget this path
              </Button>
            )}
          </p>
        )}

        {!data?.found && (
          <p className="u-muted">
            Not found on this machine.
            {data?.package_manager
              ? ` Install it via ${data.package_manager} below — this prompts for your admin password.`
              : ` ${notFoundHint}`}
          </p>
        )}

        {!data?.found && data?.package_manager && (
          <div>
            <Button variant="primary" icon={Download} loading={busy === "install"} onClick={handleInstall}>
              {`Install ${label} via ${data.package_manager}`}
            </Button>
            {busy === "install" && <p className="u-muted nginx-hint">Waiting for your admin password…</p>}
          </div>
        )}

        {data?.found && active !== null && !isActive && (
          <div>
            <Button variant="primary" icon={Check} loading={busy === "switch"} onClick={handleSwitch}>
              {`Use ${label} for sites`}
            </Button>
            <p className="u-muted nginx-hint">
              {busy === "switch"
                ? "Waiting for your admin password…"
                : `Sites are served by ${active === "apache" ? "Apache" : "nginx"} now. Switching moves all of them to ${label}.`}
            </p>
          </div>
        )}

        {message && <Callout tone="success">{message}</Callout>}
        {error && <Callout tone="danger">{error}</Callout>}
      </Stack>
    </Section>
  );
}
