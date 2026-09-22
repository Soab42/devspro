import { useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Column,
  DataTable,
  IconButton,
  Menu,
  Modal,
  Mono,
  Section,
  Stack,
} from "../../../ui";
import { Folder, KeyRound, Pencil, Plus, Server, Terminal, Trash2 } from "../../../ui/icons";
import type { Resource } from "../../../ui";
import { SshHost, SshKey, deleteHost, installKeyOnHost } from "../api";
import { errorMessage, hostLabel } from "../utils";
import HostForm from "./HostForm";
import InstallKeyModal from "./InstallKeyModal";
import { useConnectFlow } from "./useConnectFlow";

const AUTH_LABEL: Record<SshHost["auth"], string> = {
  agent: "agent",
  key: "key file",
  password: "password",
};

export default function HostsTab({
  hosts,
  keys,
  onOpenTerminal,
  onOpenSftp,
}: {
  hosts: Resource<SshHost[]>;
  keys: SshKey[];
  onOpenTerminal: (host: SshHost) => void;
  onOpenSftp: (host: SshHost) => void;
}) {
  const confirmAction = useConfirm();
  const connectFlow = useConnectFlow();
  const [formFor, setFormFor] = useState<SshHost | "new" | null>(null);
  const [installFor, setInstallFor] = useState<SshHost | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ tone: "success" | "danger"; text: string } | null>(null);

  const rows = hosts.data ?? [];

  async function handleDelete(host: SshHost) {
    const ok = await confirmAction(`"${host.name}" (${hostLabel(host)}) will be removed from the list.`, {
      title: "Delete host?",
      confirmLabel: "Delete",
      danger: true,
    });
    if (!ok) return;
    setBusyId(host.id);
    try {
      await deleteHost(host.id);
      await hosts.refresh();
    } catch (e) {
      setNotice({ tone: "danger", text: errorMessage(e) });
    } finally {
      setBusyId(null);
    }
  }

  async function handleInstall(host: SshHost, keyName: string) {
    setInstallFor(null);
    setBusyId(host.id);
    setNotice(null);
    try {
      const result = await connectFlow.run(host, (creds) => installKeyOnHost(host.id, keyName, creds));
      if (result !== null) setNotice({ tone: "success", text: result });
    } catch (e) {
      setNotice({ tone: "danger", text: errorMessage(e) });
    } finally {
      setBusyId(null);
    }
  }

  const columns: Column<SshHost>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      sortValue: (h) => h.name,
      render: (h) => <span className="dt-strong">{h.name}</span>,
    },
    {
      key: "target",
      header: "Address",
      priority: 1,
      mono: true,
      sortValue: (h) => `${h.host}:${h.port}`,
      render: (h) => <Mono truncate>{hostLabel(h)}</Mono>,
    },
    {
      key: "auth",
      header: "Auth",
      priority: 2,
      sortValue: (h) => h.auth,
      render: (h) => <Badge tone="neutral">{AUTH_LABEL[h.auth]}</Badge>,
    },
    {
      key: "key",
      header: "Key",
      priority: 3,
      mono: true,
      sortValue: (h) => h.key_path ?? "",
      render: (h) => (h.key_path ? <Mono truncate title={h.key_path}>{h.key_path}</Mono> : "—"),
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (h) => {
        const busy = busyId === h.id;
        return (
          <ActionGroup>
            <IconButton icon={Terminal} label="Open terminal" disabled={busy} onClick={() => onOpenTerminal(h)} />
            <IconButton icon={Folder} label="Open SFTP" loading={busy} onClick={() => onOpenSftp(h)} />
            <Menu
              items={[
                { key: "install", label: "Install key…", icon: KeyRound, onClick: () => setInstallFor(h) },
                { key: "edit", label: "Edit", icon: Pencil, onClick: () => setFormFor(h) },
                {
                  key: "delete",
                  label: "Delete",
                  icon: Trash2,
                  danger: true,
                  separatorBefore: true,
                  disabled: busy,
                  onClick: () => handleDelete(h),
                },
              ]}
            />
          </ActionGroup>
        );
      },
    },
  ];

  return (
    <Stack gap={5}>
      {notice && (
        <Callout tone={notice.tone} onDismiss={() => setNotice(null)}>
          {notice.text}
        </Callout>
      )}
      {hosts.error && !hosts.loading && <Callout tone="danger">{hosts.error}</Callout>}

      <Section
        title="Saved hosts"
        padding="none"
        actions={
          <Button variant="primary" size="sm" icon={Plus} onClick={() => setFormFor("new")}>
            New host
          </Button>
        }
        isEmpty={!hosts.loading && rows.length === 0}
        empty={{
          icon: Server,
          title: "No hosts yet",
          description: "Save a server once, then open a terminal or browse its files with one click.",
          action: (
            <Button variant="primary" icon={Plus} onClick={() => setFormFor("new")}>
              New host
            </Button>
          ),
        }}
      >
        <DataTable
          columns={columns}
          rows={rows}
          rowKey={(h) => h.id}
          loading={hosts.loading}
          defaultSort={{ key: "name", dir: "asc" }}
          caption="Saved SSH hosts"
        />
      </Section>

      {formFor && (
        <Modal
          size="md"
          title={formFor === "new" ? "New host" : `Edit — ${formFor.name}`}
          icon={Server}
          onClose={() => setFormFor(null)}
        >
          <HostForm
            host={formFor === "new" ? undefined : formFor}
            keys={keys}
            onSaved={() => {
              setFormFor(null);
              void hosts.refresh();
            }}
            onCancel={() => setFormFor(null)}
          />
        </Modal>
      )}

      {installFor && (
        <InstallKeyModal
          host={installFor}
          keys={keys}
          onPick={(keyName) => handleInstall(installFor, keyName)}
          onClose={() => setInstallFor(null)}
        />
      )}

      {connectFlow.element}
    </Stack>
  );
}
