import { useState } from "react";
import { Badge, BadgeTone, Button, Callout, Field, Mono, Modal, ModalFooter, Spinner, Stack, TextInput } from "../../../ui";
import { Eraser } from "../../../ui/icons";
import type { CleanOutcome, Finding, Risk } from "../api";
import { formatBytes, RISK_LABEL } from "../cleaner";

const RISK_TONE: Record<Risk, BadgeTone> = { safe: "success", review: "warning", danger: "danger" };

export interface ReviewCleanModalProps {
  items: Finding[];
  /**
   * Runs the clean. `confirmed` holds the ids whose typed text matched, which
   * is what the Docker command takes.
   */
  onConfirm: (ids: string[], confirmed: string[]) => Promise<CleanOutcome[]>;
  onClose: () => void;
}

type Phase = "review" | "running" | "done";

export default function ReviewCleanModal({ items, onConfirm, onClose }: ReviewCleanModalProps) {
  const [phase, setPhase] = useState<Phase>("review");
  const [typed, setTyped] = useState<Record<string, string>>({});
  const [outcomes, setOutcomes] = useState<CleanOutcome[]>([]);
  const [error, setError] = useState<string | null>(null);

  const total = items.reduce((sum, f) => sum + f.bytes, 0);
  const needsText = items.filter((f) => f.needs_confirm_text);
  const allConfirmed = needsText.every((f) => typed[f.id] === f.needs_confirm_text);
  const freed = outcomes.reduce((sum, o) => sum + (o.ok ? o.bytes_freed : 0), 0);
  const labelOf = (id: string) => items.find((f) => f.id === id)?.label ?? id;

  async function run() {
    setPhase("running");
    setError(null);
    try {
      const confirmed = needsText.filter((f) => typed[f.id] === f.needs_confirm_text).map((f) => f.id);
      setOutcomes(await onConfirm(items.map((f) => f.id), confirmed));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setPhase("done");
  }

  const footer =
    phase === "review" ? (
      <ModalFooter>
        <Button onClick={onClose}>Cancel</Button>
        <Button variant="danger" icon={Eraser} disabled={items.length === 0 || !allConfirmed} onClick={run}>
          Clean {formatBytes(total)}
        </Button>
      </ModalFooter>
    ) : (
      <ModalFooter hint={phase === "running" ? "Cleaning…" : undefined}>
        <Button variant="primary" disabled={phase === "running"} onClick={onClose}>
          Close
        </Button>
      </ModalFooter>
    );

  return (
    <Modal
      title="Review before cleaning"
      description="Nothing is removed until you confirm."
      icon={Eraser}
      size="lg"
      onClose={phase === "running" ? () => {} : onClose}
      footer={footer}
    >
      {phase === "review" && (
        <Stack gap={4}>
          {items.map((f) => (
            <Stack key={f.id} gap={2}>
              <Stack gap={1}>
                <span>
                  <strong>{f.label}</strong> <Badge tone={RISK_TONE[f.risk]}>{RISK_LABEL[f.risk]}</Badge>{" "}
                  <Mono>{formatBytes(f.bytes)}</Mono> <Badge mono>{f.method}</Badge>
                </span>
                {f.paths.map((p) => (
                  <Mono key={p}>{p}</Mono>
                ))}
              </Stack>
              {f.needs_confirm_text && (
                <Field label={`Type ${f.needs_confirm_text} to confirm`}>
                  <TextInput
                    value={typed[f.id] ?? ""}
                    onChange={(e) => setTyped({ ...typed, [f.id]: e.target.value })}
                    placeholder={f.needs_confirm_text}
                    autoComplete="off"
                    spellCheck={false}
                  />
                </Field>
              )}
            </Stack>
          ))}
          <p>
            Total: <Mono>{formatBytes(total)}</Mono>
          </p>
        </Stack>
      )}

      {phase === "running" && (
        <Stack gap={2}>
          <Spinner label="Cleaning" />
          <span>Cleaning {items.length} items…</span>
        </Stack>
      )}

      {phase === "done" && (
        <Stack gap={3}>
          {error && <Callout tone="danger">{error}</Callout>}
          {!error && (
            <Callout tone="success">
              Freed <Mono>{formatBytes(freed)}</Mono>
            </Callout>
          )}
          {outcomes.map((o) => (
            <Callout key={o.id} tone={o.ok ? "success" : "danger"}>
              {labelOf(o.id)}: {o.message}
              {o.ok ? ` (${formatBytes(o.bytes_freed)})` : ""}
            </Callout>
          ))}
        </Stack>
      )}
    </Modal>
  );
}
