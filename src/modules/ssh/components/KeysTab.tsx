import { useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Column,
  CopyButton,
  DataTable,
  IconButton,
  Modal,
  Mono,
  Section,
  Stack,
} from "../../../ui";
import { ClipboardPaste, KeyRound, Plus, Trash2 } from "../../../ui/icons";
import type { Resource } from "../../../ui";
import { SshKey, deleteKey } from "../api";
import { errorMessage } from "../utils";
import GenerateKeyForm from "./GenerateKeyForm";
import ImportKeyForm from "./ImportKeyForm";

export default function KeysTab({ keys }: { keys: Resource<SshKey[]> }) {
  const confirmAction = useConfirm();
  const [showGenerate, setShowGenerate] = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [busyName, setBusyName] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ tone: "success" | "danger"; text: string } | null>(null);

  const rows = keys.data ?? [];

  async function handleDelete(key: SshKey) {
    const ok = await confirmAction(
      <>
        Both <Mono variant="chip">{key.name}</Mono> and <Mono variant="chip">{key.name}.pub</Mono> will be
        deleted from ~/.ssh. Any server that only trusts this key becomes unreachable from here.
      </>,
      { title: "Delete key?", confirmLabel: "Delete", danger: true },
    );
    if (!ok) return;
    setBusyName(key.name);
    try {
      await deleteKey(key.name);
      await keys.refresh();
    } catch (e) {
      setNotice({ tone: "danger", text: errorMessage(e) });
    } finally {
      setBusyName(null);
    }
  }

  const columns: Column<SshKey>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      sortValue: (k) => k.name,
      render: (k) => (
        <Stack direction="row" gap={2} align="center">
          <span className="dt-strong">{k.name}</span>
          {!k.has_private && <Badge tone="warning">public only</Badge>}
        </Stack>
      ),
    },
    {
      key: "type",
      header: "Type",
      priority: 2,
      sortValue: (k) => k.type,
      render: (k) => <Badge tone="neutral" mono>{k.bits ? `${k.type} ${k.bits}` : k.type}</Badge>,
    },
    {
      key: "fingerprint",
      header: "Fingerprint",
      priority: 2,
      mono: true,
      sortValue: (k) => k.fingerprint,
      render: (k) => (k.fingerprint ? <Mono truncate copyable>{k.fingerprint}</Mono> : "—"),
    },
    {
      key: "comment",
      header: "Comment",
      priority: 3,
      sortValue: (k) => k.comment,
      render: (k) => k.comment || "—",
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (k) => (
        <ActionGroup>
          <CopyButton value={k.public_key} label="Copy public key" />
          <IconButton
            icon={Trash2}
            label="Delete"
            variant="danger"
            loading={busyName === k.name}
            onClick={() => handleDelete(k)}
          />
        </ActionGroup>
      ),
    },
  ];

  return (
    <Stack gap={5}>
      {notice && (
        <Callout tone={notice.tone} onDismiss={() => setNotice(null)}>
          {notice.text}
        </Callout>
      )}
      {keys.error && !keys.loading && <Callout tone="danger">{keys.error}</Callout>}

      <Section
        title="Keys in ~/.ssh"
        padding="none"
        actions={
          <>
            <Button variant="secondary" size="sm" icon={ClipboardPaste} onClick={() => setShowImport(true)}>
              Import key
            </Button>
            <Button variant="primary" size="sm" icon={Plus} onClick={() => setShowGenerate(true)}>
              Generate key
            </Button>
          </>
        }
        isEmpty={!keys.loading && rows.length === 0}
        empty={{
          icon: KeyRound,
          title: "No SSH keys",
          description:
            "Generate a key pair here, or import one you already have, then pick it in a host to log in without a password.",
          action: (
            <Stack direction="row" gap={2}>
              <Button variant="secondary" icon={ClipboardPaste} onClick={() => setShowImport(true)}>
                Import key
              </Button>
              <Button variant="primary" icon={Plus} onClick={() => setShowGenerate(true)}>
                Generate key
              </Button>
            </Stack>
          ),
        }}
      >
        <DataTable
          columns={columns}
          rows={rows}
          rowKey={(k) => k.name}
          loading={keys.loading}
          defaultSort={{ key: "name", dir: "asc" }}
          caption="SSH keys"
        />
      </Section>

      {showImport && (
        <Modal size="md" title="Import key" icon={ClipboardPaste} onClose={() => setShowImport(false)}>
          <ImportKeyForm
            existing={rows}
            onImported={(key) => {
              setShowImport(false);
              setNotice({
                tone: "success",
                text: `Imported ${key.name} (${key.fingerprint || key.type}). Pick it as a host's key to use it.`,
              });
              void keys.refresh();
            }}
            onCancel={() => setShowImport(false)}
          />
        </Modal>
      )}

      {showGenerate && (
        <Modal size="md" title="Generate key" icon={KeyRound} onClose={() => setShowGenerate(false)}>
          <GenerateKeyForm
            existing={rows}
            onGenerated={(key) => {
              setShowGenerate(false);
              setNotice({ tone: "success", text: `Generated ${key.name} (${key.fingerprint}).` });
              void keys.refresh();
            }}
            onCancel={() => setShowGenerate(false)}
          />
        </Modal>
      )}
    </Stack>
  );
}
