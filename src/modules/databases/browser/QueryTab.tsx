import { useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import {
  Badge,
  Button,
  Callout,
  Column,
  DataTable,
  IconButton,
  Menu,
  Section,
  Stack,
  Switch,
  Toolbar,
  useResource,
} from "../../../ui";
import { CodeEditor } from "../../../ui/CodeEditor";
import type { SqlSchema } from "../../../ui/CodeEditor";
import {
  Clock3,
  Download,
  FileJson,
  FileSpreadsheet,
  FileText,
  Play,
  Plus,
  Trash2,
  X,
} from "../../../ui/icons";
import {
  clearHistory,
  ExportFormat,
  exportResult,
  HistoryEntry,
  listHistory,
  ResultSet,
  runQuery,
} from "../api";
import { dumpFileName, NULL_MARKER } from "../utils";

export interface QueryTabProps {
  connection: string;
  database: string;
  dialect: "mysql" | "postgres";
  /** Tables and their columns, for completion. */
  schema: SqlSchema;
  writable: boolean;
}

interface QuerySheet {
  id: string;
  title: string;
  sql: string;
  selection: string;
  allowWrite: boolean;
  result: ResultSet | null;
  error: string | null;
  notice: string | null;
  running: boolean;
}

let sheetSeq = 0;
function newSheet(sql = ""): QuerySheet {
  sheetSeq += 1;
  return {
    id: `q${sheetSeq}`,
    title: `Query ${sheetSeq}`,
    sql,
    selection: "",
    allowWrite: false,
    result: null,
    error: null,
    notice: null,
    running: false,
  };
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** A single-line preview of a statement, for the history list. */
export function previewSql(sql: string, max = 90): string {
  const flat = sql.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max)}…` : flat;
}

function resultColumns(rs: ResultSet): Column<{ i: number; cells: string[] }>[] {
  return rs.columns.map((col, index) => ({
    // An ad-hoc query can repeat a name (`SELECT a.id, b.id`), so the index
    // keeps the keys unique.
    key: `${index}:${col.name}`,
    header: col.name,
    headerLabel: col.name,
    priority: 1,
    mono: true,
    render: (row) => {
      const value = row.cells[index] ?? "";
      return value === NULL_MARKER ? (
        <span className="u-muted">NULL</span>
      ) : (
        value
      );
    },
  }));
}

/**
 * The SQL console.
 *
 * Several sheets at once, because comparing two queries means keeping both —
 * and losing the one you just wrote to run another is the fastest way to make
 * a console annoying. Cmd/Ctrl+Enter runs; with a selection, it runs only
 * that, which is how a scratch file full of statements is actually used.
 */
export default function QueryTab({
  connection,
  database,
  dialect,
  schema,
  writable,
}: QueryTabProps) {
  const [sheets, setSheets] = useState<QuerySheet[]>(() => [newSheet()]);
  const [activeId, setActiveId] = useState(() => sheets[0].id);
  const history = useResource(() => listHistory(connection, 50));
  const [showHistory, setShowHistory] = useState(false);

  const active = sheets.find((s) => s.id === activeId) ?? sheets[0];
  // Held in a ref so the Mod-Enter binding inside CodeEditor — created once —
  // always runs the sheet as it is now, not as it was when the editor mounted.
  const activeRef = useRef(active);
  activeRef.current = active;

  useEffect(() => {
    if (!sheets.some((s) => s.id === activeId) && sheets[0]) {
      setActiveId(sheets[0].id);
    }
  }, [sheets, activeId]);

  function patch(id: string, changes: Partial<QuerySheet>) {
    setSheets((current) =>
      current.map((s) => (s.id === id ? { ...s, ...changes } : s)),
    );
  }

  async function run(sheet: QuerySheet) {
    // A selection means "run just this", which is how a sheet holding several
    // statements is worked through.
    const sql = (sheet.selection || sheet.sql).trim();
    if (!sql) return;
    patch(sheet.id, { running: true, error: null, notice: null });
    try {
      const result = await runQuery(
        connection,
        database,
        sql,
        sheet.allowWrite,
      );
      patch(sheet.id, { result, error: null });
    } catch (e) {
      patch(sheet.id, { error: errorText(e), result: null });
    } finally {
      patch(sheet.id, { running: false });
      void history.refresh();
    }
  }

  async function handleExport(format: ExportFormat) {
    const sql = (active.selection || active.sql).trim();
    if (!sql) return;
    const path = await save({
      title: `Export results as ${format.toUpperCase()}`,
      defaultPath: dumpFileName("query", new Date()).replace(
        /\.sql$/,
        `.${format}`,
      ),
      filters: [{ name: format.toUpperCase(), extensions: [format] }],
    });
    if (!path) return;
    patch(active.id, { error: null, notice: null });
    try {
      const message = await exportResult({
        connection,
        database,
        sql,
        format,
        path,
      });
      patch(active.id, { notice: message });
    } catch (e) {
      patch(active.id, { error: errorText(e) });
    }
  }

  const rows = useMemo(
    () =>
      (active.result?.rows ?? []).map((cells, i) => ({ i, cells })),
    [active.result],
  );
  const columns = useMemo(
    () => (active.result ? resultColumns(active.result) : []),
    [active.result],
  );

  const historyColumns: Column<HistoryEntry>[] = [
    {
      key: "sql",
      header: "Statement",
      priority: 1,
      mono: true,
      render: (h) => (
        <span className="dt-cell-inline">
          {!h.ok && <Badge tone="danger">failed</Badge>}
          <span title={h.sql}>{previewSql(h.sql)}</span>
        </span>
      ),
    },
    {
      key: "rows",
      header: "Rows",
      priority: 3,
      align: "end",
      render: (h) => h.rows.toLocaleString(),
    },
    {
      key: "took",
      header: "Took",
      priority: 3,
      align: "end",
      render: (h) => `${h.duration_ms} ms`,
    },
  ];

  return (
    <Stack gap={4} className="db-tab-scroll">
      <div className="db-query-tabs" role="tablist" aria-label="Query sheets">
        {sheets.map((sheet) => (
          <div
            key={sheet.id}
            className={
              sheet.id === active.id
                ? "db-query-tab db-query-tab-active"
                : "db-query-tab"
            }
          >
            <button
              type="button"
              role="tab"
              aria-selected={sheet.id === active.id}
              onClick={() => setActiveId(sheet.id)}
            >
              {sheet.title}
            </button>
            {sheets.length > 1 && (
              <IconButton
                icon={X}
                label={`Close ${sheet.title}`}
                size="sm"
                variant="ghost"
                onClick={() =>
                  setSheets((current) =>
                    current.filter((s) => s.id !== sheet.id),
                  )
                }
              />
            )}
          </div>
        ))}
        <IconButton
          icon={Plus}
          label="New query sheet"
          size="sm"
          variant="ghost"
          onClick={() => {
            const sheet = newSheet();
            setSheets((current) => [...current, sheet]);
            setActiveId(sheet.id);
          }}
        />
      </div>

      <CodeEditor
        key={active.id}
        language="sql"
        dialect={dialect}
        schema={schema}
        rows={10}
        value={active.sql}
        ariaLabel="SQL"
        onChange={(sql) => patch(active.id, { sql })}
        onSelectionChange={(selection) => patch(active.id, { selection })}
        onSubmit={() => void run(activeRef.current)}
      />

      <Toolbar
        end={
          <>
            <Menu
              label="Export results"
              icon={Download}
              items={[
                {
                  key: "csv",
                  label: "Export as CSV…",
                  icon: FileSpreadsheet,
                  disabled: !active.result,
                  onClick: () => void handleExport("csv"),
                },
                {
                  key: "json",
                  label: "Export as JSON…",
                  icon: FileJson,
                  disabled: !active.result,
                  onClick: () => void handleExport("json"),
                },
                {
                  key: "sql",
                  label: "Export as INSERTs…",
                  icon: FileText,
                  disabled: !active.result,
                  onClick: () => void handleExport("sql"),
                },
              ]}
            />
            <Button
              variant="secondary"
              icon={Clock3}
              onClick={() => setShowHistory((v) => !v)}
            >
              History
            </Button>
            <Button
              variant="primary"
              icon={Play}
              loading={active.running}
              disabled={!(active.selection || active.sql).trim()}
              onClick={() => void run(active)}
            >
              {active.selection ? "Run selection" : "Run"}
            </Button>
          </>
        }
      >
        <Switch
          checked={active.allowWrite}
          onChange={(allowWrite) => patch(active.id, { allowWrite })}
          label="Allow writes"
          description={
            writable
              ? "Lets INSERT / UPDATE / DELETE / DDL through."
              : "This connection is read-only — switch it to Read & write above."
          }
        />
      </Toolbar>

      {active.error && (
        <Callout tone="danger" onDismiss={() => patch(active.id, { error: null })}>
          {active.error}
        </Callout>
      )}
      {active.notice && (
        <Callout
          tone="success"
          onDismiss={() => patch(active.id, { notice: null })}
        >
          {active.notice}
        </Callout>
      )}

      {active.result && !active.error && (
        <Section
          title="Results"
          padding="none"
          meta={`${active.result.rows.length} ${active.result.rows.length === 1 ? "row" : "rows"}`}
        >
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
        </Section>
      )}

      {showHistory && (
        <Section
          title="History"
          icon={Clock3}
          padding="none"
          description="Everything this connection has run, including what failed."
          isEmpty={!history.loading && (history.data?.length ?? 0) === 0}
          empty={{
            icon: Clock3,
            title: "Nothing run yet",
            description: "Statements you run here are listed for later reuse.",
          }}
          actions={
            <Button
              size="sm"
              variant="secondary"
              icon={Trash2}
              disabled={(history.data?.length ?? 0) === 0}
              onClick={async () => {
                await clearHistory(connection);
                await history.refresh();
              }}
            >
              Clear
            </Button>
          }
        >
          <DataTable
            columns={historyColumns}
            rows={history.data ?? []}
            rowKey={(h) => h.id}
            loading={history.loading}
            density="compact"
            minWidth={0}
            // Click to put it back in a fresh sheet, rather than overwriting
            // whatever is in the current one.
            onRowClick={(h) => {
              const sheet = newSheet(h.sql);
              setSheets((current) => [...current, sheet]);
              setActiveId(sheet.id);
              setShowHistory(false);
            }}
            caption="Query history"
          />
        </Section>
      )}
    </Stack>
  );
}
