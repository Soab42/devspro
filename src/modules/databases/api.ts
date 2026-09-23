import { invoke } from "@tauri-apps/api/core";

export type DbKind = "mysql" | "postgres" | "mongo" | "redis";

export interface DevsproContainer {
  id: string;
  name: string;
  image: string;
  state: string;
  status: string;
  ports: string;
}

export interface CreateDatabaseResult {
  message: string;
  username: string | null;
  password: string;
  db_port: number;
  ui_port: number | null;
}

export interface Credentials {
  username: string | null;
  password: string | null;
}

export interface CreateDatabaseOptions {
  name: string;
  kind: DbKind;
  version: string;
  port: number;
  username: string | null;
  password: string | null;
  withUi: boolean;
  uiPort: number | null;
}

export const listDatabases = () =>
  invoke<DevsproContainer[]>("databases_list");

export const createDatabase = (input: CreateDatabaseOptions) =>
  invoke<CreateDatabaseResult>("databases_create", { input });

export const removeDatabase = (name: string, deleteData: boolean) =>
  invoke<string>("databases_remove", { name, deleteData });

export const getDatabaseCredentials = (name: string) =>
  invoke<Credentials>("databases_get_credentials", { name });

export const listDatabaseUsers = (name: string) =>
  invoke<string[]>("databases_list_users", { name });
export const createDatabaseUser = (
  name: string,
  username: string,
  password: string,
) => invoke<string>("databases_create_user", { name, username, password });
export const dropDatabaseUser = (name: string, username: string) =>
  invoke<string>("databases_drop_user", { name, username });

export const changeAdminPassword = (name: string, newPassword: string) =>
  invoke<string>("databases_change_admin_password", { name, newPassword });
export const changeUserPassword = (
  name: string,
  username: string,
  newPassword: string,
) =>
  invoke<string>("databases_change_user_password", {
    name,
    username,
    newPassword,
  });

export const backupDatabase = (name: string) =>
  invoke<string>("databases_backup", { name });
export const restoreDatabase = (
  name: string,
  backupPath: string,
  database?: string,
) =>
  invoke<string>("databases_restore", {
    name,
    backupPath,
    database: database ?? null,
  });

// ---- Saved custom connections ----
// Databases DevsPro does not run itself: another container on this machine, a
// server installed directly on it, or a remote one. Passwords are stored by
// the backend and never sent back here — `has_password` is all the UI gets.

export type ConnectionTransport = "tcp" | "docker";
/** Only the two engines with a tabular browser can be saved. */
export type ConnectionKind = Extract<DbKind, "mysql" | "postgres">;

export interface SavedConnection {
  id: string;
  name: string;
  kind: ConnectionKind;
  transport: ConnectionTransport;
  host: string;
  port: number;
  container: string;
  username: string;
  has_password: boolean;
  default_database: string;
  created_at: string;
}

/**
 * The connection form. `password: null` means "leave the stored one alone",
 * which is what an edit sends until the field is actually touched; `""`
 * clears it, for a server that authenticates without one.
 */
export interface ConnectionInput {
  id: string | null;
  name: string;
  kind: ConnectionKind;
  transport: ConnectionTransport;
  host: string;
  port: number;
  container: string;
  username: string;
  password: string | null;
  defaultDatabase: string;
}

export const listConnections = () =>
  invoke<SavedConnection[]>("databases_connections_list");

export const saveConnection = (input: ConnectionInput) =>
  invoke<SavedConnection>("databases_connections_save", { input });

export const deleteConnection = (id: string) =>
  invoke<void>("databases_connections_delete", { id });

/** Dials the server described by an unsaved form and reports what answered. */
export const testConnection = (input: ConnectionInput) =>
  invoke<string>("databases_test_connection", { input });

/**
 * What a container's own environment says about the database inside it. The
 * official images take their admin user, password and initial database from
 * exactly these variables, so picking a container can fill in the rest of the
 * form. Every field can come back empty for a custom-built image.
 */
export interface ContainerCredentials {
  kind: ConnectionKind | null;
  username: string;
  password: string;
  database: string;
}

export const inspectContainer = (name: string) =>
  invoke<ContainerCredentials>("databases_inspect_container", { name });

// ---- Browser (MySQL / PostgreSQL only) ----
// `connection` is either a compose service name (a database DevsPro manages)
// or `custom:<id>` for a saved connection — one opaque string, so every
// command below takes the same argument whichever it is. `database` is a
// database on that server.

export interface TableInfo {
  name: string;
  /** Views are listed too, but never offered for editing. */
  kind: "table" | "view";
  rows_estimate: number;
  size_bytes: number;
}

/* ---- Structure ---- */

export interface ColumnMeta {
  name: string;
  type: string;
  nullable: boolean;
  default: string;
  primary_key: boolean;
  extra: string;
}

export interface IndexMeta {
  name: string;
  columns: string[];
  unique: boolean;
  primary: boolean;
}

export interface ForeignKeyMeta {
  name: string;
  columns: string[];
  references_table: string;
  references_columns: string[];
  on_delete: string;
  on_update: string;
}

/** A foreign key pointing *at* this table — what its rows own. */
export interface ReverseRelation {
  name: string;
  /** The table holding the foreign key. */
  table: string;
  columns: string[];
  references_columns: string[];
}

export interface TableStructure {
  columns: ColumnMeta[];
  indexes: IndexMeta[];
  /** Where this table's rows point. */
  foreign_keys: ForeignKeyMeta[];
  /** What points back at them. */
  referenced_by: ReverseRelation[];
}

/** A reverse relation to count per row, as the grid asks for it. */
export interface RelationSpec {
  table: string;
  columns: string[];
  references_columns: string[];
}

/**
 * Alias prefix the backend gives each relation's `COUNT(*)` column. Both sides
 * must agree on it — see the note beside `RELATION_PREFIX` in edit.rs.
 */
export const RELATION_PREFIX = "__rel_";

/* ---- Filters ---- */

export type FilterOp =
  | "eq"
  | "ne"
  | "contains"
  | "not_contains"
  | "starts_with"
  | "ends_with"
  | "gt"
  | "gte"
  | "lt"
  | "lte"
  | "is_null"
  | "not_null";

export interface Filter {
  column: string;
  op: FilterOp;
  value: string;
}

/* ---- Editing ---- */

/** A cell value. `null` is SQL NULL, which is not the empty string. */
export type Cell = string | null;

/** `[column, value]` pairs, matching the tuple the backend deserializes. */
export type CellPairs = [string, Cell][];

/**
 * A staged change. Note what is *absent*: which columns identify the row. The
 * backend reads the primary key from the catalogue itself, so a bad key can't
 * be sent from here and turn an edit into a table-wide UPDATE.
 */
export type PendingEdit =
  | { kind: "update"; row: CellPairs; set: CellPairs }
  | { kind: "insert"; values: CellPairs }
  | { kind: "delete"; row: CellPairs };

/* ---- Read-only / read & write ---- */

export type ConnectionMode = "ro" | "rw";

export interface HistoryEntry {
  id: string;
  connection: string;
  database: string;
  sql: string;
  ok: boolean;
  rows: number;
  duration_ms: number;
  ran_at: string;
}

export interface ResultColumn {
  name: string;
  type: string;
}

export interface ResultSet {
  columns: ResultColumn[];
  rows: string[][];
  total: number | null;
}

export const listDatabaseNames = (connection: string) =>
  invoke<string[]>("databases_list_databases", { connection });

export const listTables = (connection: string, database: string) =>
  invoke<TableInfo[]>("databases_list_tables", { connection, database });

export const fetchTableRows = (
  connection: string,
  database: string,
  table: string,
  limit: number,
  offset: number,
  orderBy?: string,
  filters: Filter[] = [],
  relations: RelationSpec[] = [],
) =>
  invoke<ResultSet>("databases_table_rows", {
    connection,
    database,
    table,
    limit,
    offset,
    orderBy: orderBy ?? null,
    filters,
    relations,
  });

export interface AddColumnInput {
  connection: string;
  database: string;
  table: string;
  name: string;
  dataType: string;
  nullable: boolean;
  defaultValue: string;
}

export const addColumn = (input: AddColumnInput) =>
  invoke<string>("databases_add_column", {
    connection: input.connection,
    database: input.database,
    table: input.table,
    name: input.name,
    dataType: input.dataType,
    nullable: input.nullable,
    defaultValue: input.defaultValue || null,
  });

export const fetchTableStructure = (
  connection: string,
  database: string,
  table: string,
) =>
  invoke<TableStructure>("databases_table_structure", {
    connection,
    database,
    table,
  });

export const applyEdits = (
  connection: string,
  database: string,
  table: string,
  edits: PendingEdit[],
) =>
  invoke<string>("databases_apply_edits", {
    connection,
    database,
    table,
    edits,
  });

export type ExportFormat = "csv" | "json" | "sql";

/**
 * Two shapes, one command. The console exports the statement it ran (`sql`);
 * the grid exports what it is showing (`table` plus its filters and sort,
 * without the page limit) and lets the backend rebuild the SELECT, which keeps
 * identifier quoting where the engine is known.
 */
export interface ExportRequest {
  connection: string;
  database: string;
  format: ExportFormat;
  path: string;
  sql?: string;
  table?: string;
  filters?: Filter[];
  orderBy?: string;
}

export const exportResult = (request: ExportRequest) =>
  invoke<string>("databases_export", {
    connection: request.connection,
    database: request.database,
    format: request.format,
    path: request.path,
    sql: request.sql ?? null,
    table: request.table ?? null,
    filters: request.filters ?? null,
    orderBy: request.orderBy ?? null,
  });

export const getMode = (connection: string) =>
  invoke<ConnectionMode>("databases_mode_get", { connection });

export const setMode = (connection: string, mode: ConnectionMode) =>
  invoke<ConnectionMode>("databases_mode_set", { connection, mode });

export const listHistory = (connection: string, limit = 50) =>
  invoke<HistoryEntry[]>("databases_history_list", { connection, limit });

export const clearHistory = (connection: string) =>
  invoke<void>("databases_history_clear", { connection });

export const runQuery = (
  connection: string,
  database: string,
  sql: string,
  allowWrite: boolean,
) =>
  invoke<ResultSet>("databases_query", {
    connection,
    database,
    sql,
    allowWrite,
  });

export const dumpDatabase = (
  connection: string,
  database: string,
  path: string,
) => invoke<string>("databases_dump", { connection, database, path });

/**
 * Restore for the Browse tab. Separate from `restoreDatabase` above, which
 * only knows how to reach a compose service and so cannot see a custom
 * connection.
 */
export const restoreDump = (
  connection: string,
  database: string,
  path: string,
) => invoke<string>("databases_restore_dump", { connection, database, path });

// Reused from the nginx/SSL module's generic free-port scanner — same command,
// no need for a second implementation of "is this port free".
export const suggestPort = (start: number) =>
  invoke<number | null>("nginx_ssl_suggest_port", { start });
