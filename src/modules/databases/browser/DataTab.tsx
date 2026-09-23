import { useCallback, useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Checkbox,
  Column,
  DataTable,
  IconButton,
  Menu,
  SearchInput,
  Icon,
  Section,
  Select,
  SortState,
  Stack,
  Toolbar,
} from "../../../ui";
import {
  ChevronLeft,
  ChevronRight,
  Download,
  FileJson,
  FileSpreadsheet,
  FileText,
  Folder,
  Link2,
  Pencil,
  Plus,
  RefreshCw,
  Rows3,
  Save as SaveIcon,
  Table2,
  Trash2,
  Undo2,
} from "../../../ui/icons";
import {
  applyEdits,
  Cell,
  ExportFormat,
  exportResult,
  fetchTableRows,
  Filter,
  ResultSet,
  TableStructure,
} from "../api";
import { dumpFileName, hasNextPage, orderByFrom, pageLabel } from "../utils";
import AddColumnModal from "./AddColumnModal";
import ColumnManager from "./ColumnManager";
import EditableCell from "./EditableCell";
import FilterBar, { AddFilterButton } from "./FilterBar";
import RowDrawer from "./RowDrawer";
import {
  blankRow,
  cellValue,
  changeCount,
  editability,
  EditableRow,
  isCellChanged,
  isNewRow,
  isRowChanged,
  pendingEdits,
  revertRow,
  rowsFromResult,
  setCell,
  toggleDeleted,
} from "./edits";
import { activeFilters, DraftFilter, filterKey } from "./filters";
import {
  filterRows,
  headerTick,
  PAGE_SIZES,
  toggleAll,
  toggleOne,
  reorderColumn,
  visibleColumns,
} from "./grid";
import {
  isRelationKey,
  linkFilter,
  Navigation,
  outgoingLinks,
  relationColumns,
  relationFilter,
  relationKey,
  relationLabel,
  relationsByKey,
  splitResult,
} from "./relations";
import { useDragScroll } from "./useDragScroll";

export interface DataTabProps {
  connection: string;
  database: string;
  table: string;
  objectKind: "table" | "view";
  structure: TableStructure | null;
  writable: boolean;
  engine: "mysql" | "postgres";
  /** Filters to open with, when arrived at by following a relation. */
  initialFilters?: Filter[];
  /** Follow a link or a relation chip into another table. */
  onNavigate: (to: Navigation) => void;
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

let draftSeq = 0;
const asDrafts = (filters: Filter[]): DraftFilter[] =>
  filters.map((f) => ({ ...f, id: `nav${(draftSeq += 1)}` }));

/**
 * The grid.
 *
 * Paginates and sorts server-side — each page is its own SELECT, and the
 * column sort becomes ORDER BY — because the alternative is pulling a whole
 * table into the renderer to sort a page of it.
 *
 * Edits are staged locally and sent as one transaction on Save, so a mistyped
 * cell costs nothing and a batch either lands whole or not at all.
 */
export default function DataTab({
  connection,
  database,
  table,
  objectKind,
  structure,
  writable,
  engine,
  initialFilters,
  onNavigate,
}: DataTabProps) {
  const confirmAction = useConfirm();
  const [offset, setOffset] = useState(0);
  const [pageSize, setPageSize] = useState<number>(100);
  const [sort, setSort] = useState<SortState | undefined>(undefined);
  const [drafts, setDrafts] = useState<DraftFilter[]>(() =>
    asDrafts(initialFilters ?? []),
  );
  const [query, setQuery] = useState("");
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const [order, setOrder] = useState<string[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [editingRowId, setEditingRowId] = useState<string | null>(null);
  const [addingColumn, setAddingColumn] = useState(false);

  const [result, setResult] = useState<ResultSet | null>(null);
  const [rows, setRows] = useState<EditableRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // The read-only reminder can be put away for as long as this table is open.
  const [readOnlyNoticeDismissed, setReadOnlyNoticeDismissed] = useState(false);

  const orderBy = orderByFrom(sort);
  const filters = useMemo(() => activeFilters(drafts), [drafts]);
  // Serialized: both are fresh arrays each render, and an effect keyed on the
  // array itself would refetch on every keystroke in the filter bar.
  const filtersKey = filterKey(filters);
  const relations = useMemo(() => relationColumns(structure), [structure]);
  const relationsKey = JSON.stringify(relations);

  const grid = useDragScroll<HTMLDivElement>();

  const load = useCallback(
    async (signal: { cancelled: boolean }) => {
      setLoading(true);
      setError(null);
      try {
        const rs = await fetchTableRows(
          connection,
          database,
          table,
          pageSize,
          offset,
          orderBy,
          JSON.parse(filtersKey),
          JSON.parse(relationsKey),
        );
        if (signal.cancelled) return;
        setResult(rs);
        setRows(rowsFromResult(rs));
        setSelected(new Set());
      } catch (e) {
        if (!signal.cancelled) setError(errorText(e));
      } finally {
        if (!signal.cancelled) setLoading(false);
      }
    },
    [
      connection,
      database,
      table,
      pageSize,
      offset,
      orderBy,
      filtersKey,
      relationsKey,
    ],
  );

  useEffect(() => {
    const signal = { cancelled: false };
    void load(signal);
    return () => {
      signal.cancelled = true;
    };
  }, [load]);

  const { editable, reason } = editability(structure, objectKind);
  const canEdit = editable && writable;

  const { dataColumns, relationIndexes } = useMemo(
    () =>
      result ? splitResult(result) : { dataColumns: [], relationIndexes: [] },
    [result],
  );
  const allColumns = useMemo(
    () => dataColumns.map((c) => c.name),
    [dataColumns],
  );
  /* Relations are columns like any other as far as hiding and ordering go —
     they are just the ones whose key is not a column name. Keeping them in one
     list is what lets a relation be moved in among the table's own columns. */
  const relationLookup = useMemo(() => relationsByKey(relations), [relations]);
  const allKeys = useMemo(
    () => [...allColumns, ...relations.map(relationKey)],
    [allColumns, relations],
  );
  const shownKeys = useMemo(
    () => visibleColumns(allKeys, hidden, order),
    [allKeys, hidden, order],
  );
  const labelOf = useCallback(
    (key: string) => relationLookup.get(key)?.relation.table ?? key,
    [relationLookup],
  );
  const links = useMemo(() => outgoingLinks(structure), [structure]);

  const shownRows = useMemo(
    () => filterRows(rows, allColumns, query, isRowChanged),
    [rows, allColumns, query, isRowChanged],
  );
  const staged = changeCount(rows);
  const editingRow = rows.find((r) => r.id === editingRowId) ?? null;

  function patchRow(id: string, next: EditableRow | null) {
    setRows((current) =>
      next === null
        ? current.filter((r) => r.id !== id)
        : current.map((r) => (r.id === id ? next : r)),
    );
    if (next === null && editingRowId === id) setEditingRowId(null);
  }

  function navigateTo(to: Navigation | null) {
    if (to) onNavigate(to);
  }

  async function handleSave() {
    const edits = pendingEdits(rows, allColumns);
    if (edits.length === 0) return;
    const destructive = edits.filter((e) => e.kind === "delete").length;
    const ok = await confirmAction(
      `${edits.length} statement${edits.length === 1 ? "" : "s"} will run against "${table}"${
        destructive > 0
          ? `, including ${destructive} row deletion${destructive === 1 ? "" : "s"}`
          : ""
      }. This cannot be undone.`,
      { title: "Save changes?", confirmLabel: "Save", danger: destructive > 0 },
    );
    if (!ok) return;

    setSaving(true);
    setError(null);
    setNotice(null);
    try {
      setNotice(await applyEdits(connection, database, table, edits));
      // Reloaded rather than patched in place: defaults, triggers and
      // auto-increment keys mean the saved row is rarely the one sent.
      await load({ cancelled: false });
      setEditingRowId(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSaving(false);
    }
  }

  async function handleDiscard() {
    if (staged === 0) return;
    const ok = await confirmAction(
      `${staged} row${staged === 1 ? "" : "s"} with unsaved changes will go back to what's in the database.`,
      { title: "Discard changes?", confirmLabel: "Discard", danger: true },
    );
    if (ok && result) {
      setRows(rowsFromResult(result));
      setEditingRowId(null);
    }
  }

  /** Stages a delete for every ticked row, rather than doing it row by row. */
  function deleteSelected() {
    setRows((current) =>
      current.flatMap((row) => {
        if (!selected.has(row.id)) return [row];
        const next = toggleDeleted(row);
        return next === null ? [] : [next];
      }),
    );
    setSelected(new Set());
  }

  async function handleExport(format: ExportFormat) {
    const path = await save({
      title: `Export "${table}" as ${format.toUpperCase()}`,
      defaultPath: dumpFileName(table, new Date()).replace(
        /\.sql$/,
        `.${format}`,
      ),
      filters: [{ name: format.toUpperCase(), extensions: [format] }],
    });
    if (!path) return;
    setError(null);
    setNotice(null);
    try {
      // The filters and sort go along, but not the page offset: "export just
      // the page I'm looking at" is almost never what is wanted.
      setNotice(
        await exportResult({
          connection,
          database,
          table,
          filters,
          orderBy,
          format,
          path,
        }),
      );
    } catch (e) {
      setError(errorText(e));
    }
  }

  const tick = headerTick(selected, shownRows);

  const columns: Column<EditableRow>[] = useMemo(() => {
    const select: Column<EditableRow> = {
      key: "_select",
      header: (
        <Checkbox
          checked={tick === "all"}
          indeterminate={tick === "some"}
          label="Select all rows on this page"
          hideLabel
          onChange={() => setSelected(toggleAll(selected, shownRows))}
        />
      ),
      headerLabel: "Select",
      priority: 1,
      width: "1px",
      render: (row) => (
        <Checkbox
          checked={selected.has(row.id)}
          label={`Select row ${row.id}`}
          hideLabel
          onChange={() => setSelected(toggleOne(selected, row.id))}
        />
      ),
    };

    /** A column holding one of the table's own values. */
    const dataColumn = (name: string): Column<EditableRow> => {
      const meta = dataColumns.find((c) => c.name === name);
      const link = links.get(name);
      return {
        key: name,
        header: meta?.type ? <span title={meta.type}>{name}</span> : name,
        headerLabel: name,
        priority: 1,
        mono: true,
        /* Marks the column sortable without sorting anything here: the page
           already came back in the server's ORDER BY, and re-sorting it in the
           browser would use a different collation from the one the LIMIT/OFFSET
           was taken under — scrambling the page relative to its own paging.
           An all-equal comparator leaves the server's order intact, since
           Array.prototype.sort is stable. */
        sortValue: () => "",
        render: (row) => {
          const value = cellValue(row, name);
          const cell = (
            <EditableCell
              value={value}
              changed={isCellChanged(row, name)}
              editable={canEdit && !row.deleted}
              onCommit={(next: Cell) =>
                patchRow(row.id, setCell(row, name, next))
              }
            />
          );
          if (!link || value === null) return cell;
          return (
            <span className="db-fk-cell">
              {cell}
              <button
                type="button"
                className="db-link-chip"
                title={`View in ${link.table} (${value})`}
                onClick={() => {
                  const f = linkFilter(link, value);
                  if (f) navigateTo({ table: link.table, filters: f });
                }}
              >
                <Icon icon={Link2} size="xs" />
                {link.table}
              </button>
            </span>
          );
        },
      };
    };

    /** A column counting the rows of another table that point at this one. */
    const relationColumn = (key: string): Column<EditableRow> => {
      const { relation, index } = relationLookup.get(key)!;
      const label = relationLabel(relation);
      return {
        key,
        header: (
          <span className="db-relation-header">
            <Icon icon={Folder} size="xs" />
            {label}
          </span>
        ),
        headerLabel: label,
        priority: 2,
        render: (row) => {
          // New rows have no key yet, so nothing points at them.
          if (isNewRow(row)) return <span className="u-muted">—</span>;
          const countColumn =
            result?.columns[relationIndexes[index]]?.name ?? "";
          const count = row.original?.[countColumn] ?? "0";
          const to = relationFilter(relation, row.original ?? {});
          return (
            <button
              type="button"
              className="db-relation-chip"
              title={`View related ${label}`}
              disabled={!to}
              onClick={() => to && navigateTo({ table: label, filters: to })}
            >
              <Icon icon={Folder} size="xs" />
              {label}
              <span className="db-relation-count">{count}</span>
            </button>
          );
        },
      };
    };

    const body = shownKeys.map((key) =>
      relationLookup.has(key) ? relationColumn(key) : dataColumn(key),
    );

    const actions: Column<EditableRow> = {
      key: "_row_actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (row) => (
        <ActionGroup>
          <IconButton
            icon={Pencil}
            label="Open this row"
            size="sm"
            variant="ghost"
            onClick={() => setEditingRowId(row.id)}
          />
          {canEdit && isRowChanged(row) && !isNewRow(row) && (
            <IconButton
              icon={Undo2}
              label="Revert this row"
              size="sm"
              variant="ghost"
              onClick={() => patchRow(row.id, revertRow(row))}
            />
          )}
          {canEdit && (
            <IconButton
              icon={Trash2}
              label={
                isNewRow(row)
                  ? "Discard this new row"
                  : row.deleted
                    ? "Keep this row"
                    : "Delete this row"
              }
              size="sm"
              variant="ghost"
              onClick={() => patchRow(row.id, toggleDeleted(row))}
            />
          )}
        </ActionGroup>
      ),
    };

    return [...(canEdit ? [select] : []), ...body, actions];
  }, [
    shownKeys,
    dataColumns,
    relationLookup,
    relationIndexes,
    result,
    links,
    canEdit,
    selected,
    shownRows,
    tick,
  ]);

  const fetched = result?.rows.length ?? 0;
  const total = result?.total ?? null;

  return (
    <Stack gap={4} className="db-tab-fill">
      <Toolbar
        end={
          <>
            <span className="u-muted db-rows-label">Rows</span>
            <Select
              aria-label="Rows per page"
              size="sm"
              value={String(pageSize)}
              options={PAGE_SIZES.map((n) => ({
                value: String(n),
                label: String(n),
              }))}
              onChange={(v) => {
                setPageSize(Number(v));
                setOffset(0);
              }}
            />
            <ColumnManager
              columns={allKeys}
              hidden={hidden}
              onChange={setHidden}
              order={order}
              onReorder={setOrder}
              labelOf={labelOf}
              isRelation={isRelationKey}
            />
            {canEdit && (
              <>
                <Button
                  size="sm"
                  variant="secondary"
                  icon={Rows3}
                  onClick={() => {
                    const row = blankRow();
                    setRows((r) => [...r, row]);
                    setEditingRowId(row.id);
                  }}
                >
                  Add row
                </Button>
                <Button
                  size="sm"
                  variant="secondary"
                  icon={Plus}
                  onClick={() => setAddingColumn(true)}
                >
                  Add column
                </Button>
              </>
            )}
            <Menu
              label="Export"
              icon={Download}
              items={[
                {
                  key: "csv",
                  label: "Export as CSV…",
                  icon: FileSpreadsheet,
                  onClick: () => void handleExport("csv"),
                },
                {
                  key: "json",
                  label: "Export as JSON…",
                  icon: FileJson,
                  onClick: () => void handleExport("json"),
                },
                {
                  key: "sql",
                  label: "Export as INSERTs…",
                  icon: FileText,
                  onClick: () => void handleExport("sql"),
                },
              ]}
            />
            <IconButton
              icon={RefreshCw}
              label="Reload rows"
              size="sm"
              variant="secondary"
              onClick={() => void load({ cancelled: false })}
            />
          </>
        }
      >
        {/* Narrows the page already loaded — the filter bar below is what asks
            the server. Labelled so the two aren't mistaken for each other. */}
        <SearchInput
          value={query}
          onChange={setQuery}
          placeholder="Filter loaded rows…"
          aria-label="Filter the rows on this page"
          width="16rem"
        />
        <AddFilterButton
          columns={allColumns}
          filters={drafts}
          onChange={(next) => {
            setDrafts(next);
            setOffset(0);
          }}
          disabled={loading && !result}
        />
      </Toolbar>

      <FilterBar
        columns={allColumns}
        filters={drafts}
        onChange={(next) => {
          setDrafts(next);
          setOffset(0);
        }}
        disabled={loading && !result}
      />

      {reason && <Callout tone="info">{reason}</Callout>}
      {editable && !writable && !readOnlyNoticeDismissed && (
        <Callout tone="info" onDismiss={() => setReadOnlyNoticeDismissed(true)}>
          This connection is in read-only mode. Switch it to{" "}
          <strong>Read &amp; write</strong> above to edit rows.
        </Callout>
      )}
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}
      {notice && (
        <Callout tone="success" onDismiss={() => setNotice(null)}>
          {notice}
        </Callout>
      )}

      <Section
        title={table}
        icon={Table2}
        padding="none"
        fill
        meta={result ? pageLabel(offset, fetched, total) : undefined}
        actions={
          <>
            {selected.size > 0 && canEdit && (
              <Button
                size="sm"
                variant="secondary"
                icon={Trash2}
                onClick={deleteSelected}
              >
                {`Delete ${selected.size} selected`}
              </Button>
            )}
            {staged > 0 && (
              <>
                <Badge tone="warning">
                  {staged} unsaved {staged === 1 ? "row" : "rows"}
                </Badge>
                <Button
                  size="sm"
                  variant="secondary"
                  icon={Undo2}
                  onClick={() => void handleDiscard()}
                >
                  Discard
                </Button>
                <Button
                  size="sm"
                  variant="primary"
                  icon={SaveIcon}
                  loading={saving}
                  onClick={() => void handleSave()}
                >
                  Save
                </Button>
              </>
            )}
          </>
        }
      >
        <DataTable
          scrollRef={grid.ref}
          scrollProps={grid.handlers}
          columns={
            columns.length > 0
              ? columns
              : [{ key: "_", header: "", priority: 1, render: () => null }]
          }
          rows={shownRows}
          rowKey={(r) => r.id}
          rowClassName={(r) =>
            r.deleted
              ? "db-row-deleted"
              : isRowChanged(r)
                ? "db-row-changed"
                : undefined
          }
          loading={loading && !result}
          sort={sort}
          onSortChange={(next) => {
            setSort(next);
            setOffset(0);
          }}
          density="compact"
          minWidth={0}
          fillHeight
          /* Values are what you came to read; a column squeezed to fit the box
             is a column you have to widen something else to see. */
          widthMode="content"
          onReorder={(column, target) =>
            setOrder(reorderColumn(allKeys, order, column, target))
          }
          caption={`Rows of ${table}`}
        />
      </Section>

      <Toolbar
        end={
          <>
            <Button
              size="sm"
              variant="secondary"
              icon={ChevronLeft}
              disabled={loading || offset === 0}
              onClick={() => setOffset(Math.max(0, offset - pageSize))}
            >
              Prev
            </Button>
            <Button
              size="sm"
              variant="secondary"
              icon={ChevronRight}
              disabled={
                loading || !hasNextPage(offset, fetched, total, pageSize)
              }
              onClick={() => setOffset(offset + pageSize)}
            >
              Next
            </Button>
          </>
        }
      >
        <span className="u-muted">
          {result ? pageLabel(offset, fetched, total) : ""}
          {query.trim() !== "" && ` · ${shownRows.length} shown by filter`}
        </span>
      </Toolbar>

      {editingRow && (
        <RowDrawer
          table={table}
          row={editingRow}
          structure={structure}
          columns={allColumns}
          editable={canEdit && !editingRow.deleted}
          onChange={(next) => patchRow(next.id, next)}
          onClose={() => setEditingRowId(null)}
          onFollow={(column, value) => {
            const link = links.get(column);
            const f = link && linkFilter(link, value);
            if (link && f) {
              setEditingRowId(null);
              navigateTo({ table: link.table, filters: f });
            }
          }}
        />
      )}

      {addingColumn && (
        <AddColumnModal
          connection={connection}
          database={database}
          table={table}
          engine={engine}
          onClose={() => setAddingColumn(false)}
          onAdded={(message) => {
            setAddingColumn(false);
            setNotice(message);
            void load({ cancelled: false });
          }}
        />
      )}
    </Stack>
  );
}
