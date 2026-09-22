import { invoke } from "@tauri-apps/api/core";

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

export const deleteHost = (id: string) => invoke<void>("ssh_hosts_delete", { id });

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

export const deleteKey = (name: string) => invoke<void>("ssh_keys_delete", { name });

export const installKeyOnHost = (hostId: string, keyName: string, creds: Credentials) =>
  invoke<string>("ssh_keys_install_on_host", {
    hostId,
    keyName,
    password: creds.password ?? null,
    passphrase: creds.passphrase ?? null,
  });

/* ---- terminal ---- */

export type PtyCommand = "local" | { ssh: string };

/** The id is chosen by the caller so it can subscribe to output before the
 *  process starts — see the note at the top of pty.rs. */
export const openPty = (id: string, cols: number, rows: number, command: PtyCommand) =>
  invoke<string>("pty_open", { id, cols, rows, command });

export const writePty = (id: string, data: string) => invoke<void>("pty_write", { id, data });

export const resizePty = (id: string, cols: number, rows: number) =>
  invoke<void>("pty_resize", { id, cols, rows });

export const closePty = (id: string) => invoke<void>("pty_close", { id });

/* ---- sftp ---- */

export const sftpConnect = (hostId: string, creds: Credentials) =>
  invoke<string>("sftp_connect", {
    hostId,
    password: creds.password ?? null,
    passphrase: creds.passphrase ?? null,
  });

export const sftpTrustHost = (hostId: string) => invoke<string>("sftp_trust_host", { hostId });

export const sftpDisconnect = (session: string) => invoke<void>("sftp_disconnect", { session });

export const sftpHome = (session: string) => invoke<string>("sftp_home", { session });

export const sftpList = (session: string, path: string) =>
  invoke<SftpEntry[]>("sftp_list", { session, path });

export const sftpMkdir = (session: string, path: string) =>
  invoke<void>("sftp_mkdir", { session, path });

export const sftpRename = (session: string, from: string, to: string) =>
  invoke<void>("sftp_rename", { session, from, to });

export const sftpDelete = (session: string, path: string) =>
  invoke<void>("sftp_delete", { session, path });

export const sftpDownload = (session: string, remote: string, local: string) =>
  invoke<number>("sftp_download", { session, remote, local });

export const sftpUpload = (session: string, local: string, remote: string) =>
  invoke<number>("sftp_upload", { session, local, remote });
