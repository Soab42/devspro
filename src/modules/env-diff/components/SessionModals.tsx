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
import { EnvSession, SideName, sideTitle } from "../sessions";

/** What one side would be saved as. */
export interface SideSummary {
  /** What the side is called — "Left", or a name the user gave it. */
  label: string;
  path: string | null;
  pasted: boolean;
  /** A file with edits not yet saved to it — the session keeps the path, not the edits. */
  dirty?: boolean;
}

function SideLine({ summary }: { summary: SideSummary }) {
  const { label, path, pasted, dirty } = summary;
  return (
    <div>
      <span className="u-secondary">{label}: </span>
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

const sideOf = (s: EnvSession, side: SideName): SideSummary => ({
  label: sideTitle(s.labels, side),
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
              <SideLine summary={left} />
              <SideLine summary={right} />
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
          {/* Text, not a button: a cell holding a control stops truncating,
              and a long path would then widen the dialog. The row opens it. */}
          <strong>{s.name}</strong>
          <SideLine summary={sideOf(s, "left")} />
          <SideLine summary={sideOf(s, "right")} />
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
          <IconButton
            icon={Trash2}
            label={`Delete ${s.name}`}
            onClick={(e) => {
              // The row itself opens the session; deleting must not also do that.
              e.stopPropagation();
              onDelete(s);
            }}
          />
        </ActionGroup>
      ),
    },
  ];

  return (
    <Modal
      title="Sessions"
      description="Saved pairs of env files. Click one to open it; its files are read from disk again."
      icon={Bookmark}
      onClose={onClose}
    >
      <DataTable
        columns={columns}
        rows={sessions}
        rowKey={(s) => s.name}
        onRowClick={onOpen}
        rowProps={(s) => ({
          // The row is the way in, so the keyboard needs to reach it too.
          tabIndex: 0,
          title: `Open ${s.name}`,
          onKeyDown: (e) => {
            // Not when Enter is meant for the delete button inside it.
            if (e.key === "Enter" && e.target === e.currentTarget) onOpen(s);
          },
        })}
        // Two narrow columns: the default 640px floor only made the dialog scroll sideways.
        minWidth={0}
        defaultSort={{ key: "name", dir: "asc" }}
        caption="Saved env diff sessions; click one to open it"
        empty={{
          icon: Bookmark,
          title: "No saved sessions",
          description: "Load or paste an env file on either side, then use Save session.",
        }}
      />
    </Modal>
  );
}
