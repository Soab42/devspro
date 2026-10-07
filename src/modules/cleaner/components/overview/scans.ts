import {
  cleanCaches,
  cleanDocker,
  cleanProjects,
  cleanSystem,
  scanCaches,
  scanDocker,
  scanProjects,
  scanSystem,
} from "../../api";
import type { CleanOutcome, Finding, ProjectJunk } from "../../api";

export type Source = "caches" | "system" | "docker" | "projects";

export const SOURCES: { id: Source; label: string }[] = [
  { id: "caches", label: "Caches" },
  { id: "system", label: "System" },
  { id: "docker", label: "Docker" },
  { id: "projects", label: "Project junk" },
];

export interface SourceResult {
  findings: Finding[];
  /** Why this source could not be scanned. */
  error: string | null;
  /** Not an error: e.g. Docker isn't installed. */
  note: string | null;
}

export const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** A project artifact as a finding, so it can share the totals and the review modal. */
export function projectFinding(p: ProjectJunk): Finding {
  const idle = p.idle_days === null ? "no recorded activity" : `idle for ${p.idle_days} days`;
  return {
    id: p.artifact_path,
    tab: "projects",
    group: p.project_name,
    label: `${p.project_name}: ${p.artifact_kind}`,
    reason: `Rebuildable; project ${idle}.`,
    method: "Delete folder",
    paths: [p.artifact_path],
    bytes: p.bytes,
    risk: "review",
    suggested: p.suggested,
    blocked_by: null,
    needs_confirm_text: null,
  };
}

/** Runs one source's scan. Never throws: a failure becomes that source's error. */
export async function runScan(source: Source, scanId: string): Promise<SourceResult> {
  try {
    switch (source) {
      case "caches":
        return { findings: await scanCaches(scanId), error: null, note: null };
      case "system":
        return { findings: await scanSystem(scanId), error: null, note: null };
      case "docker": {
        const d = await scanDocker(scanId);
        return { findings: d.available ? d.findings : [], error: null, note: d.available ? null : (d.message ?? "Docker is not available.") };
      }
      case "projects":
        return { findings: (await scanProjects(scanId)).map(projectFinding), error: null, note: null };
    }
  } catch (e) {
    return { findings: [], error: errorText(e), note: null };
  }
}

/**
 * Sends each finding to the clean command for its tab. A command that fails
 * outright marks only its own findings as failed.
 */
export async function cleanFindings(items: Finding[], confirmed: string[]): Promise<CleanOutcome[]> {
  const byTab = new Map<string, Finding[]>();
  for (const f of items) byTab.set(f.tab, [...(byTab.get(f.tab) ?? []), f]);

  const jobs = [...byTab.entries()].map(async ([tab, group]): Promise<CleanOutcome[]> => {
    const ids = group.map((f) => f.id);
    try {
      switch (tab) {
        case "caches":
          return await cleanCaches(ids);
        case "system":
          return await cleanSystem(ids);
        case "docker":
          return await cleanDocker(ids, confirmed.filter((id) => ids.includes(id)));
        case "projects":
          return await cleanProjects(ids);
        default:
          throw new Error(`No clean command for "${tab}"`);
      }
    } catch (e) {
      return ids.map((id) => ({ id, ok: false, message: errorText(e), bytes_freed: 0 }));
    }
  });
  return (await Promise.all(jobs)).flat();
}
