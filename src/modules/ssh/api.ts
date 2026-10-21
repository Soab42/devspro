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

/** Saves a pasted key pair into ~/.ssh under `name`. The passphrase only
    works out a missing public key; it is never stored. */
export const importKey = (args: {
  name: string;
  privateKey: string;
  publicKey: string;
  passphrase: string;
}) =>
  invoke<SshKey>("ssh_keys_import", {
    args: {
      name: args.name,
      private_key: args.privateKey,
      public_key: args.publicKey,
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

/** What the system `ssh` will make of a private key file's mode. */
export interface KeyPermissions {
  /** Permission bits, e.g. 0o664. */
  mode: number;
  /** ssh will refuse it: you own it and group or others have access (on
   *  Windows: its ACL lets in someone besides you, SYSTEM and Administrators). */
  too_open: boolean;
  /** Windows only: the accounts besides you the ACL lets at the key, an owner
   *  that isn't you marked " (owner)". Absent on Unix, where `mode` says it. */
  others?: string[];
}

export const keyPermissions = (path: string) =>
  invoke<KeyPermissions>("ssh_key_permissions", { path });

/** `chmod 600` (on Windows, an ACL granting only you). Rejects a file you
 *  don't own, with what to run instead. */
export const restrictKey = (path: string) =>
  invoke<void>("ssh_key_restrict", { path });

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

/** What went into a zip download; `skipped` lists what the server refused, as "path: why". */
export interface ZipSummary {
  bytes: number;
  skipped: string[];
}

/** Packs `remotes` (files or folders) into one zip at `local`. */
export const sftpDownloadZip = (
  session: string,
  remotes: string[],
  local: string,
  transfer?: string,
) =>
  invoke<ZipSummary>("sftp_download_zip", {
    session,
    remotes,
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

/** One raw sample of a host for the terminal's status strip; see stats.rs. */
export interface HostSample {
  hostname: string;
  cpu_idle: number;
  cpu_total: number;
  mem_total_kb: number;
  mem_available_kb: number;
  swap_total_kb: number;
  swap_free_kb: number;
  /** null for this machine on Windows, which keeps no load average. */
  load1: number | null;
  uptime_seconds: number;
  /** null for this machine on Windows. */
  users: number | null;
  rx_bytes: number;
  tx_bytes: number;
  disks: { mount: string; total_kb: number; used_kb: number; percent: number }[];
}

/** `session` null samples this machine; an SFTP session id samples its host. */
export const termHostStats = (session: string | null) =>
  invoke<HostSample>("term_host_stats", { session });
