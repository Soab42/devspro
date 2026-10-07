import type { Finding, ProjectJunk } from "../../api";

export interface ProjectGroup {
  path: string;
  name: string;
  total: number;
  idleDays: number | null;
  artifacts: ProjectJunk[];
}

/** How each artifact comes back after it is deleted. */
const RECREATE: Record<string, string> = {
  node_modules: "npm install",
  ".next": "the next build or dev run",
  ".nuxt": "the next build or dev run",
  ".svelte-kit": "the next build or dev run",
  ".turbo": "the next build",
  ".parcel-cache": "the next build",
  vendor: "composer install",
  target: "cargo build",
  ".venv": "python -m venv, then pip install",
  venv: "python -m venv, then pip install",
  __pycache__: "running the code again",
  build: "gradle build",
  ".gradle": "gradle build",
};

export const recreateHint = (kind: string) => RECREATE[kind] ?? "the project's build";

/** By project, the largest first; artifacts largest first inside. */
export function groupProjects(rows: ProjectJunk[]): ProjectGroup[] {
  const groups = new Map<string, ProjectGroup>();
  for (const r of rows) {
    const g = groups.get(r.project_path) ?? {
      path: r.project_path,
      name: r.project_name,
      total: 0,
      idleDays: r.idle_days,
      artifacts: [],
    };
    g.total += r.bytes;
    g.artifacts.push(r);
    groups.set(r.project_path, g);
  }
  const out = [...groups.values()];
  for (const g of out) g.artifacts.sort((a, b) => b.bytes - a.bytes);
  return out.sort((a, b) => b.total - a.total || a.path.localeCompare(b.path));
}

export const suggestedPaths = (rows: ProjectJunk[]) => rows.filter((r) => r.suggested).map((r) => r.artifact_path);

/** What ReviewCleanModal shows for one artifact. The id is its path. */
export function toFinding(r: ProjectJunk): Finding {
  return {
    id: r.artifact_path,
    tab: "projects",
    group: r.project_name,
    label: `${r.project_name}/${r.artifact_kind}`,
    reason: `Can be regenerated with ${recreateHint(r.artifact_kind)}.`,
    method: `delete (recreate with ${recreateHint(r.artifact_kind)})`,
    paths: [r.artifact_path],
    bytes: r.bytes,
    risk: "safe",
    suggested: r.suggested,
    blocked_by: null,
    needs_confirm_text: null,
  };
}

export const idleLabel = (days: number | null) =>
  days === null ? "unknown" : days === 1 ? "1 day" : `${days} days`;
