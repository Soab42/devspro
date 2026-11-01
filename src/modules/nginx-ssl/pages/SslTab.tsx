import { useState } from "react";
import { Button, Callout, Section, Stack, StatusPill } from "../../../ui";
import { ShieldCheck } from "../../../ui/icons";
import type { Resource } from "../../../ui";
import { caEnsure, caInstall, CaStatus } from "../api";

export default function SslTab({ ca }: { ca: Resource<CaStatus> }) {
  const [busy, setBusy] = useState(false);
  // Same fix as WebServerTab: distinct success and failure states.
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const status = ca.data;

  async function handleCreate() {
    setBusy(true);
    setMessage(null);
    setError(null);
    try {
      ca.setData(await caEnsure());
      setMessage("Local CA created.");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleInstall() {
    setBusy(true);
    setMessage(null);
    setError(null);
    try {
      setMessage(await caInstall());
      await ca.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Section
      title="Local trusted CA"
      icon={ShieldCheck}
      meta={
        status && (
          <StatusPill
            variant={status.exists ? "ok" : "neutral"}
            label={status.exists ? "created" : "not created"}
          />
        )
      }
    >
      <Stack gap={4}>
        {status?.exists ? (
          <p className="u-muted">
            Expires {status.not_after ?? "unknown"}. Install it into your system and
            browser trust stores so sites show a valid padlock.
          </p>
        ) : (
          <p className="u-muted">
            No local CA yet — one is created automatically the first time you add a site,
            or you can create it now.
          </p>
        )}

        <div>
          {status?.exists ? (
            <Button
              variant="primary"
              icon={ShieldCheck}
              loading={busy}
              onClick={handleInstall}
            >
              Install into system trust store
            </Button>
          ) : (
            <Button variant="primary" loading={busy} onClick={handleCreate}>
              Create local CA
            </Button>
          )}
          {busy && status?.exists && (
            <p className="u-muted nginx-hint">Waiting for your admin password…</p>
          )}
        </div>

        {message && <Callout tone="success">{message}</Callout>}
        {error && <Callout tone="danger">{error}</Callout>}
      </Stack>
    </Section>
  );
}
