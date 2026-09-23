import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type AuthMethod = "agent" | "key" | "password";

export interface SshHost {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
  key_path: string | null;
  created_at: string;
}

export interface SshKey {
  name: string;
  type: string;
  bits: number;
  fingerprint: string;
  comment: string;
  public_key: string;
  has_private: boolean;
  path: string;
}

export interface SftpEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  /** Unix seconds; 0 when unknown. */
  modified: number;
  permissions: string;
}

/** What the user typed for one connection attempt. Never persisted. */
export interface Credentials {
  password?: string;
  passphrase?: string;
}

export interface HostInput {
  name: string;
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
  keyPath: string | null;
}

/* ---- hosts ---- */

export const listHosts = () => invoke<SshHost[]>("ssh_hosts_list");

export const saveHost = (id: string | null, input: HostInput) =>
  invoke<SshHost>("ssh_hosts_save", { id, ...input });

export const deleteHost = (id: string) =>
  invoke<void>("ssh_hosts_delete", { id });

/* ---- keys ---- */

export const listKeys = () => invoke<SshKey[]>("ssh_keys_list");

export const generateKey = (args: {
  name: string;
  keyType: "ed25519" | "rsa";
  bits: number | null;
  comment: string;
  passphrase: string;
}) =>
  invoke<SshKey>("ssh_keys_generate", {
    args: {
      name: args.name,
      key_type: args.keyType,
      bits: args.bits,
      comment: args.comment,
      passphrase: args.passphrase,
    },
  });

export const deleteKey = (name: string) =>
  invoke<void>("ssh_keys_delete", { name });

export const installKeyOnHost = (
  hostId: string,
  keyName: string,
  creds: Credentials,
) =>
  invoke<string>("ssh_keys_install_on_host", {
    hostId,
    keyName,
    password: creds.password ?? null,
    passphrase: creds.passphrase ?? null,
  });

/* ---- terminal ---- */

/** One program run in the user's shell environment: an AI agent, an installer. */
export interface ExecSpec {
  program: string;
  args: string[];
  /** Working directory; the home directory when null. */
  cwd: string | null;
}

export type PtyCommand = "local" | { ssh: string } | { exec: ExecSpec };

/** The id is chosen by the caller so it can subscribe to output before the
 *  process starts — see the note at the top of pty.rs. */
export const openPty = (
  id: string,
  cols: number,
  rows: number,
  command: PtyCommand,
) => invoke<string>("pty_open", { id, cols, rows, command });

export const writePty = (id: string, data: string) =>
  invoke<void>("pty_write", { id, data });

export const resizePty = (id: string, cols: number, rows: number) =>
  invoke<void>("pty_resize", { id, cols, rows });

export const closePty = (id: string) => invoke<void>("pty_close", { id });

/** Make this window the owner of a session another window started (pop-out). */
export const adoptPty = (id: string) => invoke<void>("pty_adopt", { id });

/** The local shell's working directory, or null once it has exited. */
export const ptyCwd = (id: string) => invoke<string | null>("pty_cwd", { id });

/* ---- sftp ---- */

export const sftpConnect = (hostId: string, creds: Credentials) =>
  invoke<string>("sftp_connect", {
    hostId,
    password: creds.password ?? null,
    passphrase: creds.passphrase ?? null,
  });

export const sftpTrustHost = (hostId: string) =>
  invoke<string>("sftp_trust_host", { hostId });

export const sftpDisconnect = (session: string) =>
  invoke<void>("sftp_disconnect", { session });

export const sftpHome = (session: string) =>
  invoke<string>("sftp_home", { session });

export const sftpList = (session: string, path: string) =>
  invoke<SftpEntry[]>("sftp_list", { session, path });

export const sftpMkdir = (session: string, path: string) =>
  invoke<void>("sftp_mkdir", { session, path });

export const sftpRename = (session: string, from: string, to: string) =>
  invoke<void>("sftp_rename", { session, from, to });

export const sftpDelete = (session: string, path: string) =>
  invoke<void>("sftp_delete", { session, path });

/** Running total for one transfer, emitted on `sftp:progress`. */
export interface TransferProgress {
  id: string;
  done: number;
  total: number;
}

/** Pass a `transfer` id to have progress emitted as the copy runs. */
export const sftpDownload = (
  session: string,
  remote: string,
  local: string,
  transfer?: string,
) =>
  invoke<number>("sftp_download", {
    session,
    remote,
    local,
    transfer: transfer ?? null,
  });

export const sftpUpload = (
  session: string,
  local: string,
  remote: string,
  transfer?: string,
) =>
  invoke<number>("sftp_upload", {
    session,
    local,
    remote,
    transfer: transfer ?? null,
  });

export const onSftpProgress = (handler: (p: TransferProgress) => void) =>
  listen<TransferProgress>("sftp:progress", (e) => handler(e.payload));

export const sftpReadFile = (session: string, path: string) =>
  invoke<string>("sftp_read_file", { session, path });

export const sftpWriteFile = (
  session: string,
  path: string,
  contents: string,
) => invoke<string>("sftp_write_file", { session, path, contents });

/** Downloads the file to the edit cache and opens it there; resolves to the local path. */
export const sftpOpenInEditor = (
  session: string,
  path: string,
  command: string,
) => invoke<string>("sftp_open_in_editor", { session, path, command });
