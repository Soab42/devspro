import { useEffect, useMemo, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  Button,
  Callout,
  Column,
  DataTable,
  EmptyState,
  Field,
  Section,
  Select,
  SortState,
  Stack,
  Switch,
  TextArea,
  Toolbar,
  useResource,
} from "../../../ui";
import {
  ChevronLeft,
  ChevronRight,
  Download,
  Play,
  Table2,
  Upload,
} from "../../../ui/icons";
import { formatBytes } from "../../dashboard/utils";
import {
  dumpDatabase,
  fetchTableRows,
  listDatabaseNames,
  listTables,
  restoreDatabase,
  ResultSet,
  runQuery,
  TableInfo,
} from "../api";
import {
  cellSortValue,
  dumpFileName,
  hasNextPage,
  NULL_MARKER,
  orderByFrom,
  PAGE_SIZE,
  pageLabel,
} from "../utils";

export interface BrowseConnection {
  /** Compose service name — what every backend command calls `connection`. */
  base: string;
  running: boolean;
}

export interface BrowsePanelProps {
  connections: BrowseConnection[];
}

/** A result row with a stable key: rows have no identity of their own. */
interface GridRow {
  i: number;
  cells: string[];
}

const CELL_MAX = 200;

function toGridRows(rs: ResultSet): GridRow[] {
  return rs.rows.map((cells, i) => ({ i, cells }));
}

/**
 * Builds DataTable columns from a result set. Keys are the column *names* so
 * the table's sort state maps straight onto the backend's `order_by`; an
 * ad-hoc query can repeat a name (`SELECT a.id, b.id`), so the index is
 * appended there to keep keys unique.
 */
function gridColumns(rs: ResultSet, sortable: boolean): Column<GridRow>[] {
  return rs.columns.map((col, idx) => ({
    key: sortable ? col.name : `${idx}:${col.name}`,
    header: col.type ? <span title={col.type}>{col.name}</span> : col.name,
    headerLabel: col.name,
    priority: 1,
    mono: true,
    sortValue: (r) => cellSortValue(r.cells[idx] ?? ""),
    render: (r) => {
      const v = r.cells[idx] ?? "";
      if (v === NULL_MARKER) return <span className="u-muted">NULL</span>;
      if (v.length > CELL_MAX)
        return <span title={v}>{v.slice(0, CELL_MAX)}…</span>;
      return v;
    },
  }));
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/* -------------------------------------------------------------- Tables -- */

interface TablesListProps {
  connection: string;
  database: string;
  onOpen: (table: string) => void;
}

function TablesList({ connection, database, onOpen }: TablesListProps) {
  const tables = useResource(() => listTables(connection, database));

  const columns: Column<TableInfo>[] = [
    {
      key: "name",
      header: "Table",
      priority: 1,
      sortValue: (t) => t.name,
      render: (t) => <span className="dt-strong">{t.name}</span>,
    },
    {
      key: "rows",
      header: "Rows (est.)",
      priority: 2,
      align: "end",
      sortValue: (t) => t.rows_estimate,
      render: (t) => t.rows_estimate.toLocaleString(),
    },
    {
      key: "size",
      header: "Size",
      priority: 2,
      align: "end",
      sortValue: (t) => t.size_bytes,
      render: (t) => formatBytes(t.size_bytes),
    },
  ];

  return (
    <Section
      title="Tables"
      padding="none"
      meta={tables.data ? `${tables.data.length}` : undefined}
      isEmpty={
        !tables.loading && !tables.error && (tables.data?.length ?? 0) === 0
      }
      empty={{
        icon: Table2,
        title: "No tables",
        description: `"${database}" has no tables yet.`,
      }}
      actions={
        <Button
          size="sm"
          variant="secondary"
          loading={tables.refreshing}
          onClick={tables.refresh}
        >
          Refresh
        </Button>
      }
    >
      {tables.error ? (
        <Callout tone="danger">{tables.error}</Callout>
      ) : (
        <DataTable
          columns={columns}
          rows={tables.data ?? []}
          rowKey={(t) => t.name}
          loading={tables.loading}
          defaultSort={{ key: "name", dir: "asc" }}
          onRowClick={(t) => onOpen(t.name)}
          caption={`Tables in ${database}`}
        />
      )}
    </Section>
  );
}

/* ----------------------------------------------------------- Table view -- */

interface TableViewProps {
  connection: string;
  database: string;
  table: string;
  onBack: () => void;
}

/**
 * Paginates server-side: each page is its own SELECT with LIMIT/OFFSET and the
 * column sort becomes ORDER BY. The fetch is a plain effect rather than
 * `useResource` because that hook only fires on mount — pages and sorts change
 * the request without remounting.
 */
function TableView({ connection, database, table, onBack }: TableViewProps) {
  const [offset, setOffset] = useState(0);
  const [sort, setSort] = useState<SortState | undefined>(undefined);
  const [result, setResult] = useState<ResultSet | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const orderBy = orderByFrom(sort);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    fetchTableRows(connection, database, table, PAGE_SIZE, offset, orderBy)
      .then((rs) => {
        if (!cancelled) setResult(rs);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(errorText(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [connection, database, table, offset, orderBy]);

  const columns = useMemo(
    () => (result ? gridColumns(result, true) : []),
    [result],
  );
  const rows = useMemo(() => (result ? toGridRows(result) : []), [result]);
  const fetched = result?.rows.length ?? 0;
  const total = result?.total ?? null;

  return (
    <Section
      title={table}
      icon={Table2}
      padding="none"
      meta={result ? pageLabel(offset, fetched, total) : undefined}
      actions={
        <>
          <Button
            size="sm"
            variant="secondary"
            icon={ChevronLeft}
            onClick={onBack}
          >
            All tables
          </Button>
          <Button
            size="sm"
            variant="secondary"
            icon={ChevronLeft}
            disabled={loading || offset === 0}
            onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}
          >
            Prev
          </Button>
          <Button
            size="sm"
            variant="secondary"
            icon={ChevronRight}
            disabled={loading || !hasNextPage(offset, fetched, total)}
            onClick={() => setOffset(offset + PAGE_SIZE)}
          >
            Next
          </Button>
        </>
      }
    >
      {error ? (
        <Callout tone="danger">{error}</Callout>
      ) : (
        <DataTable
          columns={
            columns.length > 0
              ? columns
              : [{ key: "_", header: "", priority: 1, render: () => null }]
          }
          rows={rows}
          rowKey={(r) => `${offset}-${r.i}`}
          loading={loading && !result}
          sort={sort}
          onSortChange={(next) => {
            setSort(next);
            setOffset(0);
          }}
          density="compact"
          minWidth={0}
          caption={`Rows of ${table}`}
        />
      )}
    </Section>
  );
}

/* --------------------------------------------------------------- Query -- */

interface QuerySectionProps {
  connection: string;
  database: string;
}

function QuerySection({ connection, database }: QuerySectionProps) {
  const [sql, setSql] = useState("");
  const [allowWrite, setAllowWrite] = useState(false);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<ResultSet | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function run() {
    setRunning(true);
    setError(null);
    try {
      setResult(await runQuery(connection, database, sql, allowWrite));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setRunning(false);
    }
  }

  const columns = useMemo(
    () => (result ? gridColumns(result, false) : []),
    [result],
  );
  const rows = useMemo(() => (result ? toGridRows(result) : []), [result]);

  return (
    <Section
      title="Query"
      description={
        <>
          Runs against <strong>{database}</strong>. Writes are blocked unless{" "}
          <strong>Allow writes</strong> is on — only a single SELECT / SHOW /
          DESCRIBE / EXPLAIN / WITH statement is accepted otherwise.
        </>
      }
    >
      <Stack gap={4}>
        <Field label="SQL" htmlFor="db-browse-sql">
          <TextArea
            id="db-browse-sql"
            mono
            rows={6}
            value={sql}
            spellCheck={false}
            placeholder="SELECT * FROM ... LIMIT 100"
            onChange={(e) => setSql(e.target.value)}
            onKeyDown={(e) => {
              if ((e.ctrlKey || e.metaKey) && e.key === "Enter" && sql.trim())
                void run();
            }}
          />
        </Field>
        <Toolbar
          end={
            <Button
              variant="primary"
              icon={Play}
              loading={running}
              disabled={!sql.trim()}
              onClick={() => void run()}
            >
              Run
            </Button>
          }
        >
          <Switch
            checked={allowWrite}
            onChange={setAllowWrite}
            label="Allow writes"
            description="Lets INSERT / UPDATE / DELETE / DDL and multi-statement input through."
          />
        </Toolbar>

        {error && (
          <Callout tone="danger" onDismiss={() => setError(null)}>
            {error}
          </Callout>
        )}

        {result && !error && (
          <DataTable
            columns={
              columns.length > 0
                ? columns
                : [{ key: "_", header: "", priority: 1, render: () => null }]
            }
            rows={rows}
            rowKey={(r) => String(r.i)}
            density="compact"
            minWidth={0}
            empty={{
              title: columns.length > 0 ? "No rows returned" : "Statement ran",
              description:
                columns.length > 0
                  ? undefined
                  : "It produced no result set — nothing to show.",
              size: "sm",
            }}
            caption="Query results"
          />
        )}
      </Stack>
    </Section>
  );
}

/* --------------------------------------------------------------- Panel -- */

interface ConnectionBrowserProps {
  connections: BrowseConnection[];
  connection: string;
  onChangeConnection: (base: string) => void;
}

/**
 * Everything scoped to one server. Mounted with `key={connection}` by the
 * panel so switching servers remounts it: `useResource` fetches on mount only,
 * and a remount also throws away the previous server's database/table picks
 * instead of leaving them dangling against a server that lacks them.
 */
function ConnectionBrowser({
  connections,
  connection,
  onChangeConnection,
}: ConnectionBrowserProps) {
  const confirmAction = useConfirm();
  const [database, setDatabase] = useState<string>("");
  const [table, setTable] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const databases = useResource(() => listDatabaseNames(connection), {
    enabled: connection !== "",
  });
  const dbNames = useMemo(() => databases.data ?? [], [databases.data]);

  // Default the database to the one named after the service, falling back to
  // the first on the server.
  useEffect(() => {
    if (dbNames.length === 0 || dbNames.includes(database)) return;
    setDatabase(dbNames.includes(connection) ? connection : dbNames[0]);
    setTable(null);
  }, [dbNames, database, connection]);

  async function handleDump() {
    const path = await save({
      title: `Dump "${database}" to…`,
      defaultPath: dumpFileName(database, new Date()),
      filters: [{ name: "SQL dump", extensions: ["sql"] }],
    });
    if (!path) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setNotice(await dumpDatabase(connection, database, path));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleRestore() {
    const selected = await open({
      multiple: false,
      title: `Select a dump to restore into "${database}"`,
      filters: [{ name: "SQL dump", extensions: ["sql"] }],
    });
    if (!selected || Array.isArray(selected)) return;
    const ok = await confirmAction(
      `Restoring ${selected} runs it against "${database}" on "${connection}". Existing data may be overwritten. This cannot be undone.`,
      { title: "Restore from dump?", confirmLabel: "Restore", danger: true },
    );
    if (!ok) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setNotice(await restoreDatabase(connection, selected, database));
      setTable(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  const connectionOptions = connections.map((c) => ({
    value: c.base,
    label: c.running ? c.base : `${c.base} (stopped)`,
  }));
  const ready = connection !== "" && database !== "";

  return (
    <Stack gap={5}>
      <Toolbar
        end={
          <>
            <Button
              variant="secondary"
              icon={Download}
              disabled={!ready || busy}
              loading={busy}
              onClick={() => void handleDump()}
            >
              Dump…
            </Button>
            <Button
              variant="secondary"
              icon={Upload}
              disabled={!ready || busy}
              onClick={() => void handleRestore()}
            >
              Restore…
            </Button>
          </>
        }
      >
        <Select
          aria-label="Connection"
          value={connection}
          options={connectionOptions}
          placeholder="Connection"
          onChange={onChangeConnection}
        />
        <Select
          aria-label="Database"
          value={database}
          options={dbNames.map((d) => ({ value: d, label: d }))}
          placeholder={databases.loading ? "Loading…" : "Database"}
          disabled={connection === "" || dbNames.length === 0}
          onChange={(v) => {
            setDatabase(v);
            setTable(null);
          }}
        />
      </Toolbar>

      {notice && (
        <Callout tone="success" onDismiss={() => setNotice(null)}>
          {notice}
        </Callout>
      )}
      {(error || databases.error) && (
        <Callout
          tone="danger"
          onDismiss={error ? () => setError(null) : undefined}
        >
          {error ?? databases.error}
        </Callout>
      )}

      {ready && table === null && (
        <TablesList
          key={database}
          connection={connection}
          database={database}
          onOpen={setTable}
        />
      )}
      {ready && table !== null && (
        <TableView
          key={`${database}/${table}`}
          connection={connection}
          database={database}
          table={table}
          onBack={() => setTable(null)}
        />
      )}
      {ready && (
        <QuerySection
          key={database}
          connection={connection}
          database={database}
        />
      )}
    </Stack>
  );
}

export default function BrowsePanel({ connections }: BrowsePanelProps) {
  const [connection, setConnection] = useState<string>(
    connections[0]?.base ?? "",
  );

  if (connections.length === 0) {
    return (
      <EmptyState
        icon={Table2}
        title="Nothing to browse"
        description="Create a MySQL or PostgreSQL database first — the browser works with those two engines."
      />
    );
  }

  return (
    <ConnectionBrowser
      key={connection}
      connections={connections}
      connection={connection}
      onChangeConnection={setConnection}
    />
  );
}
