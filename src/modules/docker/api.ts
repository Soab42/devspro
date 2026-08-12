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
}

export const detectDocker = () => invoke<DockerDetection>("docker_detect");
export const listContainers = () => invoke<Container[]>("docker_list_containers");
export const startContainer = (id: string) => invoke<string>("docker_start", { id });
export const stopContainer = (id: string) => invoke<string>("docker_stop", { id });
export const restartContainer = (id: string) => invoke<string>("docker_restart", { id });
export const fetchLogs = (id: string, tail: number) => invoke<string>("docker_logs", { id, tail });
export const changePort = (id: string, containerPort: number, newHostPort: number) =>
  invoke<string>("docker_change_port", { id, containerPort, newHostPort });
