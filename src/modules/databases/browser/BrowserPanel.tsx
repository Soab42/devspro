import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  Button,
  Callout,
  EmptyState,
  Menu,
  SegmentedControl,
  Select,
  SplitView,
  Stack,
  Tabs,
  Toolbar,
  usePageChrome,
  useResource,
} from "../../../ui";
import type { SqlSchema } from "../../../ui/CodeEditor";
import {
  Database,
  Download,
  Ellipsis,
  Lock,
  LockOpen,
  PlugZap,
  SquareTerminal,
  Table2,
  Upload,
} from "../../../ui/icons";
import {
  ConnectionMode,
  dumpDatabase,
  Filter,
  fetchTableStructure,
  getMode,
  listConnections,
  listDatabaseNames,
  listTables,
  restoreDump,
  setMode,
  TableInfo,
  TableStructure,
} from "../api";
import { connectionTarget, customRef, dumpFileName } from "../utils";
import ConnectionsModal from "../components/ConnectionsModal";
/* Lazy: the SQL console pulls in CodeMirror, which is larger than the rest of
   this page put together. Behind a tab you only open to write SQL, it has no
   business loading with the Databases page — same reasoning as the Docker
   module's compose editor. */
const QueryTab = lazy(() => import("./QueryTab"));

import DataTab from "./DataTab";
import ObjectSidebar from "./ObjectSidebar";
import StructureTab from "./StructureTab";
import "./browser.css";

/** A MySQL/Postgres service in DevsPro's own compose project. */
export interface ManagedConnection {
  /** Compose service name — what the backend calls `connection`. */
  base: string;
  running: boolean;
}

/**
 * One entry in the connection picker, whether it is a database DevsPro runs or
 * a saved custom one. `ref` is the single opaque string the backend resolves;
 * everything else is for the UI.
 */
interface BrowseTarget {
  ref: string;
  label: string;
  /** Which database to open first, when the server has one by that name. */
  preferredDatabase: string;
  engine: "mysql" | "postgres";
}

export interface BrowserPanelProps {
  managed: ManagedConnection[];
}

type MainTab = "data" | "structure" | "query";

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/* ------------------------------------------------------------- Database -- */

interface DatabaseBrowserProps {
  targets: BrowseTarget[];
  target: BrowseTarget;
  onChangeTarget: (ref: string) => void;
  onManageConnections: () => void;
}

/**
 * Everything scoped to one server. Mounted with `key={target.ref}` by the
 * panel so switching servers remounts it: `useResource` fetches on mount only,
 * and a remount also throws away the previous server's database and table
 * picks instead of leaving them dangling against a server that lacks them.
 */
function DatabaseBrowser({
  targets,
  target,
  onChangeTarget,
  onManageConnections,
}: DatabaseBrowserProps) {
  const confirmAction = useConfirm();
  const connection = target.ref;
  const [database, setDatabase] = useState("");
  const [table, setTable] = useState<string | null>(null);
  const [tab, setTab] = useState<MainTab>("data");
  const [mode, setModeState] = useState<ConnectionMode>("ro");
  const [structure, setStructure] = useState<TableStructure | null>(null);
  /* Where a relation click landed: the table it opened and the filter that
     says why these rows. Bumping the key remounts the grid so the filters
     apply even when the target is the table already open. */
  const [arrivedWith, setArrivedWith] = useState<Filter[] | undefined>(
    undefined,
  );
  const [navigationKey, setNavigationKey] = useState(0);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const databases = useResource(() => listDatabaseNames(connection), {
    enabled: connection !== "",
  });
  const dbNames = useMemo(() => databases.data ?? [], [databases.data]);

  // The mode is authoritative in the backend; this is only what to draw.
  useEffect(() => {
    let cancelled = false;
    void getMode(connection).then((m) => {
      if (!cancelled) setModeState(m);
    });
    return () => {
      cancelled = true;
    };
  }, [connection]);

  const preferred = target.preferredDatabase;
  useEffect(() => {
    if (dbNames.length === 0 || dbNames.includes(database)) return;
    setDatabase(dbNames.includes(preferred) ? preferred : dbNames[0]);
    setTable(null);
  }, [dbNames, database, preferred]);

  /*
   * A plain effect, not `useResource`: that hook fetches on mount and when its
   * `enabled` flips, and the database can change without either happening.
   * It didn't, so picking a second database left the first one's tables in the
   * sidebar with nothing to open. Same reason DataTab fetches its rows this way.
   */
  const [objectList, setObjectList] = useState<TableInfo[]>([]);
  const [objectsLoading, setObjectsLoading] = useState(false);
  const [objectsError, setObjectsError] = useState<string | null>(null);

  const loadObjects = useCallback(
    async (signal: { cancelled: boolean }) => {
      if (!database) {
        setObjectList([]);
        return;
      }
      setObjectsLoading(true);
      setObjectsError(null);
      try {
        const found = await listTables(connection, database);
        if (!signal.cancelled) setObjectList(found);
      } catch (e) {
        if (!signal.cancelled) {
          setObjectList([]);
          setObjectsError(errorText(e));
        }
      } finally {
        if (!signal.cancelled) setObjectsLoading(false);
      }
    },
    [connection, database],
  );

  useEffect(() => {
    const signal = { cancelled: false };
    // Cleared up front so the previous database's tables are never on screen
    // beside the new one's name.
    setObjectList([]);
    void loadObjects(signal);
    return () => {
      signal.cancelled = true;
    };
  }, [loadObjects]);

  const selected = useMemo(
    () => objectList.find((o) => o.name === table) ?? null,
    [objectList, table],
  );

  // Loaded once per table and shared: the Structure tab renders it, and the
  // grid needs its primary key to know whether rows can be edited at all.
  useEffect(() => {
    let cancelled = false;
    setStructure(null);
    if (!table || !database) return;
    void fetchTableStructure(connection, database, table)
      .then((s) => {
        if (!cancelled) setStructure(s);
      })
      .catch(() => {
        /* the Structure tab reports this; the grid just stays read-only */
      });
    return () => {
      cancelled = true;
    };
  }, [connection, database, table]);

  /** Table → column names, for the console's completion. */
  const sqlSchema: SqlSchema = useMemo(() => {
    const entry: SqlSchema = {};
    for (const object of objectList) entry[object.name] = [];
    if (table && structure) {
      entry[table] = structure.columns.map((c) => c.name);
    }
    return entry;
  }, [objectList, table, structure]);

  const changeMode = useCallback(
    async (next: ConnectionMode) => {
      if (next === "rw") {
        const ok = await confirmAction(
          `Edits and writes will be allowed against "${target.label}". Everything you change here changes the real database.`,
          {
            title: "Allow writes?",
            confirmLabel: "Allow writes",
            danger: true,
          },
        );
        if (!ok) return;
      }
      try {
        setModeState(await setMode(connection, next));
      } catch (e) {
        setError(errorText(e));
      }
    },
    [connection, confirmAction, target.label],
  );

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
    const selectedFile = await open({
      multiple: false,
      title: `Select a dump to restore into "${database}"`,
      filters: [{ name: "SQL dump", extensions: ["sql"] }],
    });
    if (!selectedFile || Array.isArray(selectedFile)) return;
    const ok = await confirmAction(
      `Restoring ${selectedFile} runs it against "${database}" on "${target.label}". Existing data may be overwritten. This cannot be undone.`,
      { title: "Restore from dump?", confirmLabel: "Restore", danger: true },
    );
    if (!ok) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setNotice(await restoreDump(connection, database, selectedFile));
      await loadObjects({ cancelled: false });
      setTable(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  const ready = connection !== "" && database !== "";
  const writable = mode === "rw";

  // In a popped-out window the top bar already carries the module's name and
  // offers a slot beside it; the toolbar moves up there, so the row that
  // picks the connection and database doesn't take a band off the workspace.
  const slot = usePageChrome()?.actionsSlot ?? null;

  const toolbar = (
    <Toolbar
      className={
        slot
          ? "db-browser-toolbar db-browser-toolbar-topbar"
          : "db-browser-toolbar"
      }
      end={
        <>
          <SegmentedControl
            ariaLabel="Write mode"
            size="md"
            value={mode}
            options={[
              { value: "ro" as const, label: "Read only", icon: Lock },
              { value: "rw" as const, label: "Read & write", icon: LockOpen },
            ]}
            onChange={(next) => void changeMode(next)}
          />
          <Button
            variant="secondary"
            icon={PlugZap}
            onClick={onManageConnections}
          >
            Connections…
          </Button>
          {/* Behind a menu rather than two more buttons: this toolbar already
                carries the connection, the database and the write mode, and in
                a popped-out window the row ran off the edge. Dump and restore
                are also the two verbs here you reach for least. */}
          <Menu
            label="Database actions"
            icon={Ellipsis}
            size="md"
            items={[
              {
                key: "dump",
                label: "Dump…",
                icon: Download,
                disabled: !ready || busy,
                onClick: () => void handleDump(),
              },
              {
                key: "restore",
                label: "Restore from dump…",
                icon: Upload,
                disabled: !ready || busy || !writable,
                onClick: () => void handleRestore(),
              },
            ]}
          />
        </>
      }
    >
      <Select
        aria-label="Connection"
        value={connection}
        options={targets.map((t) => ({ value: t.ref, label: t.label }))}
        searchable={targets.length > 8}
        placeholder="Connection"
        onChange={onChangeTarget}
      />
      <Select
        aria-label="Database"
        value={database}
        options={dbNames.map((d) => ({ value: d, label: d }))}
        searchable={dbNames.length > 8}
        searchPlaceholder="Search databases…"
        placeholder={databases.loading ? "Loading…" : "Database"}
        disabled={connection === "" || dbNames.length === 0}
        onChange={(v) => {
          setDatabase(v);
          setTable(null);
        }}
      />
    </Toolbar>
  );

  return (
    <Stack gap={4} className="db-browser-root">
      {slot ? createPortal(toolbar, slot) : toolbar}

      {notice && (
        <Callout tone="success" onDismiss={() => setNotice(null)}>
          {notice}
        </Callout>
      )}
      {(error || databases.error || objectsError) && (
        <Callout
          tone="danger"
          onDismiss={error ? () => setError(null) : undefined}
        >
          {error ?? databases.error ?? objectsError}
        </Callout>
      )}

      {ready && (
        <SplitView
          asideLabel="Tables and views"
          asideWidth={240}
          className="db-browser"
          aside={
            <ObjectSidebar
              objects={objectList}
              loading={objectsLoading}
              selected={table}
              onSelect={(name) => {
                setTable(name);
                setArrivedWith(undefined);
                setNavigationKey((n) => n + 1);
                if (tab === "query") setTab("data");
              }}
            />
          }
        >
          <Stack gap={4} className="db-browser-main">
            <Tabs
              ariaLabel="Table view"
              active={tab}
              onChange={(id) => setTab(id as MainTab)}
              tabs={[
                { id: "data", label: "Data", icon: Table2 },
                { id: "structure", label: "Structure", icon: Database },
                { id: "query", label: "Query", icon: SquareTerminal },
              ]}
            />

            {tab === "query" ? (
              /* null, not a spinner: the chunk is served from local disk, and a
                 spinner for one frame is just a flicker. */
              <Suspense fallback={null}>
                <QueryTab
                  key={`${connection}/${database}`}
                  connection={connection}
                  database={database}
                  dialect={target.engine}
                  schema={sqlSchema}
                  writable={writable}
                />
              </Suspense>
            ) : !table ? (
              <EmptyState
                icon={Table2}
                title="Pick a table"
                description="Choose one on the left to see its rows, or switch to Query to write SQL against this database."
              />
            ) : tab === "data" ? (
              <DataTab
                /* Keyed on the navigation too, so following a relation into
                   the table you are already on reloads it with the new
                   filters instead of keeping the old ones on screen. */
                key={`${database}/${table}/${navigationKey}`}
                connection={connection}
                database={database}
                table={table}
                objectKind={selected?.kind ?? "table"}
                structure={structure}
                writable={writable}
                engine={target.engine}
                initialFilters={arrivedWith}
                onNavigate={(to) => {
                  setTable(to.table);
                  setArrivedWith(to.filters);
                  setNavigationKey((n) => n + 1);
                  setTab("data");
                }}
              />
            ) : (
              <StructureTab
                key={`${database}/${table}`}
                connection={connection}
                database={database}
                table={table}
              />
            )}
          </Stack>
        </SplitView>
      )}
    </Stack>
  );
}

/* ---------------------------------------------------------------- Panel -- */

export default function BrowserPanel({ managed }: BrowserPanelProps) {
  const connections = useResource(listConnections);
  const [ref, setRef] = useState("");
  const [managing, setManaging] = useState(false);

  const targets = useMemo<BrowseTarget[]>(
    () => [
      ...managed.map((m) => ({
        ref: m.base,
        label: m.running ? m.base : `${m.base} (stopped)`,
        preferredDatabase: m.base,
        // A managed service's engine isn't in this list; MySQL is the safe
        // default for the console's dialect, and only affects highlighting.
        engine: "mysql" as const,
      })),
      ...(connections.data ?? []).map((c) => ({
        ref: customRef(c.id),
        label: `${c.name} — ${connectionTarget(c)}`,
        preferredDatabase: c.default_database,
        engine: c.kind,
      })),
    ],
    [managed, connections.data],
  );

  // The selection is held as a ref rather than an index so that adding or
  // removing a connection doesn't silently switch which server is open.
  const target = targets.find((t) => t.ref === ref) ?? targets[0];

  const connectionsModal = managing ? (
    <ConnectionsModal
      onClose={() => setManaging(false)}
      onChanged={connections.refresh}
    />
  ) : null;

  if (!target) {
    return (
      <>
        <EmptyState
          icon={Table2}
          title="Nothing to browse"
          description="Create a MySQL or PostgreSQL database here, or add a connection to one that lives somewhere else — a remote server, one installed on this machine, or another container's."
          action={
            <Button
              variant="primary"
              icon={PlugZap}
              onClick={() => setManaging(true)}
            >
              Add a connection
            </Button>
          }
        />
        {connectionsModal}
      </>
    );
  }

  return (
    <>
      <DatabaseBrowser
        key={target.ref}
        targets={targets}
        target={target}
        onChangeTarget={setRef}
        onManageConnections={() => setManaging(true)}
      />
      {connectionsModal}
    </>
  );
}
