import { useId, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  Badge,
  Button,
  Callout,
  CopyButton,
  Field,
  FormRow,
  Mono,
  Section,
  Stack,
  TextArea,
  TextInput,
} from "../../../ui";
import { FileUp } from "../../../ui/icons";
import { readEnvFile, writeEnvFile } from "../api";
import { EnvIssue, EnvSide, emptySide, isDirty } from "../envFile";

export interface Notice {
  tone: "success" | "danger";
  text: string;
}

const MAX_ISSUES = 4;

const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * One of the two files. Its text is the source of truth for the comparison —
 * loading a file just fills the box — so a pasted snippet, a loaded file and a
 * loaded file with edits are all the same thing to the diff.
 */
export default function EnvSource({
  label,
  side,
  issues,
  onChange,
  onNotice,
}: {
  label: string;
  side: EnvSide;
  issues: EnvIssue[];
  onChange: (next: EnvSide) => void;
  onNotice: (notice: Notice) => void;
}) {
  const confirmAction = useConfirm();
  const pathId = useId();
  const textId = useId();
  // The path being typed is kept apart from `side.path`, the file actually
  // loaded. Save must only ever write to the latter: were they one value,
  // editing the box after a load would quietly re-aim Save at a different file.
  const [pathDraft, setPathDraft] = useState(side.path ?? "");
  const [busy, setBusy] = useState<"load" | "save" | null>(null);

  const dirty = isDirty(side);
  const lower = label.toLowerCase();

  async function load(path: string) {
    const target = path.trim();
    if (!target) return;
    setBusy("load");
    try {
      const text = await readEnvFile(target);
      onChange({ text, path: target, disk: text });
      setPathDraft(target);
    } catch (e) {
      onNotice({ tone: "danger", text: messageOf(e) });
    } finally {
      setBusy(null);
    }
  }

  async function browse() {
    try {
      // No extension filter: `.env` and `.env.local` have no extension to match.
      const selected = await open({ multiple: false, title: `Select the ${lower} env file` });
      if (!selected || Array.isArray(selected)) return;
      await load(selected);
    } catch (e) {
      onNotice({ tone: "danger", text: messageOf(e) });
    }
  }

  async function save() {
    if (!side.path) return;
    const ok = await confirmAction(
      <p>
        Overwrite <Mono variant="chip">{side.path}</Mono> with the contents shown here?
      </p>,
      { title: `Save the ${lower} file?`, confirmLabel: "Save" },
    );
    if (!ok) return;

    setBusy("save");
    try {
      const message = await writeEnvFile(side.path, side.text);
      onChange({ ...side, disk: side.text });
      onNotice({ tone: "success", text: message });
    } catch (e) {
      onNotice({ tone: "danger", text: messageOf(e) });
    } finally {
      setBusy(null);
    }
  }

  function clear() {
    onChange(emptySide());
    setPathDraft("");
  }

  return (
    <Section
      title={label}
      meta={
        <>
          {side.path && (
            <Mono truncate title={side.path}>
              {side.path}
            </Mono>
          )}
          {dirty && <Badge tone="warning">Unsaved changes</Badge>}
          {!side.path && side.text && <Badge tone="neutral">Pasted</Badge>}
        </>
      }
      actions={
        <>
          {dirty && (
            <Button
              variant="ghost"
              onClick={() => onChange({ ...side, text: side.disk ?? "" })}
            >
              Revert
            </Button>
          )}
          {dirty && (
            <Button variant="primary" loading={busy === "save"} onClick={save}>
              Save to file
            </Button>
          )}
          {side.text && <CopyButton value={side.text} label={`Copy ${lower} contents`} />}
          {(side.text || side.path) && (
            <Button variant="ghost" onClick={clear}>
              Clear
            </Button>
          )}
        </>
      }
    >
      <Stack gap={4}>
        <Field
          label="File"
          htmlFor={pathId}
          description="Browse, or type a path — some file pickers hide dotfiles like .env. Or skip this and paste below."
        >
          <FormRow>
            {/* A bare input is width:100%, so as a direct child of the row it
                claims the whole line and pushes the buttons underneath. */}
            <div className="u-flex-1">
              <TextInput
                id={pathId}
                mono
                placeholder="/home/you/project/.env"
                value={pathDraft}
                onChange={(e) => setPathDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void load(pathDraft);
                }}
              />
            </div>
            <Button variant="secondary" icon={FileUp} disabled={busy !== null} onClick={browse}>
              Browse…
            </Button>
            <Button
              variant="secondary"
              loading={busy === "load"}
              disabled={!pathDraft.trim() || busy === "save"}
              onClick={() => load(pathDraft)}
            >
              Load
            </Button>
          </FormRow>
        </Field>

        <Field label="Contents" htmlFor={textId}>
          <TextArea
            id={textId}
            mono
            rows={10}
            wrap="off"
            spellCheck={false}
            autoCapitalize="off"
            autoComplete="off"
            placeholder={"# paste a .env file here\nAPI_URL=https://example.test\nDEBUG=false"}
            value={side.text}
            onChange={(e) => onChange({ ...side, text: e.target.value })}
          />
        </Field>

        {issues.length > 0 && (
          <Callout tone="warning" title="Worth a look">
            {issues.slice(0, MAX_ISSUES).map((issue) => (
              <div key={`${issue.line}:${issue.message}`}>
                Line {issue.line}: {issue.message}
              </div>
            ))}
            {issues.length > MAX_ISSUES && <div>…and {issues.length - MAX_ISSUES} more.</div>}
          </Callout>
        )}
      </Stack>
    </Section>
  );
}
