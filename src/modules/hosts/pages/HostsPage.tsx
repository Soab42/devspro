import { useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  CodeBlock,
  Column,
  DataTable,
  IconButton,
  Modal,
  Mono,
  Page,
  Section,
  Stack,
  statusOf,
  useResource,
} from "../../../ui";
import { BookUser, Pencil, Plus, RefreshCw, Trash2 } from "../../../ui/icons";
import HostsEntryForm from "../components/HostsEntryForm";
import { HostsEntry, HostsView, missingSiteHosts, readHosts, removeHostsEntry } from "../api";

type FormTarget = { kind: "new"; hostname?: string } | { kind: "edit"; entry: HostsEntry };

export default function HostsPage() {
  const confirmAction = useConfirm();
  const module = useModule("hosts");

  // No polling: the file only changes when something writes it, and every write
  // through this page already hands back the fresh contents.
  const hosts = useResource(readHosts);
  const missing = useResource(missingSiteHosts);

  const [error, setError] = useState<string | null>(null);
  const [busyLine, setBusyLine] = useState<number | null>(null);
  const [form, setForm] = useState<FormTarget | null>(null);

  function applyView(view: HostsView) {
    hosts.setData(view);
    void missing.refresh();
  }

  async function refreshAll() {
    await Promise.all([hosts.refresh(), missing.refresh()]);
  }

  async function handleDelete(entry: HostsEntry) {
    const ok = await confirmAction(
      <>
        <Mono variant="chip">{entry.hostnames.join(" ")}</Mono> will no longer resolve to{" "}
        <Mono variant="chip">{entry.ip}</Mono>. You will be asked for your password.
      </>,
      { title: "Remove hosts entry?", confirmLabel: "Remove", danger: true },
    );
    if (!ok) return;

    setBusyLine(entry.line_no);
    setError(null);
    try {
      applyView(await removeHostsEntry(entry.line_no));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyLine(null);
    }
  }

  const entries = hosts.data?.entries ?? [];
  const missingDomains = missing.data ?? [];

  const columns: Column<HostsEntry>[] = [
    {
      key: "ip",
      header: "IP",
      priority: 1,
      mono: true,
      width: "12rem",
      sortValue: (e) => e.ip,
      render: (e) => e.ip,
    },
    {
      key: "hostnames",
      header: "Hostnames",
      priority: 1,
      mono: true,
      sortValue: (e) => e.hostnames[0] ?? "",
      render: (e) => e.hostnames.join(" "),
    },
    {
      key: "source",
      header: "Source",
      priority: 2,
      sortValue: (e) => (e.managed ? 1 : 0),
      render: (e) =>
        e.managed ? <Badge tone="accent">DevsPro</Badge> : <Badge tone="neutral">system</Badge>,
    },
    {
      key: "comment",
      header: "Comment",
      priority: 3,
      render: (e) =>
        e.comment && e.comment !== "devspro" ? (
          <span className="u-muted">{e.comment}</span>
        ) : (
          ""
        ),
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (e) => {
        const isLocalhost = e.hostnames.some((h) => h.toLowerCase().includes("localhost"));
        return (
          <ActionGroup>
            <IconButton
              icon={Pencil}
              label="Edit entry"
              disabled={busyLine !== null}
              onClick={() => setForm({ kind: "edit", entry: e })}
            />
            <IconButton
              icon={Trash2}
              label={isLocalhost ? "localhost entries cannot be removed" : "Remove entry"}
              variant="danger"
              disabled={busyLine !== null || isLocalhost}
              loading={busyLine === e.line_no}
              onClick={() => handleDelete(e)}
            />
          </ActionGroup>
        );
      },
    },
  ];

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(hosts)}
      description={
        <>
          Entries in <Mono variant="chip">/etc/hosts</Mono>. Changes are written through the
          system password prompt; everything else in the file is left exactly as it was.
        </>
      }
      actions={
        <>
          <Button
            variant="secondary"
            icon={RefreshCw}
            loading={hosts.refreshing}
            onClick={refreshAll}
          >
            Refresh
          </Button>
          <Button variant="primary" icon={Plus} onClick={() => setForm({ kind: "new" })}>
            Add entry
          </Button>
        </>
      }
    >
      <Stack gap={5}>
        {error && (
          <Callout tone="danger" onDismiss={() => setError(null)}>
            {error}
          </Callout>
        )}

        {missingDomains.length > 0 && (
          <Callout tone="warning" title="Sites without a hosts entry">
            <Stack gap={2}>
              <span>
                These nginx sites are enabled but do not resolve to this machine, so the
                browser cannot reach them.
              </span>
              {missingDomains.map((domain) => (
                <Stack key={domain} direction="row" gap={3} align="center">
                  <Mono variant="chip">{domain}</Mono>
                  <Button
                    size="sm"
                    variant="secondary"
                    icon={Plus}
                    onClick={() => setForm({ kind: "new", hostname: domain })}
                  >
                    Add entry
                  </Button>
                </Stack>
              ))}
            </Stack>
          </Callout>
        )}

        <Section
          title="Entries"
          padding="none"
          meta={<Badge tone="neutral">{entries.length}</Badge>}
          isEmpty={!hosts.loading && entries.length === 0}
          empty={{
            icon: BookUser,
            title: "No entries",
            description: "The hosts file has no IP-to-name mappings.",
          }}
        >
          <DataTable
            columns={columns}
            rows={entries}
            rowKey={(e) => String(e.line_no)}
            loading={hosts.loading}
            caption="Entries in /etc/hosts, in file order"
          />
        </Section>

        <Section title="Raw file" collapsible defaultCollapsed>
          <CodeBlock maxHeight="40vh">{hosts.data?.raw ?? ""}</CodeBlock>
        </Section>
      </Stack>

      {form && (
        <Modal
          size="sm"
          title={form.kind === "new" ? "Add hosts entry" : `Edit line ${form.entry.line_no}`}
          icon={BookUser}
          onClose={() => setForm(null)}
        >
          <HostsEntryForm
            entry={form.kind === "edit" ? form.entry : undefined}
            initialHostname={form.kind === "new" ? form.hostname : undefined}
            onSaved={(view) => {
              setForm(null);
              applyView(view);
            }}
            onCancel={() => setForm(null)}
          />
        </Modal>
      )}
    </Page>
  );
}
