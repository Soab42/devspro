import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { open } from "@tauri-apps/plugin-dialog";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import BackupResultModal from "../components/BackupResultModal";
import BrowsePanel from "../components/BrowsePanel";
import ChangePasswordModal from "../components/ChangePasswordModal";
import CreateDatabaseModal from "../components/CreateDatabaseModal";
import CredentialsModal from "../components/CredentialsModal";
import ManageUsersModal from "../components/ManageUsersModal";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Column,
  DataTable,
  IconButton,
  Menu,
  Page,
  Section,
  Stack,
  StatusPill,
  statusOf,
  Tabs,
  useResource,
} from "../../../ui";
import {
  Database,
  Download,
  ExternalLink,
  KeyRound,
  Plus,
  RefreshCw,
  Table2,
  Trash2,
  Upload,
  Users,
} from "../../../ui/icons";
import {
  backupDatabase,
  Credentials,
  DevsuiteContainer,
  getDatabaseCredentials,
  listDatabases,
  removeDatabase,
  restoreDatabase,
} from "../api";
import { parsePortMappings } from "../../docker/utils";
import { isBrowsable } from "../utils";

type PageTab = "manage" | "browse";

interface Group {
  base: string;
  db?: DevsuiteContainer;
  ui?: DevsuiteContainer;
}

type Kind = "mysql" | "postgres" | "mongo" | "redis";

function inferKind(image: string): Kind | null {
  if (image.startsWith("mysql")) return "mysql";
  if (image.startsWith("postgres")) return "postgres";
  if (image.startsWith("mongo")) return "mongo";
  if (image.startsWith("redis")) return "redis";
  return null;
}

function inferDefaultUsername(image: string): string | null {
  const kind = inferKind(image);
  if (kind === "postgres") return "postgres";
  if (kind === "mongo" || kind === "mysql") return "root";
  return null;
}

function groupContainers(containers: DevsuiteContainer[]): Group[] {
  const groups = new Map<string, Group>();
  for (const c of containers) {
    const isUi = c.name.endsWith("_ui");
    const base = c.name.replace(/^devsuite_/, "").replace(/_ui$/, "");
    const group = groups.get(base) ?? { base };
    if (isUi) group.ui = c;
    else group.db = c;
    groups.set(base, group);
  }
  return Array.from(groups.values());
}

export default function DatabasesPage() {
  const confirmAction = useConfirm();
  const module = useModule("databases");
  const databases = useResource(listDatabases);

  const [tab, setTab] = useState<PageTab>("manage");
  const [showCreate, setShowCreate] = useState(false);
  const [busyBase, setBusyBase] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [credentialsFor, setCredentialsFor] = useState<{
    base: string;
    creds: Credentials;
    fallbackUsername: string | null;
  } | null>(null);
  const [usersFor, setUsersFor] = useState<string | null>(null);
  const [changingAdminPasswordFor, setChangingAdminPasswordFor] = useState<
    string | null
  >(null);
  const [backupResult, setBackupResult] = useState<{
    base: string;
    path: string;
  } | null>(null);

  async function handleReveal(g: Group) {
    setBusyBase(g.base);
    try {
      const creds = await getDatabaseCredentials(g.base);
      const fallbackUsername = g.db ? inferDefaultUsername(g.db.image) : null;
      setCredentialsFor({ base: g.base, creds, fallbackUsername });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyBase(null);
    }
  }

  // Adminer pre-fills server/username/db from its URL query string (its own
  // documented "permanent login" feature) but — by Adminer's own security
  // design — never accepts a password that way, so that part always has to be
  // typed once when the login page loads. Mongo/Redis's UIs connect via env
  // vars directly and don't show a login form at all, so this only applies to
  // MySQL/Postgres-backed entries.
  async function handleOpenUi(g: Group, hostPort: number) {
    const kind = g.db ? inferKind(g.db.image) : null;
    if (!g.db || (kind !== "mysql" && kind !== "postgres")) {
      void openUrl(`http://localhost:${hostPort}`);
      return;
    }
    try {
      const creds = await getDatabaseCredentials(g.base);
      const username = creds.username ?? inferDefaultUsername(g.db.image) ?? "";
      const driverParam = kind === "postgres" ? "pgsql" : "server";
      const params = new URLSearchParams({
        [driverParam]: g.base,
        username,
        db: g.base,
      });
      void openUrl(`http://localhost:${hostPort}/?${params.toString()}`);
    } catch {
      void openUrl(`http://localhost:${hostPort}`);
    }
  }

  async function handleRemove(base: string, deleteData: boolean) {
    const ok = await confirmAction(
      deleteData
        ? `"${base}" and its data volume will be permanently deleted. This cannot be undone.`
        : `The container(s) for "${base}" are removed, but data on disk is kept.`,
      {
        title: deleteData
          ? "Delete database and its data?"
          : "Remove database?",
        confirmLabel: deleteData ? "Delete everything" : "Remove",
        danger: true,
      },
    );
    if (!ok) return;

    setBusyBase(base);
    setError(null);
    try {
      await removeDatabase(base, deleteData);
      await databases.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyBase(null);
    }
  }

  async function handleBackup(base: string) {
    setBusyBase(base);
    setError(null);
    setNotice(null);
    try {
      setBackupResult({ base, path: await backupDatabase(base) });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyBase(null);
    }
  }

  async function handleRestore(base: string) {
    const selected = await open({
      multiple: false,
      title: `Select a backup file to restore into "${base}"`,
    });
    if (!selected || Array.isArray(selected)) return;

    const ok = await confirmAction(
      `Restoring from ${selected} overwrites the current data in "${base}". This cannot be undone.`,
      { title: "Restore from backup?", confirmLabel: "Restore", danger: true },
    );
    if (!ok) return;

    setBusyBase(base);
    setError(null);
    setNotice(null);
    try {
      setNotice(await restoreDatabase(base, selected));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyBase(null);
    }
  }

  const groups = groupContainers(databases.data ?? []);
  const browsable = groups
    .filter((g) => g.db && isBrowsable(g.db.image))
    .map((g) => ({ base: g.base, running: g.db?.state === "running" }));

  const columns: Column<Group>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      sortValue: (g) => g.base,
      render: (g) => {
        const kind = g.db ? inferKind(g.db.image) : null;
        return (
          <span className="dt-cell-inline">
            <span className="dt-strong">{g.base}</span>
            {/* The engine was previously invisible in the table — you had to
                open a dialog to find out what kind of database a row was. */}
            {kind && (
              <Badge tone="neutral" icon={Database}>
                {kind}
              </Badge>
            )}
          </span>
        );
      },
    },
    {
      key: "db",
      header: "Database",
      priority: 1,
      render: (g) =>
        g.db ? (
          <span className="dt-cell-inline">
            <StatusPill
              variant={g.db.state === "running" ? "ok" : "bad"}
              label={g.db.state}
            />
            {parsePortMappings(g.db.ports).map((m) => (
              <Badge key={m.hostPort} mono>
                :{m.hostPort}
              </Badge>
            ))}
          </span>
        ) : (
          <span className="u-muted">—</span>
        ),
    },
    {
      key: "ui",
      header: "Admin UI",
      priority: 2,
      render: (g) =>
        g.ui ? (
          <span className="dt-cell-inline">
            <StatusPill
              variant={g.ui.state === "running" ? "ok" : "bad"}
              label={g.ui.state}
            />
            {parsePortMappings(g.ui.ports).map((m) => (
              <Button
                key={m.hostPort}
                size="sm"
                variant="secondary"
                icon={ExternalLink}
                onClick={() => handleOpenUi(g, m.hostPort)}
              >
                :{m.hostPort}
              </Button>
            ))}
          </span>
        ) : (
          <span className="u-muted">none</span>
        ),
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (g) => {
        const busy = busyBase === g.base;
        return (
          <ActionGroup>
            <IconButton
              icon={KeyRound}
              label="Reveal password"
              disabled={busy}
              onClick={() => handleReveal(g)}
            />
            <IconButton
              icon={Users}
              label="Manage users"
              disabled={busy}
              onClick={() => setUsersFor(g.base)}
            />
            <Menu
              items={[
                {
                  key: "backup",
                  label: "Back up",
                  icon: Download,
                  disabled: busy,
                  onClick: () => handleBackup(g.base),
                },
                {
                  key: "restore",
                  label: "Restore from file…",
                  icon: Upload,
                  disabled: busy,
                  onClick: () => handleRestore(g.base),
                },
                {
                  key: "remove",
                  label: "Remove (keep data)",
                  icon: Trash2,
                  separatorBefore: true,
                  disabled: busy,
                  onClick: () => handleRemove(g.base, false),
                },
                {
                  key: "delete",
                  label: "Delete + data",
                  icon: Trash2,
                  danger: true,
                  disabled: busy,
                  onClick: () => handleRemove(g.base, true),
                },
              ]}
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
      status={statusOf(databases)}
      description={
        <>
          Managed in DevsPro's own docker-compose project ("devsuite") —
          separate from any other compose stacks you run yourself.
        </>
      }
      actions={
        <>
          <Button
            variant="secondary"
            icon={RefreshCw}
            loading={databases.refreshing}
            onClick={databases.refresh}
          >
            Refresh
          </Button>
          <Button
            variant="primary"
            icon={Plus}
            onClick={() => setShowCreate(true)}
          >
            Create database
          </Button>
        </>
      }
    >
      <Stack gap={5}>
        <Tabs
          ariaLabel="Databases view"
          active={tab}
          onChange={(id) => setTab(id as PageTab)}
          tabs={[
            { id: "manage", label: "Manage", icon: Database },
            { id: "browse", label: "Browse", icon: Table2 },
          ]}
        />

        {tab === "browse" && !databases.loading && (
          <BrowsePanel connections={browsable} />
        )}

        {/* Load-bearing: explains why the admin UI still asks for a password
            after "Open" pre-fills everything else. */}
        {tab === "manage" && (
          <Callout tone="info">
            For MySQL/Postgres, <strong>Open</strong> pre-fills the server,
            username, and database on Adminer's login page — but never the
            password, since Adminer deliberately never accepts one via URL or
            env var for security. Use <strong>Reveal password</strong> to copy
            it in, once per session. Mongo/Redis's UIs connect automatically
            with no login form.
          </Callout>
        )}

        {notice && (
          <Callout tone="success" onDismiss={() => setNotice(null)}>
            {notice}
          </Callout>
        )}
        {error && (
          <Callout tone="danger" onDismiss={() => setError(null)}>
            {error}
          </Callout>
        )}

        {tab === "manage" && (
          <Section
            title="DevsPro space"
            padding="none"
            isEmpty={!databases.loading && groups.length === 0}
            empty={{
              icon: Database,
              title: "No databases yet",
              description:
                "Create one to get a database container plus an admin UI, managed together.",
              action: (
                <Button
                  variant="primary"
                  icon={Plus}
                  onClick={() => setShowCreate(true)}
                >
                  Create database
                </Button>
              ),
            }}
          >
            <DataTable
              columns={columns}
              rows={groups}
              rowKey={(g) => g.base}
              loading={databases.loading}
              defaultSort={{ key: "name", dir: "asc" }}
              caption="Databases managed by DevsPro"
            />
          </Section>
        )}
      </Stack>

      {showCreate && (
        <CreateDatabaseModal
          onClose={() => setShowCreate(false)}
          onCreated={databases.refresh}
        />
      )}

      {usersFor && (
        <ManageUsersModal base={usersFor} onClose={() => setUsersFor(null)} />
      )}

      {credentialsFor && (
        <CredentialsModal
          base={credentialsFor.base}
          credentials={credentialsFor.creds}
          fallbackUsername={credentialsFor.fallbackUsername}
          onClose={() => setCredentialsFor(null)}
          onChangePassword={() =>
            setChangingAdminPasswordFor(credentialsFor.base)
          }
        />
      )}

      {backupResult && (
        <BackupResultModal
          base={backupResult.base}
          path={backupResult.path}
          onClose={() => setBackupResult(null)}
        />
      )}

      {changingAdminPasswordFor && (
        <ChangePasswordModal
          base={changingAdminPasswordFor}
          onClose={() => setChangingAdminPasswordFor(null)}
          onChanged={async () => {
            const creds = await getDatabaseCredentials(
              changingAdminPasswordFor,
            );
            setCredentialsFor((prev) => (prev ? { ...prev, creds } : prev));
          }}
        />
      )}
    </Page>
  );
}
