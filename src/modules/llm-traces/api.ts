import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** Which protocol a trace arrived over. */
export type Source = "langfuse" | "langsmith" | "otel";

export interface StoreStats {
  traces: number;
  observations: number;
  bytes: number;
}

export interface TracesStatus {
  running: boolean;
  /** The port it's on when running; the one it will use when not. */
  port: number;
  base_url: string;
  /** Something else holds the port while ours is off. */
  conflict: string | null;
  start_with_app: boolean;
  max_traces: number;
  /** Also listening on the Docker bridges, so containers can post. */
  docker: boolean;
  /** The bridge addresses it is listening on now (Linux; empty elsewhere). */
  docker_addresses: string[];
  stats: StoreStats;
}

/** Times are microseconds since the epoch. */
export interface TraceSummary {
  id: string;
  source: Source;
  project: string | null;
  name: string | null;
  user_id: string | null;
  session_id: string | null;
  tags: string[];
  start_us: number;
  end_us: number | null;
  updated_us: number;
  observation_count: number;
  generation_count: number;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  cost: number | null;
  error_count: number;
  models: string[];
  input_preview: string | null;
  output_preview: string | null;
}

export interface TraceList {
  traces: TraceSummary[];
  /** Matching the filters, not just this page. */
  total: number;
  projects: string[];
}

export interface Observation {
  id: string;
  trace_id: string;
  parent_id: string | null;
  /** span, generation, event, tool, chain, retriever, embedding, agent… */
  kind: string;
  name: string | null;
  start_us: number | null;
  end_us: number | null;
  completion_start_us: number | null;
  model: string | null;
  model_parameters: unknown;
  input: unknown;
  output: unknown;
  metadata: unknown;
  input_tokens: number | null;
  output_tokens: number | null;
  total_tokens: number | null;
  cost: number | null;
  /** DEBUG, DEFAULT, WARNING or ERROR. */
  level: string | null;
  status_message: string | null;
  /** OpenTelemetry spans only: the attributes the mapping didn't consume. */
  attributes: unknown;
}

export interface Score {
  id: string;
  observation_id: string | null;
  name: string;
  value: number | null;
  string_value: string | null;
  comment: string | null;
  time_us: number;
}

export interface TraceDetail {
  summary: TraceSummary;
  input: unknown;
  output: unknown;
  metadata: unknown;
  release: string | null;
  version: string | null;
  environment: string | null;
  observations: Observation[];
  scores: Score[];
}

export interface RequestLog {
  time_us: number;
  method: string;
  path: string;
  status: number;
  source: Source | null;
  note: string;
  /** 127.0.0.1 for this machine, a bridge address for a container. */
  client: string;
}

export interface ListQuery {
  search?: string;
  source?: string;
  project?: string;
  errors_only?: boolean;
  limit?: number;
  offset?: number;
}

export const getStatus = () => invoke<TracesStatus>("llm_traces_status");
export const start = (port: number) => invoke<TracesStatus>("llm_traces_start", { port });
export const stop = () => invoke<TracesStatus>("llm_traces_stop");
export const setOptions = (startWithApp: boolean, maxTraces: number, docker: boolean) =>
  invoke<TracesStatus>("llm_traces_set_options", { startWithApp, maxTraces, docker });

export const listTraces = (query: ListQuery) => invoke<TraceList>("llm_traces_list", { query });
export const getTrace = (id: string) => invoke<TraceDetail | null>("llm_traces_get", { id });
export const deleteTraces = (ids: string[]) => invoke<number>("llm_traces_delete", { ids });
export const clearTraces = () => invoke<void>("llm_traces_clear");
export const recentRequests = () => invoke<RequestLog[]>("llm_traces_recent");

/** Fired (at most every ¾ s) after the collector stores something. */
export const onIngested = (handler: () => void) => listen("llm-traces://ingested", () => handler());

// The nginx/SSL module's free-port scanner, as the Mail Catcher uses it.
export const suggestPort = (from: number) => invoke<number | null>("nginx_ssl_suggest_port", { start: from });
