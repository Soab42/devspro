import { RefObject, useId, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  Badge,
  Button,
  Callout,
  CopyButton,
  Field,
  FormRow,
  IconButton,
  Mono,
  Section,
  Stack,
  TextInput,
} from "../../../ui";
import { CodeEditor, CodeEditorHandle, ScrollPos } from "../../../ui/CodeEditor";
import { FileUp, FolderOpen, ListOrdered, Redo2, Search, Undo2 } from "../../../ui/icons";
import { readEnvFile, writeEnvFile } from "../api";
import { EnvIssue, EnvSide, emptySide, isDirty } from "../envFile";

export interface Notice {
  tone: "success" | "danger";
  text: string;
}

const MAX_ISSUES = 4;

export const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

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
  editorRef,
  onScroll,
  initialPath,
}: {
  label: string;
  side: EnvSide;
  issues: EnvIssue[];
  onChange: (next: EnvSide) => void;
  onNotice: (notice: Notice) => void;
  /** Lets the page drive this side's editor, e.g. to scroll it in step. */
  editorRef?: RefObject<CodeEditorHandle | null>;
  onScroll?: (pos: ScrollPos) => void;
  /** Fills the path box on mount — a saved session's file that couldn't be read, ready to fix. */
  initialPath?: string;
}) {
  const confirmAction = useConfirm();
  const pathId = useId();
  const textId = useId();
  const ownEditor = useRef<CodeEditorHandle>(null);
  const editor = editorRef ?? ownEditor;
  const [history, setHistory] = useState({ canUndo: false, canRedo: false });
  // The path being typed is kept apart from `side.path`, the file actually
  // loaded. Save must only ever write to the latter: were they one value,
  // editing the box after a load would quietly re-aim Save at a different file.
  const [pathDraft, setPathDraft] = useState(side.path ?? initialPath ?? "");
  const [busy, setBusy] = useState<"load" | "save" | null>(null);
  // Folded once there is something to compare: the path is already in the
  // header, and the row just pushes the contents down.
  const [fileOpen, setFileOpen] = useState(!side.text);

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
      setFileOpen(false);
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
    setFileOpen(true);
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
          <IconButton
            icon={FolderOpen}
            label={fileOpen ? "Hide the file picker" : "Open a file"}
            aria-expanded={fileOpen}
            aria-controls={pathId}
            variant={fileOpen ? "secondary" : "ghost"}
            onClick={() => setFileOpen((open) => !open)}
          />
          {dirty && (
            <Button variant="ghost" onClick={() => onChange({ ...side, text: side.disk ?? "" })}>
              Revert
            </Button>
          )}
          {dirty && (
            <Button variant="primary" loading={busy === "save"} onClick={save}>
              Save to file
            </Button>
          )}
          {side.text && (
            <IconButton
              icon={Undo2}
              className="btn-live"
              disabled={!history.canUndo}
              label="Undo (Ctrl+Z)"
              onClick={() => editor.current?.undo()}
            />
          )}
          {side.text && (
            <IconButton
              icon={Redo2}
              className="btn-live"
              disabled={!history.canRedo}
              label="Redo (Ctrl+Shift+Z)"
              onClick={() => editor.current?.redo()}
            />
          )}
          {side.text && (
            <IconButton
              icon={Search}
              label={`Search ${lower} contents (Ctrl+F)`}
              onClick={() => editor.current?.openSearch()}
            />
          )}
          {side.text && (
            <IconButton
              icon={ListOrdered}
              label="Go to line (Ctrl+Alt+G)"
              onClick={() => editor.current?.openGotoLine()}
            />
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
        {fileOpen && (
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
        )}

        <Field label="Contents" htmlFor={textId}>
          <CodeEditor
            ref={editor}
            onHistoryChange={setHistory}
            onScroll={onScroll}
            id={textId}
            language="env"
            rows={10}
            ariaLabel={`${label} contents`}
            placeholder={"# paste a .env file here\nAPI_URL=https://example.test\nDEBUG=false"}
            value={side.text}
            onChange={(text) => onChange({ ...side, text })}
          />
        </Field>

        {issues.length > 0 && (
          <Callout tone="warning" title="Worth a look">
            {issues.slice(0, MAX_ISSUES).map((issue) => (
              <div key={`${issue.line}:${issue.message}`}>
                <Button variant="link" onClick={() => editor.current?.goToLine(issue.line)}>
                  Line {issue.line}
                </Button>
                : {issue.message}
              </div>
            ))}
            {issues.length > MAX_ISSUES && <div>…and {issues.length - MAX_ISSUES} more.</div>}
          </Callout>
        )}
      </Stack>
    </Section>
  );
}
