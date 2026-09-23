import type { PtyCommand } from "../ssh/api";
import type { AgentStatus } from "./api";

/**
 * Splits a line of extra arguments the way a shell would, minus expansion:
 * whitespace separates, single or double quotes group, backslash escapes the
 * next character outside single quotes. `--model "gpt 5"` -> ["--model", "gpt 5"].
 */
export function splitArgs(input: string): string[] {
  const out: string[] = [];
  let current = "";
  let quote: '"' | "'" | null = null;
  let has = false;
  for (let i = 0; i < input.length; i += 1) {
    const ch = input[i];
    if (quote) {
      if (ch === quote) quote = null;
      else if (ch === "\\" && quote === '"' && i + 1 < input.length) current += input[++i];
      else current += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
      has = true;
    } else if (ch === "\\" && i + 1 < input.length) {
      current += input[++i];
      has = true;
    } else if (/\s/.test(ch)) {
      if (has) out.push(current);
      current = "";
      has = false;
    } else {
      current += ch;
      has = true;
    }
  }
  if (has) out.push(current);
  return out;
}

/** The PTY command that runs an agent in a project, with any extra arguments after its fixed ones. */
export function launchCommand(agent: AgentStatus, cwd: string | null, extraArgs: string): PtyCommand {
  return { exec: { program: agent.command, args: [...agent.args, ...splitArgs(extraArgs)], cwd } };
}

/** The PTY command that runs an agent's install one-liner in the user's shell. */
export function installCommand(script: string): PtyCommand {
  return { exec: { program: "sh", args: ["-c", script], cwd: null } };
}

export function baseName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** "Claude Code · devspro" when a project is chosen, "Claude Code · ~" otherwise. */
export function sessionTitle(agentName: string, cwd: string | null): string {
  return `${agentName} · ${cwd ? baseName(cwd) : "~"}`;
}

export const MAX_RECENT_PROJECTS = 8;

/** A folder just used goes to the front; the list stays short and unique. */
export function rememberProject(recent: string[], path: string): string[] {
  return [path, ...recent.filter((p) => p !== path)].slice(0, MAX_RECENT_PROJECTS);
}

/** Installed agents first, then the rest, each group in catalog order. */
export function groupAgents(agents: AgentStatus[]): { installed: AgentStatus[]; missing: AgentStatus[] } {
  return {
    installed: agents.filter((a) => a.installed),
    missing: agents.filter((a) => !a.installed),
  };
}
