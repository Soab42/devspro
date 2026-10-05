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

function PathLine({ side, path }: { side: string; path: string | null }) {
  return (
    <div>
      <span className="u-secondary">{side}: </span>
      {path ? <Mono truncate>{path}</Mono> : <span className="u-muted">pasted text, not saved</span>}
    </div>
  );
}

export function SaveSessionModal({
  left,
  right,
  initialName,
  existing,
  onSave,
  onClose,
}: {
  left: string | null;
  right: string | null;
  initialName: string;
  existing: EnvSession[];
  onSave: (name: string) => void;
  onClose: () => void;
}) {
  const nameId = useId();
  const [name, setName] = useState(initialName);
  const trimmed = name.trim();
  const nothingToSave = !left && !right;
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
          A session remembers files, not pasted text. Load a file on either side first.
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
              <PathLine side="Left" path={left} />
              <PathLine side="Right" path={right} />
            </Stack>
            <p className="u-muted">
              Saves the file paths and comparison options. The files are read from disk again
              when the session is opened, so none of their contents are stored.
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
          <PathLine side="Left" path={s.left} />
          <PathLine side="Right" path={s.right} />
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
      description="Saved pairs of env files. Opening one reads both files from disk again."
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
          description: "Load a file on either side, then use Save session.",
        }}
      />
    </Modal>
  );
}
