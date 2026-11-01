import { invoke } from "@tauri-apps/api/core";

export interface DockerDetection {
  found: boolean;
  daemon_reachable: boolean;
  version: string | null;
  error: string | null;
}

export interface Container {
  id: string;
  name: string;
  image: string;
  state: string;
  status: string;
  ports: string;
  /** `com.docker.compose.project`, or null for a plain `docker run` container. */
  compose_project: string | null;
  compose_service: string | null;
}

export interface ActionOutcome {
  id: string;
  success: boolean;
  message: string;
}

export type BulkAction = "start" | "stop" | "restart";

/**
 * Whole-stack verbs. `up` re-reads the compose file and recreates only what
 * changed — it is how an edit to the file gets applied, where `restart` would
 * bounce the containers with their old definition still in force.
 */
export type ComposeAction = "up" | "start" | "stop" | "restart" | "pull" | "down";

export const detectDocker = () => invoke<DockerDetection>("docker_detect");
/** Asks Docker Desktop / the docker service to start; returns before it's up. */
export const startDockerEngine = () => invoke<string>("docker_start_engine");
export const listContainers = () => invoke<Container[]>("docker_list_containers");
export const startContainer = (id: string) => invoke<string>("docker_start", { id });
export const stopContainer = (id: string) => invoke<string>("docker_stop", { id });
export const restartContainer = (id: string) => invoke<string>("docker_restart", { id });
export const fetchLogs = (id: string, tail: number) => invoke<string>("docker_logs", { id, tail });
export const changePort = (id: string, containerPort: number, newHostPort: number) =>
  invoke<string>("docker_change_port", { id, containerPort, newHostPort });

export const bulkAction = (action: BulkAction, ids: string[]) =>
  invoke<ActionOutcome[]>("docker_bulk_action", { action, ids });
export const composeProjectAction = (project: string, action: ComposeAction) =>
  invoke<string>("docker_compose_project_action", { project, action });
export const composeUpFile = (path: string) => invoke<string>("docker_compose_up_file", { path });
export const composeValidate = (path: string) =>
  invoke<string>("docker_compose_validate", { path });

/**
 * A compose file's text, plus what to narrow the apply to.
 *
 * `service` is set only when the editor was opened from a single container.
 * Both entry points hand back the *same whole file* — a compose project is one
 * file, and a service's config is a block inside it — so the difference between
 * "edit this container" and "edit this stack" is the scope of the apply, not the
 * scope of the file.
 */
export interface ComposeSource {
  path: string;
  contents: string;
  project: string | null;
  service: string | null;
}

export const readContainerCompose = (id: string) =>
  invoke<ComposeSource>("docker_read_container_compose", { id });
export const readProjectCompose = (project: string) =>
  invoke<ComposeSource>("docker_read_project_compose", { project });

/**
 * Validates the contents, writes them, then `up -d` — scoped to `service` when
 * given, the whole stack otherwise. Validation happens before the write, so a
 * rejected edit leaves the file on disk untouched.
 */
export const writeCompose = (
  path: string,
  contents: string,
  project: string | null,
  service: string | null,
) => invoke<string>("docker_write_compose", { path, contents, project, service });
