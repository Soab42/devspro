import { invoke } from "@tauri-apps/api/core";

/** A catalog agent or a custom one, with whether its command was found. */
export interface AgentStatus {
  id: string;
  name: string;
  vendor: string;
  description: string;
  command: string;
  /** Fixed arguments (custom agents only); the user's extra ones come after. */
  args: string[];
  /** A shell one-liner that installs it, or null when there is none to offer. */
  install: string | null;
  homepage: string | null;
  installed: boolean;
  /** Where the command resolved to, when it did. */
  path: string | null;
  custom: boolean;
  /** The program and leading arguments that run it; `args` and extra ones follow.
   *  Just `[command]` on Linux; on Windows the resolved path, or `cmd.exe /d /c call
   *  <path>` for an npm `.cmd` shim. */
  launch: string[];
  /** The program and leading arguments that run `install` as a script: `sh -c`, or
   *  PowerShell on Windows. */
  install_shell: string[];
}

export interface CustomAgentInput {
  name: string;
  command: string;
  args: string[];
  description: string;
}

export const listAgents = () => invoke<AgentStatus[]>("ai_agents_list");

export const saveCustomAgent = (id: string | null, input: CustomAgentInput) =>
  invoke<AgentStatus>("ai_custom_agent_save", { id, ...input });

export const deleteCustomAgent = (id: string) => invoke<void>("ai_custom_agent_delete", { id });
