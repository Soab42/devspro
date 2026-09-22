import { invoke } from "@tauri-apps/api/core";

/** Connection settings sent with every command; see the module doc in Rust. */
export interface RedisConn {
  host: string;
  port: number;
  password?: string;
  db: number;
}

export interface PingView {
  ok: boolean;
  version: string;
  uptime_seconds: number;
  used_memory_human: string;
  connected_clients: number;
  total_keys: number;
  db: number;
}

export interface KeyView {
  key: string;
  type: string;
  /** -1 = no expiry, -2 = gone. */
  ttl: number;
}

export interface ScanView {
  keys: KeyView[];
  /** "0" once the iteration has wrapped. */
  cursor: string;
}

export interface ZMember {
  member: string;
  score: number;
}

export type ValueView =
  | { type: "string"; value: string }
  | { type: "list"; items: string[]; length: number }
  | { type: "hash"; fields: [string, string][]; length: number }
  | { type: "set"; members: string[]; length: number }
  | { type: "zset"; members: ZMember[]; length: number }
  | { type: "none" }
  | { type: "other"; kind: string };

export interface QueueView {
  queue: string;
  /** Full pending-list key, prefix included — pass to peekJob. */
  key: string;
  pending: number;
  delayed: number;
  reserved: number;
}

export interface QueuesView {
  queues: QueueView[];
  horizon_keys: number;
}

export interface JobPeek {
  raw: string;
  display_name: string | null;
  command_name: string | null;
  attempts: number | null;
  id: string | null;
}

export const ping = (conn: RedisConn) => invoke<PingView>("redis_ping", { conn });

export const scanKeys = (conn: RedisConn, pattern: string, cursor: string, count = 100) =>
  invoke<ScanView>("redis_scan", { conn, pattern, cursor, count });

export const getValue = (conn: RedisConn, key: string) =>
  invoke<ValueView>("redis_get", { conn, key });

export const deleteKeys = (conn: RedisConn, keys: string[]) =>
  invoke<number>("redis_delete", { conn, keys });

export const flushDb = (conn: RedisConn) => invoke<string>("redis_flush_db", { conn });

export const laravelQueues = (conn: RedisConn) =>
  invoke<QueuesView>("redis_laravel_queues", { conn });

export const peekJob = (conn: RedisConn, queue: string, index = 0) =>
  invoke<JobPeek>("redis_peek_job", { conn, queue, index });
