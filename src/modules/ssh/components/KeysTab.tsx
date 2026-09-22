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
import { KeyRound, Plus, Trash2 } from "../../../ui/icons";
import type { Resource } from "../../../ui";
import { SshKey, deleteKey } from "../api";
import { errorMessage } from "../utils";
import GenerateKeyForm from "./GenerateKeyForm";

export default function KeysTab({ keys }: { keys: Resource<SshKey[]> }) {
  const confirmAction = useConfirm();
  const [showGenerate, setShowGenerate] = useState(false);
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
          <Button variant="primary" size="sm" icon={Plus} onClick={() => setShowGenerate(true)}>
            Generate key
          </Button>
        }
        isEmpty={!keys.loading && rows.length === 0}
        empty={{
          icon: KeyRound,
          title: "No SSH keys",
          description: "Generate a key pair here, then install its public half on a host to log in without a password.",
          action: (
            <Button variant="primary" icon={Plus} onClick={() => setShowGenerate(true)}>
              Generate key
            </Button>
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
