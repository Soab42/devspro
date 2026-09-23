import { describe, expect, it } from "vitest";
import type { ModuleDefinition } from "./ModuleRegistry";
import { Package } from "../ui/icons";
import { rankModules } from "./moduleSearch";

/** A bare-bones fixture — only the fields the ranker actually reads matter here. */
function mod(fields: Partial<ModuleDefinition> & Pick<ModuleDefinition, "id" | "label">): ModuleDefinition {
  return {
    icon: Package,
    group: "system",
    path: `/${fields.id}`,
    Component: () => null,
    ...fields,
  };
}

const docker = mod({
  id: "docker",
  label: "Docker",
  description: "Containers and compose stacks: start, stop, logs and ports.",
  keywords: ["containers", "compose", "images"],
});

const scheduled = mod({
  id: "scheduled",
  label: "Scheduled tasks",
  description: "Cron-style tasks that run on a schedule.",
  keywords: ["cron", "schedule", "timer", "artisan"],
});

const http = mod({
  id: "http",
  label: "HTTP client",
  description: "Send requests to your local sites and inspect the response.",
  keywords: ["rest", "api", "request", "postman", "curl"],
});

// Its description mentions "docker" but its label and keywords don't — the
// foil for the label-prefix-outranks-description-match case.
const logs = mod({
  id: "logs",
  label: "Logs",
  description: "Tail application logs, including docker container output.",
  keywords: ["tail", "errors"],
});

const dashboard = mod({
  id: "dashboard",
  label: "Dashboard",
  description: "System health, resource usage and port conflicts at a glance.",
});

describe("rankModules", () => {
  it("returns candidates untouched, in registry order, for an empty or whitespace query", () => {
    const candidates = [logs, docker, scheduled];
    expect(rankModules("", candidates)).toEqual(candidates);
    expect(rankModules("   ", candidates)).toEqual(candidates);
  });

  it("ranks a label prefix match above a description-only match", () => {
    // "docker" is Docker's label (a prefix match); Logs only mentions it in
    // prose. The label match must win even though Logs is listed first.
    const ranked = rankModules("docker", [logs, docker]);
    expect(ranked.map((m) => m.id)).toEqual(["docker", "logs"]);
  });

  it("finds a module through a keyword synonym", () => {
    expect(rankModules("cron", [dashboard, scheduled, http]).map((m) => m.id)).toEqual([
      "scheduled",
    ]);
    expect(rankModules("postman", [dashboard, scheduled, http]).map((m) => m.id)).toEqual([
      "http",
    ]);
  });

  it("excludes candidates that match nowhere", () => {
    expect(rankModules("zzznotfound", [docker, scheduled, http])).toEqual([]);
  });

  it("matches a label by subsequence", () => {
    // "dkr" -> d-o-c-k-e-r: in order, not contiguous.
    expect(rankModules("dkr", [scheduled, docker, http]).map((m) => m.id)).toEqual(["docker"]);
  });

  it("requires every word in a multi-word query, in any order", () => {
    // "client http" matches Http's label with its words reversed.
    expect(rankModules("client http", [dashboard, docker, http]).map((m) => m.id)).toEqual([
      "http",
    ]);
    // "http zzz" has one word that matches nothing, so the candidate is dropped.
    expect(rankModules("http zzz", [http])).toEqual([]);
  });
});
