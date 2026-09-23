import {
  Badge,
  Callout,
  Column,
  DataTable,
  Section,
  Stack,
  useResource,
} from "../../../ui";
import { KeyRound, Link2, Table2 } from "../../../ui/icons";
import {
  ColumnMeta,
  fetchTableStructure,
  ForeignKeyMeta,
  IndexMeta,
} from "../api";

export interface StructureTabProps {
  connection: string;
  database: string;
  table: string;
}

const columnColumns: Column<ColumnMeta>[] = [
  {
    key: "name",
    header: "Column",
    priority: 1,
    sortValue: (c) => c.name,
    render: (c) => (
      <span className="dt-cell-inline">
        <span className="dt-strong">{c.name}</span>
        {c.primary_key && (
          <Badge tone="accent" icon={KeyRound}>
            PK
          </Badge>
        )}
      </span>
    ),
  },
  {
    key: "type",
    header: "Type",
    priority: 1,
    mono: true,
    sortValue: (c) => c.type,
    render: (c) => c.type,
  },
  {
    key: "nullable",
    header: "Null",
    priority: 2,
    render: (c) =>
      c.nullable ? "yes" : <span className="u-muted">no</span>,
  },
  {
    key: "default",
    header: "Default",
    priority: 3,
    mono: true,
    render: (c) =>
      c.default ? c.default : <span className="u-muted">—</span>,
  },
  {
    key: "extra",
    header: "Extra",
    priority: 3,
    render: (c) =>
      c.extra ? <Badge tone="neutral">{c.extra}</Badge> : null,
  },
];

const indexColumns: Column<IndexMeta>[] = [
  {
    key: "name",
    header: "Index",
    priority: 1,
    sortValue: (i) => i.name,
    render: (i) => (
      <span className="dt-cell-inline">
        <span className="dt-strong">{i.name}</span>
        {i.primary && (
          <Badge tone="accent" icon={KeyRound}>
            primary
          </Badge>
        )}
        {i.unique && !i.primary && <Badge tone="info">unique</Badge>}
      </span>
    ),
  },
  {
    key: "columns",
    header: "Columns",
    priority: 1,
    mono: true,
    render: (i) => i.columns.join(", "),
  },
];

const foreignKeyColumns: Column<ForeignKeyMeta>[] = [
  {
    key: "name",
    header: "Constraint",
    priority: 1,
    sortValue: (f) => f.name,
    render: (f) => <span className="dt-strong">{f.name}</span>,
  },
  {
    key: "ref",
    header: "References",
    priority: 1,
    mono: true,
    render: (f) =>
      `${f.columns.join(", ")} → ${f.references_table}(${f.references_columns.join(", ")})`,
  },
  {
    key: "rules",
    header: "On delete / update",
    priority: 3,
    render: (f) => (
      <span className="u-muted">
        {f.on_delete || "—"} / {f.on_update || "—"}
      </span>
    ),
  },
];

/**
 * What a table is made of. Both engines are asked their own catalogue and the
 * answers are normalised in the backend, so these three tables render the same
 * whichever engine is behind them.
 */
export default function StructureTab({
  connection,
  database,
  table,
}: StructureTabProps) {
  const structure = useResource(() =>
    fetchTableStructure(connection, database, table),
  );

  if (structure.error) {
    return <Callout tone="danger">{structure.error}</Callout>;
  }

  const data = structure.data;

  return (
    <Stack gap={5} className="db-tab-scroll">
      <Section title="Columns" icon={Table2} padding="none" meta={data ? `${data.columns.length}` : undefined}>
        <DataTable
          columns={columnColumns}
          rows={data?.columns ?? []}
          rowKey={(c) => c.name}
          loading={structure.loading}
          caption={`Columns of ${table}`}
          minWidth={0}
        />
      </Section>

      <Section
        title="Indexes"
        icon={KeyRound}
        padding="none"
        meta={data ? `${data.indexes.length}` : undefined}
        isEmpty={!structure.loading && data?.indexes.length === 0}
        empty={{ icon: KeyRound, title: "No indexes" }}
      >
        <DataTable
          columns={indexColumns}
          rows={data?.indexes ?? []}
          rowKey={(i) => i.name}
          loading={structure.loading}
          caption={`Indexes of ${table}`}
          minWidth={0}
        />
      </Section>

      <Section
        title="Foreign keys"
        icon={Link2}
        padding="none"
        meta={data ? `${data.foreign_keys.length}` : undefined}
        isEmpty={!structure.loading && data?.foreign_keys.length === 0}
        empty={{ icon: Link2, title: "No foreign keys" }}
      >
        <DataTable
          columns={foreignKeyColumns}
          rows={data?.foreign_keys ?? []}
          rowKey={(f) => f.name}
          loading={structure.loading}
          caption={`Foreign keys of ${table}`}
          minWidth={0}
        />
      </Section>
    </Stack>
  );
}
