import { useMemo, useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Checkbox,
  Column,
  DataTable,
  Field,
  Form,
  FormRow,
  IconButton,
  Modal,
  ModalFooter,
  SegmentedControl,
  Select,
  SelectOption,
  Stack,
  TextInput,
  useResource,
} from "../../../ui";
import {
  Container,
  Database,
  Network,
  Pencil,
  PlugZap,
  Plus,
  Server,
  Trash2,
} from "../../../ui/icons";
import { listContainers } from "../../docker/api";
import {
  ConnectionInput,
  ConnectionKind,
  ConnectionTransport,
  deleteConnection,
  inspectContainer,
  listConnections,
  SavedConnection,
  saveConnection,
  testConnection,
} from "../api";
import {
  connectionTarget,
  DEFAULT_PORT,
  DEFAULT_USER,
  isEngineImage,
} from "../utils";

/**
 * The three starting points the form offers. "Remote" and "Local" are the same
 * `tcp` transport as far as the backend is concerned — the only difference is
 * the host they prefill — but they are the two cases people actually think in,
 * and collapsing them into one "TCP" option made the form read like a network
 * setting rather than "where is this database".
 */
type Source = "remote" | "local" | "docker";

const SOURCE_OPTIONS = [
  { value: "remote" as const, label: "Remote", icon: Network },
  { value: "local" as const, label: "Local", icon: Server },
  { value: "docker" as const, label: "Docker", icon: Container },
];

const KIND_OPTIONS: SelectOption[] = [
  { value: "mysql", label: "MySQL / MariaDB" },
  { value: "postgres", label: "PostgreSQL" },
];

const transportOf = (source: Source): ConnectionTransport =>
  source === "docker" ? "docker" : "tcp";

function sourceOf(c: SavedConnection): Source {
  if (c.transport === "docker") return "docker";
  return c.host === "127.0.0.1" || c.host === "localhost" ? "local" : "remote";
}

const SOURCE_LABEL: Record<Source, string> = {
  remote: "Remote",
  local: "Local",
  docker: "Docker",
};

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/* ---------------------------------------------------------------- Form -- */

interface ConnectionFormProps {
  /** The row being edited, or null when adding. */
  editing: SavedConnection | null;
  onDone: () => void;
  onCancel: () => void;
}

function ConnectionForm({ editing, onDone, onCancel }: ConnectionFormProps) {
  const [name, setName] = useState(editing?.name ?? "");
  const [kind, setKind] = useState<ConnectionKind>(editing?.kind ?? "mysql");
  const [source, setSource] = useState<Source>(
    editing ? sourceOf(editing) : "remote",
  );
  const [host, setHost] = useState(editing?.host ?? "");
  const [port, setPort] = useState(editing ? String(editing.port) : "");
  const [container, setContainer] = useState(editing?.container ?? "");
  const [username, setUsername] = useState(editing?.username ?? "");
  const [password, setPassword] = useState("");
  // An edit starts with a blank password field that means "keep the stored
  // one". Only once it is typed in does a value get sent — otherwise reopening
  // the form and saving would wipe the password.
  const [passwordTouched, setPasswordTouched] = useState(false);
  const [defaultDatabase, setDefaultDatabase] = useState(
    editing?.default_database ?? "",
  );

  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // Only loaded for the Docker source, and only once the modal is open — the
  // picker is the whole reason a container name doesn't have to be typed.
  const containers = useResource(listContainers, {
    enabled: source === "docker",
  });
  // Showing every container on the machine buried the two or three that are
  // actually databases. The filter is a heuristic on the image name, so it can
  // be turned off for a database baked into a custom image.
  const [allContainers, setAllContainers] = useState(false);
  const matching = useMemo(
    () =>
      (containers.data ?? []).filter(
        (c) =>
          allContainers ||
          isEngineImage(c.image, kind) ||
          // Whatever is already chosen stays listed whatever the filter says,
          // so switching engines can't make the picker look empty while a
          // container is in fact selected.
          c.name === container,
      ),
    [containers.data, allContainers, kind, container],
  );
  const containerOptions = useMemo<SelectOption[]>(
    () =>
      matching.map((c) => ({
        value: c.name,
        label: `${c.name} — ${c.image}${c.state === "running" ? "" : ` (${c.state})`}`,
      })),
    [matching],
  );
  const hiddenCount = (containers.data?.length ?? 0) - matching.length;

  /**
   * Picking a container fills in the rest of the form from its own
   * environment, which is where the official images took those credentials
   * from in the first place. Only fields the container actually reports are
   * overwritten, so a custom image that says nothing leaves what's typed alone.
   */
  async function handleContainer(containerName: string) {
    setContainer(containerName);
    // The container's own name beats anything this form could invent, but it
    // never overwrites a name already typed.
    setName((current) => current || containerName);
    setError(null);
    try {
      const found = await inspectContainer(containerName);
      if (found.kind) setKind(found.kind);
      if (found.username) setUsername(found.username);
      if (found.database) setDefaultDatabase(found.database);
      if (found.password) {
        setPassword(found.password);
        setPasswordTouched(true);
      }
    } catch (e) {
      setError(errorText(e));
    }
  }

  function handleSource(next: Source) {
    setSource(next);
    if (next === "local") setHost("127.0.0.1");
    else if (next === "remote" && host === "127.0.0.1") setHost("");
  }

  function currentInput(): ConnectionInput {
    return {
      id: editing?.id ?? null,
      name,
      kind,
      transport: transportOf(source),
      host,
      port: Number(port) || DEFAULT_PORT[kind],
      container,
      username: username || DEFAULT_USER[kind],
      password: editing && !passwordTouched ? null : password,
      defaultDatabase,
    };
  }

  async function handleTest() {
    setTesting(true);
    setError(null);
    setNotice(null);
    try {
      setNotice(await testConnection(currentInput()));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setTesting(false);
    }
  }

  async function handleSubmit() {
    setBusy(true);
    setError(null);
    try {
      await saveConnection(currentInput());
      onDone();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      size="md"
      title={editing ? `Edit "${editing.name}"` : "Add a connection"}
      icon={PlugZap}
      onClose={onCancel}
      footer={
        <ModalFooter>
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button
            variant="secondary"
            icon={PlugZap}
            loading={testing}
            onClick={() => void handleTest()}
          >
            Test
          </Button>
          <Button variant="primary" loading={busy} onClick={handleSubmit}>
            {editing ? "Save" : "Add"}
          </Button>
        </ModalFooter>
      }
    >
      <Form onSubmit={handleSubmit}>
        <Stack gap={5}>
          <Field
            label="Where is it?"
            hint={
              source === "docker"
                ? "The client runs inside the container, so the database's port doesn't have to be published."
                : "Remote and local servers are both reached over TCP — pick whichever describes yours."
            }
          >
            <SegmentedControl
              ariaLabel="Connection source"
              size="md"
              value={source}
              options={SOURCE_OPTIONS}
              onChange={handleSource}
            />
          </Field>

          <FormRow align="end">
            <Field label="Name" htmlFor="conn-name" required>
              <TextInput
                id="conn-name"
                placeholder="e.g. Staging MySQL"
                value={name}
                onChange={(e) => setName(e.target.value)}
                autoFocus
              />
            </Field>
            <Field label="Engine" htmlFor="conn-kind">
              <Select
                id="conn-kind"
                value={kind}
                options={KIND_OPTIONS}
                onChange={(v) => setKind(v as ConnectionKind)}
              />
            </Field>
          </FormRow>

          {source === "docker" ? (
            <Field
              label="Container"
              htmlFor="conn-container"
              required
              hint={
                containers.error ??
                "Narrowed to images that look like this engine. Picking one fills in the username, password and database from its environment."
              }
            >
              <Stack gap={2}>
                <Select
                  id="conn-container"
                  value={container}
                  options={containerOptions}
                  searchable
                  searchPlaceholder="Search containers…"
                  placeholder={
                    containers.loading
                      ? "Loading containers…"
                      : containerOptions.length === 0
                        ? "No database containers found"
                        : "Pick a container"
                  }
                  onChange={(v) => void handleContainer(v)}
                />
                <Checkbox
                  checked={allContainers}
                  onChange={setAllContainers}
                  label={
                    hiddenCount > 0
                      ? `Show all containers (${hiddenCount} hidden)`
                      : "Show all containers"
                  }
                />
              </Stack>
            </Field>
          ) : (
            <FormRow align="end">
              <Field label="Host" htmlFor="conn-host" required>
                <TextInput
                  id="conn-host"
                  mono
                  placeholder={source === "local" ? "127.0.0.1" : "db.example.com"}
                  value={host}
                  onChange={(e) => setHost(e.target.value)}
                />
              </Field>
              <Field label="Port" htmlFor="conn-port">
                <TextInput
                  id="conn-port"
                  mono
                  inputMode="numeric"
                  className="field-input-narrow"
                  placeholder={String(DEFAULT_PORT[kind])}
                  value={port}
                  onChange={(e) => setPort(e.target.value)}
                />
              </Field>
            </FormRow>
          )}

          <FormRow align="end">
            <Field label="Username" htmlFor="conn-user">
              <TextInput
                id="conn-user"
                mono
                placeholder={DEFAULT_USER[kind]}
                value={username}
                onChange={(e) => setUsername(e.target.value)}
              />
            </Field>
            <Field
              label="Password"
              htmlFor="conn-password"
              hint={
                editing && !passwordTouched
                  ? "Leave blank to keep the saved one."
                  : undefined
              }
            >
              <TextInput
                id="conn-password"
                mono
                type="password"
                value={password}
                onChange={(e) => {
                  setPassword(e.target.value);
                  setPasswordTouched(true);
                }}
              />
            </Field>
          </FormRow>

          <Field
            label="Default database"
            htmlFor="conn-database"
            hint="Optional — which database the browser opens first."
          >
            <TextInput
              id="conn-database"
              mono
              value={defaultDatabase}
              onChange={(e) => setDefaultDatabase(e.target.value)}
            />
          </Field>

          <Callout tone="info">
            The password is saved in DevsPro's local database in plain text —
            the same protection the compose file gives the databases DevsPro
            runs itself. Don't save credentials here you wouldn't keep in a
            local <strong>.env</strong> file.
          </Callout>

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
        </Stack>
      </Form>
    </Modal>
  );
}

/* ---------------------------------------------------------------- List -- */

export interface ConnectionsModalProps {
  onClose: () => void;
  /** Called after any change, so the browser can pick up the new list. */
  onChanged: () => void;
}

export default function ConnectionsModal({
  onClose,
  onChanged,
}: ConnectionsModalProps) {
  const confirmAction = useConfirm();
  const connections = useResource(listConnections);
  const [form, setForm] = useState<
    { editing: SavedConnection | null } | null
  >(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleDelete(c: SavedConnection) {
    const ok = await confirmAction(
      `"${c.name}" is removed from DevsPro. The database itself is not touched.`,
      { title: "Forget this connection?", confirmLabel: "Forget", danger: true },
    );
    if (!ok) return;
    setBusyId(c.id);
    setError(null);
    try {
      await deleteConnection(c.id);
      await connections.refresh();
      onChanged();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusyId(null);
    }
  }

  const columns: Column<SavedConnection>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      sortValue: (c) => c.name,
      render: (c) => (
        <span className="dt-cell-inline">
          <span className="dt-strong">{c.name}</span>
          <Badge tone="neutral" icon={Database}>
            {c.kind}
          </Badge>
        </span>
      ),
    },
    {
      key: "target",
      header: "Server",
      priority: 1,
      render: (c) => (
        <span className="dt-cell-inline">
          <Badge tone="neutral">{SOURCE_LABEL[sourceOf(c)]}</Badge>
          <span className="u-mono">{connectionTarget(c)}</span>
        </span>
      ),
    },
    {
      key: "user",
      header: "User",
      priority: 2,
      render: (c) => (
        <span className="dt-cell-inline">
          <span className="u-mono">{c.username}</span>
          {!c.has_password && <Badge tone="warning">no password</Badge>}
        </span>
      ),
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (c) => (
        <ActionGroup>
          <IconButton
            icon={Pencil}
            label="Edit connection"
            disabled={busyId === c.id}
            onClick={() => setForm({ editing: c })}
          />
          <IconButton
            icon={Trash2}
            label="Forget connection"
            disabled={busyId === c.id}
            onClick={() => void handleDelete(c)}
          />
        </ActionGroup>
      ),
    },
  ];

  if (form) {
    return (
      <ConnectionForm
        editing={form.editing}
        onCancel={() => setForm(null)}
        onDone={async () => {
          setForm(null);
          await connections.refresh();
          onChanged();
        }}
      />
    );
  }

  return (
    <Modal
      size="lg"
      title="Connections"
      icon={PlugZap}
      description="Databases DevsPro doesn't run itself — a remote server, one installed on this machine, or another container's."
      onClose={onClose}
      footer={
        <ModalFooter>
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
          <Button
            variant="primary"
            icon={Plus}
            onClick={() => setForm({ editing: null })}
          >
            Add connection
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={4}>
        {(error || connections.error) && (
          <Callout
            tone="danger"
            onDismiss={error ? () => setError(null) : undefined}
          >
            {error ?? connections.error}
          </Callout>
        )}
        <DataTable
          columns={columns}
          rows={connections.data ?? []}
          rowKey={(c) => c.id}
          loading={connections.loading}
          defaultSort={{ key: "name", dir: "asc" }}
          empty={{
            icon: PlugZap,
            title: "No saved connections",
            description:
              "Add one to browse a database that lives outside DevsPro's own compose project.",
            size: "sm",
          }}
          caption="Saved database connections"
        />
      </Stack>
    </Modal>
  );
}
