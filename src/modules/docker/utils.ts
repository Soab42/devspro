import { ActionOutcome, Container } from "./api";

/** Containers with no compose project, grouped under one heading. */
export const STANDALONE = "__standalone__";

export interface ContainerGroup {
  /** Compose project name, or `STANDALONE`. */
  key: string;
  label: string;
  isCompose: boolean;
  containers: Container[];
  runningCount: number;
}

/**
 * Groups containers by compose project, the way Docker Desktop does — a stack is
 * the unit people actually think in, and acting on one container of a five-service
 * stack is usually not what was meant.
 *
 * Ungrouped containers are collected under a single "Standalone" group rather than
 * given a group each: they have no stack-level actions available, so a per-container
 * heading would be pure noise. Groups sort alphabetically with Standalone pinned
 * last, so the named stacks stay in a stable order as containers come and go.
 */
export function groupContainers(containers: Container[]): ContainerGroup[] {
  const byKey = new Map<string, Container[]>();
  for (const container of containers) {
    const key = container.compose_project ?? STANDALONE;
    const existing = byKey.get(key);
    if (existing) existing.push(container);
    else byKey.set(key, [container]);
  }

  return [...byKey.entries()]
    .map(([key, group]) => ({
      key,
      label: key === STANDALONE ? "Standalone" : key,
      isCompose: key !== STANDALONE,
      containers: group,
      runningCount: group.filter((c) => c.state === "running").length,
    }))
    .sort((a, b) => {
      if (a.key === STANDALONE) return 1;
      if (b.key === STANDALONE) return -1;
      return a.label.localeCompare(b.label);
    });
}

/**
 * Collapses a bulk run into one sentence. Partial success is the case worth
 * getting right: it is common (one container refuses to stop), and reporting it
 * as either "done" or "failed" would be a lie in opposite directions.
 */
export function summarizeOutcomes(action: string, outcomes: ActionOutcome[]): {
  message: string;
  failed: boolean;
} {
  const failures = outcomes.filter((o) => !o.success);
  if (failures.length === 0) {
    return { message: `${action} ${outcomes.length} container(s).`, failed: false };
  }
  if (failures.length === outcomes.length) {
    return { message: `Could not ${action.toLowerCase()} any container: ${failures[0].message}`, failed: true };
  }
  const succeeded = outcomes.length - failures.length;
  return {
    message: `${action} ${succeeded} of ${outcomes.length}. ${failures.length} failed: ${failures[0].message}`,
    failed: true,
  };
}

export interface PortMapping {
  hostPort: number;
  containerPort: number;
  proto: string;
}

// "0.0.0.0:6379->6379/tcp, [::]:6379->6379/tcp" -> one deduped mapping (ipv4/ipv6
// lines repeat the same host:container pair). Unpublished exposed ports (no "->")
// are skipped — there's nothing to open or remap for those.
export function parsePortMappings(ports: string): PortMapping[] {
  const seen = new Set<string>();
  const result: PortMapping[] = [];
  for (const raw of ports.split(",")) {
    const match = raw.trim().match(/:(\d+)->(\d+)\/(\w+)$/);
    if (!match) continue;
    const hostPort = Number(match[1]);
    const containerPort = Number(match[2]);
    const proto = match[3];
    const key = `${containerPort}/${proto}`;
    if (seen.has(key)) continue;
    seen.add(key);
    result.push({ hostPort, containerPort, proto });
  }
  return result;
}
