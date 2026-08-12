import { describe, expect, it } from "vitest";
import { parsePortMappings } from "./utils";

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
