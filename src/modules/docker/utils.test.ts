import { describe, expect, it } from "vitest";
import { Container } from "./api";
import { groupContainers, parsePortMappings, STANDALONE, summarizeOutcomes } from "./utils";

function container(partial: Partial<Container> & { id: string }): Container {
  return {
    name: partial.id,
    image: "img",
    state: "running",
    status: "Up 2 hours",
    ports: "",
    compose_project: null,
    compose_service: null,
    ...partial,
  };
}

describe("groupContainers", () => {
  it("groups by compose project", () => {
    const groups = groupContainers([
      container({ id: "a", compose_project: "databases" }),
      container({ id: "b", compose_project: "databases" }),
      container({ id: "c", compose_project: "mail" }),
    ]);
    expect(groups.map((g) => g.key)).toEqual(["databases", "mail"]);
    expect(groups[0].containers).toHaveLength(2);
    expect(groups[0].isCompose).toBe(true);
  });

  it("collects every ungrouped container under one Standalone group", () => {
    const groups = groupContainers([
      container({ id: "a" }),
      container({ id: "b" }),
    ]);
    expect(groups).toHaveLength(1);
    expect(groups[0].key).toBe(STANDALONE);
    expect(groups[0].label).toBe("Standalone");
    expect(groups[0].isCompose).toBe(false);
  });

  it("pins Standalone last regardless of name ordering", () => {
    const groups = groupContainers([
      container({ id: "a" }),
      container({ id: "b", compose_project: "zzz" }),
      container({ id: "c", compose_project: "aaa" }),
    ]);
    expect(groups.map((g) => g.key)).toEqual(["aaa", "zzz", STANDALONE]);
  });

  it("counts only running containers per group", () => {
    const groups = groupContainers([
      container({ id: "a", compose_project: "s", state: "running" }),
      container({ id: "b", compose_project: "s", state: "exited" }),
    ]);
    expect(groups[0].runningCount).toBe(1);
    expect(groups[0].containers).toHaveLength(2);
  });

  it("returns no groups for no containers", () => {
    expect(groupContainers([])).toEqual([]);
  });
});

describe("summarizeOutcomes", () => {
  const ok = (id: string) => ({ id, success: true, message: "ok" });
  const bad = (id: string, message: string) => ({ id, success: false, message });

  it("reports a clean run without mentioning failures", () => {
    const result = summarizeOutcomes("Stopped", [ok("a"), ok("b")]);
    expect(result.failed).toBe(false);
    expect(result.message).toContain("2");
  });

  it("reports total failure with the underlying reason", () => {
    const result = summarizeOutcomes("Stopped", [bad("a", "permission denied")]);
    expect(result.failed).toBe(true);
    expect(result.message).toContain("permission denied");
  });

  it("distinguishes a partial run from both success and total failure", () => {
    const result = summarizeOutcomes("Stopped", [ok("a"), ok("b"), bad("c", "no such container")]);
    expect(result.failed).toBe(true);
    expect(result.message).toContain("2 of 3");
    expect(result.message).toContain("no such container");
  });
});

describe("parsePortMappings", () => {
  it("parses a single published mapping", () => {
    expect(parsePortMappings("0.0.0.0:6379->6379/tcp")).toEqual([
      { hostPort: 6379, containerPort: 6379, proto: "tcp" },
    ]);
  });

  it("dedupes the ipv4/ipv6 pair docker reports for the same mapping", () => {
    const mappings = parsePortMappings("0.0.0.0:6379->6379/tcp, [::]:6379->6379/tcp");
    expect(mappings).toEqual([{ hostPort: 6379, containerPort: 6379, proto: "tcp" }]);
  });

  it("skips unpublished exposed ports with no ->", () => {
    expect(parsePortMappings("6379/tcp")).toEqual([]);
  });

  it("returns multiple distinct mappings", () => {
    const mappings = parsePortMappings("0.0.0.0:3306->3306/tcp, 0.0.0.0:33060->33060/tcp");
    expect(mappings).toEqual([
      { hostPort: 3306, containerPort: 3306, proto: "tcp" },
      { hostPort: 33060, containerPort: 33060, proto: "tcp" },
    ]);
  });

  it("returns an empty list for an empty string", () => {
    expect(parsePortMappings("")).toEqual([]);
  });
});
