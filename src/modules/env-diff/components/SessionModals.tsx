import { useId, useState } from "react";
import {
  ActionGroup,
  Button,
  Callout,
  Column,
  DataTable,
  Field,
  Form,
  IconButton,
  Modal,
  ModalFooter,
  Mono,
  Stack,
  TextInput,
} from "../../../ui";
import { Bookmark, Save, Trash2 } from "../../../ui/icons";
import type { EnvSession } from "../sessions";

/** What one side would be saved as. */
export interface SideSummary {
  path: string | null;
  pasted: boolean;
  /** A file with edits not yet saved to it — the session keeps the path, not the edits. */
  dirty?: boolean;
}

function SideLine({ side, summary }: { side: string; summary: SideSummary }) {
  const { path, pasted, dirty } = summary;
  return (
    <div>
      <span className="u-secondary">{side}: </span>
      {path ? (
        <>
          <Mono truncate>{path}</Mono>
          {dirty && <span className="u-muted"> (unsaved edits aren&apos;t kept)</span>}
        </>
      ) : (
        <span className="u-muted">{pasted ? "pasted text" : "empty"}</span>
      )}
    </div>
  );
}

const sideOf = (s: EnvSession, side: "left" | "right"): SideSummary => ({
  path: s[side],
  pasted: s.pasted.includes(side),
});

export function SaveSessionModal({
  left,
  right,
  initialName,
  existing,
  onSave,
  onClose,
}: {
  left: SideSummary;
  right: SideSummary;
  initialName: string;
  existing: EnvSession[];
  onSave: (name: string) => void;
  onClose: () => void;
}) {
  const nameId = useId();
  const [name, setName] = useState(initialName);
  const trimmed = name.trim();
  const nothingToSave = !left.path && !left.pasted && !right.path && !right.pasted;
  const replaces = existing.some((s) => s.name === trimmed);
  const canSave = !nothingToSave && trimmed !== "";
  const submit = () => {
    if (canSave) onSave(trimmed);
  };

  return (
    <Modal
      title="Save session"
      icon={Save}
      size="sm"
      onClose={onClose}
      footer={
        <ModalFooter>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={!canSave} onClick={submit}>
            {replaces ? "Replace" : "Save"}
          </Button>
        </ModalFooter>
      }
    >
      {nothingToSave ? (
        <Callout tone="info" title="Nothing to save yet">
          Load a file or paste an env file on either side first.
        </Callout>
      ) : (
        <Form onSubmit={submit}>
          <Stack gap={4}>
            <Field
              label="Name"
              htmlFor={nameId}
              hint={replaces ? `Replaces the saved session "${trimmed}".` : undefined}
            >
              <TextInput id={nameId} autoFocus value={name} onChange={(e) => setName(e.target.value)} />
            </Field>
            <Stack gap={1}>
              <SideLine side="Left" summary={left} />
              <SideLine side="Right" summary={right} />
            </Stack>
            <p className="u-muted">
              Files are saved by path and read from disk again when the session is opened.
              {(left.pasted || right.pasted) &&
                " Pasted text is saved as it is, on this machine only, and is left out of backups."}
            </p>
          </Stack>
        </Form>
      )}
    </Modal>
  );
}

export function SessionsModal({
  sessions,
  onOpen,
  onDelete,
  onClose,
}: {
  sessions: EnvSession[];
  onOpen: (session: EnvSession) => void;
  onDelete: (session: EnvSession) => void;
  onClose: () => void;
}) {
  const columns: Column<EnvSession>[] = [
    {
      key: "name",
      header: "Session",
      priority: 1,
      sortValue: (s) => s.name,
      render: (s) => (
        <Stack gap={1}>
          <strong>{s.name}</strong>
          <SideLine side="Left" summary={sideOf(s, "left")} />
          <SideLine side="Right" summary={sideOf(s, "right")} />
        </Stack>
      ),
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (s) => (
        <ActionGroup>
          <Button variant="secondary" size="sm" onClick={() => onOpen(s)}>
            Open
          </Button>
          <IconButton icon={Trash2} label={`Delete ${s.name}`} onClick={() => onDelete(s)} />
        </ActionGroup>
      ),
    },
  ];

  return (
    <Modal
      title="Sessions"
      description="Saved pairs of env files. Opening one reads its files from disk again."
      icon={Bookmark}
      onClose={onClose}
    >
      <DataTable
        columns={columns}
        rows={sessions}
        rowKey={(s) => s.name}
        defaultSort={{ key: "name", dir: "asc" }}
        caption="Saved env diff sessions"
        empty={{
          icon: Bookmark,
          title: "No saved sessions",
          description: "Load or paste an env file on either side, then use Save session.",
        }}
      />
    </Modal>
  );
}
