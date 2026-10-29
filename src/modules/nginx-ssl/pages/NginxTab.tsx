import { useState } from "react";
import { Button, Callout, Mono, Section, Stack, StatusPill } from "../../../ui";
import { Download, RefreshCw, Server } from "../../../ui/icons";
import type { Resource } from "../../../ui";
import { installNginx, NginxDetection } from "../api";
import { isWindows } from "../../../app/platform";

export default function NginxTab({ nginx }: { nginx: Resource<NginxDetection> }) {
  const [busy, setBusy] = useState(false);
  // Success and failure were previously the same `message` string rendered as
  // the same grey paragraph, so a failed install looked exactly like a
  // successful one. They are now separate states with distinct tones.
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const detection = nginx.data;

  async function handleInstall() {
    setBusy(true);
    setMessage(null);
    setError(null);
    try {
      setMessage(await installNginx());
      await nginx.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Section
      title="Nginx"
      icon={Server}
      meta={
        detection && (
          <StatusPill
            variant={detection.found ? "ok" : "bad"}
            label={detection.found ? "detected" : "not found"}
          />
        )
      }
      actions={
        <Button
          variant="secondary"
          icon={RefreshCw}
          loading={nginx.refreshing}
          onClick={nginx.refresh}
        >
          Recheck
        </Button>
      }
    >
      <Stack gap={4}>
        {detection?.found ? (
          <p className="u-muted">
            {detection.version} — config at{" "}
            <Mono variant="chip" copyable>
              {detection.config_path ?? "unknown"}
            </Mono>
          </p>
        ) : (
          <p className="u-muted">
            Not found on this machine.
            {detection?.package_manager
              ? ` Install it via ${detection.package_manager} below — this prompts for your admin password.`
              : isWindows
                ? " Install it manually, then recheck — unzip the Windows build from nginx.org/en/download.html to C:\\nginx, or add its folder to PATH."
                : " Install it manually, then recheck — e.g. sudo apt install nginx on Debian/Ubuntu, or brew install nginx on macOS."}
          </p>
        )}

        {!detection?.found && detection?.package_manager && (
          <div>
            <Button
              variant="primary"
              icon={Download}
              loading={busy}
              onClick={handleInstall}
            >
              {`Install nginx via ${detection.package_manager}`}
            </Button>
            {busy && (
              <p className="u-muted nginx-hint">Waiting for your admin password…</p>
            )}
          </div>
        )}

        {message && <Callout tone="success">{message}</Callout>}
        {error && <Callout tone="danger">{error}</Callout>}
      </Stack>
    </Section>
  );
}
