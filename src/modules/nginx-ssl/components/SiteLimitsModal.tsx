import { useState } from "react";
import {
  Button,
  Callout,
  Field,
  FormGrid,
  Modal,
  ModalFooter,
  Stack,
  TextArea,
  TextInput,
} from "../../../ui";
import { SlidersHorizontal } from "../../../ui/icons";
import { EMPTY_LIMITS, setSiteLimits, Site, SiteLimits } from "../api";

/**
 * Every field is a string in here, including the numeric ones, so that clearing a
 * box is representable. A number-typed state would have to encode "empty" as
 * `null | number | NaN`, and each of those reads back as a value the user did not
 * type. Conversion to the wire shape happens once, on save.
 */
type Draft = Record<keyof SiteLimits, string>;

function toDraft(limits: SiteLimits): Draft {
  return {
    max_upload_size: limits.max_upload_size ?? "",
    memory_limit: limits.memory_limit ?? "",
    max_execution_time: limits.max_execution_time?.toString() ?? "",
    max_input_vars: limits.max_input_vars?.toString() ?? "",
    extra_nginx: limits.extra_nginx ?? "",
  };
}

/** Blank means "system default", which is `null` on the wire — not 0 or "". */
function text(value: string): string | null {
  return value.trim() === "" ? null : value.trim();
}

function count(value: string): number | null {
  const trimmed = value.trim();
  if (trimmed === "") return null;
  const parsed = Number(trimmed);
  if (!Number.isInteger(parsed) || parsed <= 0) {
    throw new Error(`"${trimmed}" must be a whole number greater than zero`);
  }
  return parsed;
}

export default function SiteLimitsModal({
  site,
  onClose,
  onSaved,
}: {
  site: Site;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [draft, setDraft] = useState<Draft>(() => toDraft(site.limits));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isPhp = site.backend.type === "laravel";

  function set<K extends keyof Draft>(key: K, value: string) {
    setDraft((prev) => ({ ...prev, [key]: value }));
  }

  async function handleSave() {
    setError(null);
    let limits: SiteLimits;
    try {
      limits = {
        max_upload_size: text(draft.max_upload_size),
        memory_limit: text(draft.memory_limit),
        max_execution_time: count(draft.max_execution_time),
        max_input_vars: count(draft.max_input_vars),
        extra_nginx: text(draft.extra_nginx),
      };
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return;
    }

    setBusy(true);
    try {
      await setSiteLimits(site.id, limits);
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  async function handleReset() {
    setError(null);
    setBusy(true);
    try {
      await setSiteLimits(site.id, EMPTY_LIMITS);
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  return (
    <Modal
      size="md"
      title={`Limits — ${site.name}`}
      icon={SlidersHorizontal}
      onClose={onClose}
      footer={
        <ModalFooter
          hint={busy ? "Waiting for your admin password…" : undefined}
          start={
            <Button variant="ghost" disabled={busy} onClick={handleReset}>
              Reset to defaults
            </Button>
          }
        >
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} onClick={handleSave}>
            Save
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={4}>
        <Callout tone="info">
          Applies to this site only, through its nginx config — your system{" "}
          <code>php.ini</code> is not modified and PHP-FPM does not need
          restarting. Leave a field blank to use the system default.
        </Callout>

        <FormGrid columns={2}>
          <Field
            label="Max upload size"
            htmlFor="limit-upload"
            hint={
              isPhp
                ? "Sets client_max_body_size, upload_max_filesize and post_max_size together."
                : "Sets client_max_body_size. Your app enforces its own body limits."
            }
          >
            <TextInput
              id="limit-upload"
              mono
              placeholder="64M"
              value={draft.max_upload_size}
              onChange={(e) => set("max_upload_size", e.target.value)}
            />
          </Field>

          <Field
            label="Max execution time"
            htmlFor="limit-exec"
            hint="Seconds. nginx's read timeout is raised to match."
          >
            <TextInput
              id="limit-exec"
              mono
              placeholder="30"
              inputMode="numeric"
              value={draft.max_execution_time}
              onChange={(e) => set("max_execution_time", e.target.value)}
            />
          </Field>

          {isPhp && (
            <>
              <Field
                label="Memory limit"
                htmlFor="limit-memory"
                hint="PHP memory per request. -1 for no limit."
              >
                <TextInput
                  id="limit-memory"
                  mono
                  placeholder="512M"
                  value={draft.memory_limit}
                  onChange={(e) => set("memory_limit", e.target.value)}
                />
              </Field>

              <Field
                label="Max input vars"
                htmlFor="limit-vars"
                hint="Cap on POST fields. Raise it for very large forms."
              >
                <TextInput
                  id="limit-vars"
                  mono
                  placeholder="1000"
                  inputMode="numeric"
                  value={draft.max_input_vars}
                  onChange={(e) => set("max_input_vars", e.target.value)}
                />
              </Field>
            </>
          )}
        </FormGrid>

        <Field
          label="Extra nginx directives"
          htmlFor="limit-extra"
          description="Added verbatim inside this site's server block."
          hint="Checked with nginx -t on save — if nginx rejects them, nothing changes."
        >
          <TextArea
            id="limit-extra"
            mono
            rows={5}
            spellCheck={false}
            placeholder={"add_header X-Dev 1;\nclient_body_timeout 120s;"}
            value={draft.extra_nginx}
            onChange={(e) => set("extra_nginx", e.target.value)}
          />
        </Field>

        {error && <Callout tone="danger">{error}</Callout>}
      </Stack>
    </Modal>
  );
}
