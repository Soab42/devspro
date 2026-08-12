import { invoke } from "@tauri-apps/api/core";

export type DbKind = "mysql" | "postgres" | "mongo" | "redis";

export interface DevsuiteContainer {
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

export const listDatabases = () => invoke<DevsuiteContainer[]>("databases_list");

export const createDatabase = (input: CreateDatabaseOptions) =>
  invoke<CreateDatabaseResult>("databases_create", { input });

export const removeDatabase = (name: string, deleteData: boolean) =>
  invoke<string>("databases_remove", { name, deleteData });

export const getDatabaseCredentials = (name: string) => invoke<Credentials>("databases_get_credentials", { name });

export const listDatabaseUsers = (name: string) => invoke<string[]>("databases_list_users", { name });
export const createDatabaseUser = (name: string, username: string, password: string) =>
  invoke<string>("databases_create_user", { name, username, password });
export const dropDatabaseUser = (name: string, username: string) =>
  invoke<string>("databases_drop_user", { name, username });

export const changeAdminPassword = (name: string, newPassword: string) =>
  invoke<string>("databases_change_admin_password", { name, newPassword });
export const changeUserPassword = (name: string, username: string, newPassword: string) =>
  invoke<string>("databases_change_user_password", { name, username, newPassword });

// Reused from the nginx/SSL module's generic free-port scanner — same command,
// no need for a second implementation of "is this port free".
export const suggestPort = (start: number) => invoke<number | null>("nginx_ssl_suggest_port", { start });
