import { describe, expect, it } from "vitest";
import {
  describeStartCollisions,
  findPortConflicts,
  parseHostPorts,
  startCollisions,
} from "./portConflicts";

describe("parseHostPorts", () => {
  it("collapses the ipv4/ipv6 pair Docker emits for one publish", () => {
    expect(parseHostPorts("0.0.0.0:8080->80/tcp, :::8080->80/tcp")).toEqual([
      { port: 8080, proto: "tcp" },
    ]);
  });

  it("accepts the bracketed ipv6 spelling", () => {
    expect(parseHostPorts("0.0.0.0:6379->6379/tcp, [::]:6379->6379/tcp")).toEqual([
      { port: 6379, proto: "tcp" },
    ]);
  });

  it("keeps tcp and udp on the same number apart", () => {
    expect(parseHostPorts("0.0.0.0:53->53/tcp, 0.0.0.0:53->53/udp")).toEqual([
      { port: 53, proto: "tcp" },
      { port: 53, proto: "udp" },
    ]);
  });

  it("expands host ranges", () => {
    expect(parseHostPorts("0.0.0.0:8000-8002->8000-8002/tcp").map((p) => p.port)).toEqual([
      8000, 8001, 8002,
    ]);
  });

  it("skips exposed-but-unpublished ports and empty strings", () => {
    expect(parseHostPorts("80/tcp, 443/tcp")).toEqual([]);
    expect(parseHostPorts("")).toEqual([]);
  });

  it("sorts by host port regardless of input order", () => {
    expect(parseHostPorts("0.0.0.0:9000->9000/tcp, 0.0.0.0:80->80/tcp").map((p) => p.port)).toEqual([
      80, 9000,
    ]);
  });
});

const proc = (port: number, process: string | null, pid: number | null = 1) => ({ port, pid, process });
const container = (name: string, ports: string, state = "exited") => ({ name, ports, state });
const site = (name: string, ports: number[] = [80, 443]) => ({ name, ports });

describe("findPortConflicts", () => {
  it("returns nothing when every port has one owner", () => {
    expect(
      findPortConflicts({
        listening: [proc(3000, "node")],
        containers: [container("db", "0.0.0.0:5432->5432/tcp")],
        sites: [site("app")],
      }),
    ).toEqual([]);
  });

  it("flags a stopped container whose port a process already holds", () => {
    const out = findPortConflicts({
      listening: [proc(8080, "node", 42)],
      containers: [container("web", "0.0.0.0:8080->80/tcp, :::8080->80/tcp")],
      sites: [],
    });
    expect(out).toHaveLength(1);
    expect(out[0].port).toBe(8080);
    expect(out[0].claimants.map((c) => c.label)).toEqual(["node", "web"]);
    expect(out[0].claimants[0].detail).toBe("pid 42");
  });

  it("does not report a running container against its own docker-proxy", () => {
    expect(
      findPortConflicts({
        listening: [proc(8080, "docker-proxy")],
        containers: [container("web", "0.0.0.0:8080->80/tcp", "running")],
        sites: [],
      }),
    ).toEqual([]);
  });

  it("flags two stopped containers declaring the same host port", () => {
    const out = findPortConflicts({
      listening: [],
      containers: [
        container("a-web", "0.0.0.0:8080->80/tcp"),
        container("b-web", "0.0.0.0:8080->80/tcp", "created"),
      ],
      sites: [],
    });
    expect(out.map((c) => c.port)).toEqual([8080]);
    expect(out[0].claimants.map((c) => c.label)).toEqual(["a-web", "b-web"]);
  });

  it("flags a stopped container against a running one", () => {
    const out = findPortConflicts({
      listening: [proc(5432, "docker-proxy")],
      containers: [
        container("pg-old", "0.0.0.0:5432->5432/tcp", "running"),
        container("pg-new", "0.0.0.0:5432->5432/tcp"),
      ],
      sites: [],
    });
    expect(out).toHaveLength(1);
    expect(out[0].claimants.filter((c) => c.kind === "container")).toHaveLength(2);
  });

  it("lets nginx and many sites share 80/443", () => {
    expect(
      findPortConflicts({
        listening: [proc(80, "nginx"), proc(443, "nginx")],
        containers: [],
        sites: [site("one"), site("two")],
      }),
    ).toEqual([]);
  });

  it("flags a site whose port is held by something other than nginx", () => {
    const out = findPortConflicts({
      listening: [proc(80, "python3", 7)],
      containers: [],
      sites: [site("app")],
    });
    expect(out.map((c) => c.port)).toEqual([80]);
    expect(out[0].claimants.map((c) => c.kind)).toEqual(["process", "site"]);
  });

  it("flags a container publishing a port sites listen on", () => {
    const out = findPortConflicts({
      listening: [],
      containers: [container("proxy", "0.0.0.0:443->443/tcp")],
      sites: [site("app")],
    });
    expect(out.map((c) => c.port)).toEqual([443]);
  });

  it("ignores duplicate ipv4/ipv6 process rows and never pits processes against each other", () => {
    expect(
      findPortConflicts({
        listening: [proc(22, "sshd", 9), proc(22, "sshd", 9)],
        containers: [],
        sites: [],
      }),
    ).toEqual([]);
  });

  it("keeps udp claims separate from tcp listeners", () => {
    expect(
      findPortConflicts({
        listening: [proc(53, "systemd-resolve")],
        containers: [container("dns", "0.0.0.0:53->53/udp")],
        sites: [],
      }),
    ).toEqual([]);
  });

  it("sorts conflicts by port", () => {
    const out = findPortConflicts({
      listening: [proc(9000, "php-fpm"), proc(3000, "node")],
      containers: [
        container("x", "0.0.0.0:9000->9000/tcp"),
        container("y", "0.0.0.0:3000->3000/tcp"),
      ],
      sites: [],
    });
    expect(out.map((c) => c.port)).toEqual([3000, 9000]);
  });
});

describe("startCollisions", () => {
  it("names the process holding a stopped target's port", () => {
    const out = startCollisions(
      [container("web", "0.0.0.0:8080->80/tcp")],
      [proc(8080, "node", 42)],
    );
    expect(out).toEqual([{ container: "web", port: 8080, holder: "node (pid 42)" }]);
    expect(describeStartCollisions(out)).toBe('port 8080 for "web" is already used by node (pid 42)');
  });

  it("skips running targets and free ports", () => {
    expect(
      startCollisions(
        [container("web", "0.0.0.0:8080->80/tcp", "running"), container("api", "0.0.0.0:9000->9000/tcp")],
        [proc(8080, "docker-proxy")],
      ),
    ).toEqual([]);
  });

  it("resolves a docker-proxy holder to the running container it fronts", () => {
    const all = [
      container("old", "0.0.0.0:5432->5432/tcp", "running"),
      container("new", "0.0.0.0:5432->5432/tcp"),
    ];
    const out = startCollisions([all[1]], [proc(5432, "docker-proxy", 99)], all);
    expect(out[0].holder).toBe('container "old"');
  });

  it("describes several collisions on one line", () => {
    const text = describeStartCollisions([
      { container: "a", port: 1, holder: "x" },
      { container: "b", port: 2, holder: "y" },
    ]);
    expect(text).toBe('port 1 for "a" is already used by x; port 2 for "b" is already used by y');
  });
});
