import { useCallback, useEffect, useMemo, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  ActionGroup,
  Button,
  Callout,
  Column,
  DataTable,
  EmptyState,
  Field,
  Form,
  FormRow,
  Icon,
  IconButton,
  Menu,
  Modal,
  Mono,
  Section,
  Select,
  SortState,
  Stack,
  TextInput,
  Toolbar,
} from "../../../ui";
import {
  ArrowUp,
  Download,
  File,
  Folder,
  FolderPlus,
  Pencil,
  RefreshCw,
  Trash2,
  Unplug,
  Upload,
} from "../../../ui/icons";
import {
  SftpEntry,
  SshHost,
  sftpDelete,
  sftpDownload,
  sftpHome,
  sftpList,
  sftpMkdir,
  sftpRename,
  sftpUpload,
} from "../api";
import {
  EntrySortKey,
  baseName,
  errorMessage,
  formatModified,
  formatSize,
  hostLabel,
  joinPath,
  parentPath,
  sortEntries,
  splitBreadcrumb,
} from "../utils";

export interface SftpSessionInfo {
  id: string;
  host: SshHost;
}

export default function SftpTab({
  sessions,
  activeId,
  hosts,
  connecting,
  onActivate,
  onConnect,
  onDisconnect,
}: {
  sessions: SftpSessionInfo[];
  activeId: string | null;
  hosts: SshHost[];
  connecting: boolean;
  onActivate: (id: string) => void;
  onConnect: (host: SshHost) => void;
  onDisconnect: (id: string) => void;
}) {
  const active = sessions.find((s) => s.id === activeId) ?? null;

  return (
    <Stack gap={5}>
      <Toolbar
        end={
          <>
            <Select
              aria-label="Connect to host"
              size="sm"
              value=""
              placeholder={connecting ? "Connecting…" : "Connect to host…"}
              disabled={connecting || hosts.length === 0}
              onChange={(id) => {
                const host = hosts.find((h) => h.id === id);
                if (host) onConnect(host);
              }}
              options={hosts.map((h) => ({ value: h.id, label: `${h.name} (${hostLabel(h)})` }))}
            />
            {active && (
              <Button variant="secondary" size="sm" icon={Unplug} onClick={() => onDisconnect(active.id)}>
                Disconnect
              </Button>
            )}
          </>
        }
      >
        {sessions.length > 0 && (
          <Select
            aria-label="Active SFTP session"
            size="sm"
            value={active?.id ?? ""}
            onChange={onActivate}
            options={sessions.map((s) => ({ value: s.id, label: `${s.host.name} (${hostLabel(s.host)})` }))}
          />
        )}
      </Toolbar>

      {active ? (
        <SftpBrowser key={active.id} session={active} />
      ) : (
        <Section padding="none">
          <EmptyState
            icon={Folder}
            size="md"
            title="Not connected"
            description={
              hosts.length === 0
                ? "Save a host in the Hosts tab first, then connect to browse its files."
                : "Pick a host above to open an SFTP session."
            }
          />
        </Section>
      )}
    </Stack>
  );
}

function SftpBrowser({ session }: { session: SftpSessionInfo }) {
  const confirmAction = useConfirm();
  const [path, setPath] = useState<string | null>(null);
  const [entries, setEntries] = useState<SftpEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [sort, setSort] = useState<SortState>({ key: "name", dir: "asc" });
  const [busy, setBusy] = useState<string | null>(null);
  const [nameDialog, setNameDialog] = useState<{ kind: "mkdir" } | { kind: "rename"; entry: SftpEntry } | null>(
    null,
  );

  const load = useCallback(
    async (target: string) => {
      setLoading(true);
      setError(null);
      try {
        setEntries(await sftpList(session.id, target));
      } catch (e) {
        setError(errorMessage(e));
      } finally {
        setLoading(false);
      }
    },
    [session.id],
  );

  // Start in the remote home directory; fall back to / if the server won't
  // say (some chrooted SFTP setups).
  useEffect(() => {
    let cancelled = false;
    sftpHome(session.id)
      .catch(() => "/")
      .then((home) => {
        if (!cancelled) setPath(home || "/");
      });
    return () => {
      cancelled = true;
    };
  }, [session.id]);

  useEffect(() => {
    if (path !== null) void load(path);
  }, [path, load]);

  const sorted = useMemo(
    () => sortEntries(entries, sort.key as EntrySortKey, sort.dir),
    [entries, sort],
  );

  function openDir(entry: SftpEntry) {
    if (entry.is_dir) setPath(entry.path);
  }

  async function withBusy(key: string, action: () => Promise<void>) {
    setBusy(key);
    setError(null);
    try {
      await action();
      if (path !== null) await load(path);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  async function handleDownload(entry: SftpEntry) {
    const target = await save({ defaultPath: entry.name, title: `Download ${entry.name}` });
    if (!target) return;
    await withBusy(entry.path, async () => {
      await sftpDownload(session.id, entry.path, target);
    });
  }

  async function handleUpload() {
    if (path === null) return;
    const picked = await open({ multiple: true, title: "Upload files" });
    if (!picked) return;
    const files = Array.isArray(picked) ? picked : [picked];
    await withBusy("upload", async () => {
      for (const local of files) {
        await sftpUpload(session.id, local, joinPath(path, baseName(local)));
      }
    });
  }

  async function handleDelete(entry: SftpEntry) {
    const ok = await confirmAction(
      entry.is_dir
        ? `The directory "${entry.name}" will be removed. It must be empty.`
        : `"${entry.name}" will be permanently deleted on ${session.host.name}.`,
      { title: entry.is_dir ? "Remove directory?" : "Delete file?", confirmLabel: "Delete", danger: true },
    );
    if (!ok) return;
    await withBusy(entry.path, async () => {
      await sftpDelete(session.id, entry.path);
    });
  }

  async function handleNameSubmit(value: string) {
    if (path === null || !nameDialog) return;
    const name = value.trim();
    const dialog = nameDialog;
    setNameDialog(null);
    if (!name || name.includes("/")) {
      setError("Enter a name without slashes.");
      return;
    }
    await withBusy("name", async () => {
      if (dialog.kind === "mkdir") {
        await sftpMkdir(session.id, joinPath(path, name));
      } else if (name !== dialog.entry.name) {
        await sftpRename(session.id, dialog.entry.path, joinPath(path, name));
      }
    });
  }

  // Rows arrive pre-sorted (directories first, then by the chosen column);
  // the constant sortValue keeps the header clickable without letting the
  // table re-sort and scatter folders through the files.
  const columns: Column<SftpEntry>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      sortValue: () => 0,
      render: (e) => (
        <Stack direction="row" gap={2} align="center">
          <Icon icon={e.is_dir ? Folder : File} size="sm" />
          {e.is_dir ? (
            <Button variant="link" onClick={() => openDir(e)}>
              {e.name}
            </Button>
          ) : (
            <span>{e.name}</span>
          )}
        </Stack>
      ),
    },
    {
      key: "size",
      header: "Size",
      priority: 2,
      align: "end",
      sortValue: () => 0,
      render: (e) => (e.is_dir ? "—" : formatSize(e.size)),
    },
    {
      key: "modified",
      header: "Modified",
      priority: 2,
      sortValue: () => 0,
      render: (e) => formatModified(e.modified),
    },
    {
      key: "perm",
      header: "Mode",
      priority: 3,
      mono: true,
      render: (e) => <Mono>{e.permissions}</Mono>,
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (e) => (
        <ActionGroup>
          {!e.is_dir && (
            <IconButton
              icon={Download}
              label="Download"
              loading={busy === e.path}
              onClick={() => handleDownload(e)}
            />
          )}
          <Menu
            items={[
              {
                key: "rename",
                label: "Rename",
                icon: Pencil,
                onClick: () => setNameDialog({ kind: "rename", entry: e }),
              },
              {
                key: "delete",
                label: "Delete",
                icon: Trash2,
                danger: true,
                separatorBefore: true,
                onClick: () => handleDelete(e),
              },
            ]}
          />
        </ActionGroup>
      ),
    },
  ];

  const crumbs = splitBreadcrumb(path ?? "/");

  return (
    <Section
      padding="none"
      toolbar={
        <Toolbar
          end={
            <>
              <IconButton
                icon={ArrowUp}
                label="Up one level"
                disabled={path === null || path === "/"}
                onClick={() => path !== null && setPath(parentPath(path))}
              />
              <IconButton
                icon={RefreshCw}
                label="Refresh"
                loading={loading}
                onClick={() => path !== null && load(path)}
              />
              <Button
                variant="secondary"
                size="sm"
                icon={FolderPlus}
                disabled={path === null}
                onClick={() => setNameDialog({ kind: "mkdir" })}
              >
                New folder
              </Button>
              <Button
                variant="secondary"
                size="sm"
                icon={Upload}
                loading={busy === "upload"}
                disabled={path === null}
                onClick={handleUpload}
              >
                Upload
              </Button>
            </>
          }
        >
          <Stack direction="row" gap={1} align="center" wrap>
            {crumbs.map((crumb, i) => (
              <Stack key={crumb.path} direction="row" gap={1} align="center">
                {i > 0 && <span className="u-muted">/</span>}
                <Button variant="link" size="sm" onClick={() => setPath(crumb.path)}>
                  {crumb.label}
                </Button>
              </Stack>
            ))}
          </Stack>
        </Toolbar>
      }
    >
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}
      <DataTable
        columns={columns}
        rows={sorted}
        rowKey={(e) => e.path}
        sort={sort}
        onSortChange={setSort}
        loading={loading && entries.length === 0}
        onRowDoubleClick={openDir}
        density="compact"
        caption={`Files in ${path ?? "/"} on ${session.host.name}`}
        empty={{ icon: Folder, title: "Empty directory" }}
      />

      {nameDialog && (
        <NameDialog
          title={nameDialog.kind === "mkdir" ? "New folder" : `Rename ${nameDialog.entry.name}`}
          initial={nameDialog.kind === "mkdir" ? "" : nameDialog.entry.name}
          confirmLabel={nameDialog.kind === "mkdir" ? "Create" : "Rename"}
          onSubmit={handleNameSubmit}
          onClose={() => setNameDialog(null)}
        />
      )}
    </Section>
  );
}

function NameDialog({
  title,
  initial,
  confirmLabel,
  onSubmit,
  onClose,
}: {
  title: string;
  initial: string;
  confirmLabel: string;
  onSubmit: (value: string) => void;
  onClose: () => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <Modal size="sm" title={title} onClose={onClose}>
      <Form onSubmit={() => onSubmit(value)}>
        <Stack gap={5}>
          <Field label="Name" htmlFor="sftp-name">
            <TextInput id="sftp-name" mono value={value} onChange={(e) => setValue(e.target.value)} autoFocus />
          </Field>
          <FormRow className="form-actions">
            <Button variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={!value.trim()}>
              {confirmLabel}
            </Button>
          </FormRow>
        </Stack>
      </Form>
    </Modal>
  );
}
