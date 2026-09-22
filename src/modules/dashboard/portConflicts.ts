/**
 * Port-conflict detection across everything DevsPro knows binds a host port:
 * listening processes, Docker containers (running or not) and nginx sites.
 *
 * Pure on purpose — the dashboard and the Docker page both feed it their
 * already-fetched resources, so it never touches the backend and is fully
 * unit-tested. The interesting part is not "same number twice" but which
 * pairings are *not* conflicts: a running container's own docker-proxy shows
 * up as a listening process, and nginx listens on 443 on behalf of every site.
 * Reporting those would drown the real problems in noise.
 */

export type Proto = "tcp" | "udp";

export interface ListeningInput {
  port: number;
  pid: number | null;
  /** Process name as `ss` reports it (e.g. "nginx", "docker-proxy", "node"). */
  process: string | null;
}

export interface ContainerInput {
  name: string;
  /** Docker's `Ports` column, e.g. "0.0.0.0:8080->80/tcp, :::8080->80/tcp". */
  ports: string;
  /** Docker state: "running", "exited", "created", ... */
  state: string;
}

export interface SiteInput {
  name: string;
  /** Host ports the site's vhost `listen`s on. Empty for a disabled site. */
  ports: number[];
}

export interface PortClaimsInput {
  listening: ListeningInput[];
  containers: ContainerInput[];
  sites: SiteInput[];
}

export type ClaimantKind = "process" | "container" | "site";

export interface Claimant {
  kind: ClaimantKind;
  /** Process name, container name or site name. */
  label: string;
  /** "pid 1234", "running" / "exited", or undefined for a site. */
  detail?: string;
  /** Only set for containers; what "would this fail to start?" hinges on. */
  running?: boolean;
}

export interface PortConflict {
  port: number;
  proto: Proto;
  claimants: Claimant[];
}

interface HostPort {
  port: number;
  proto: Proto;
}

/**
 * Every enabled site's vhost renders `listen 80;` and `listen 443 ssl;` (see
 * src-tauri/src/modules/nginx_ssl/nginx/render.rs). The backend port of a
 * port-proxy site is an upstream nginx *connects* to, not something the site
 * binds, so it is deliberately not included.
 */
export const SITE_LISTEN_PORTS: readonly number[] = [80, 443];

/**
 * Parses Docker's `Ports` string into the host ports it publishes.
 *
 * Handles the ipv4 + ipv6 duplicate lines Docker emits for one `-p` flag
 * ("0.0.0.0:8080->80/tcp, :::8080->80/tcp" is a single host port), the
 * `[::]:8080` spelling newer daemons use, udp, and ranges
 * ("0.0.0.0:8000-8005->8000-8005/tcp" expands to six ports). Unpublished
 * exposed ports ("80/tcp" with no arrow) bind nothing on the host and are
 * skipped. Results are deduped and sorted so callers can compare them.
 */
export function parseHostPorts(ports: string): HostPort[] {
  const seen = new Set<string>();
  const out: HostPort[] = [];
  for (const raw of ports.split(",")) {
    const match = raw.trim().match(/:(\d+)(?:-(\d+))?->[^/]+\/(tcp|udp|sctp)$/i);
    if (!match) continue;
    const proto = match[3].toLowerCase();
    if (proto !== "tcp" && proto !== "udp") continue;
    const start = Number(match[1]);
    const end = match[2] ? Number(match[2]) : start;
    if (end < start || end - start > 65535) continue;
    for (let port = start; port <= end; port++) {
      const key = `${port}/${proto}`;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push({ port, proto });
    }
  }
  return out.sort((a, b) => a.port - b.port || a.proto.localeCompare(b.proto));
}

const isRunning = (state: string) => state === "running";

/** Names `ss` reports for the processes that are the *same* owner as a site or container. */
const isNginx = (name: string | null) => (name ?? "").toLowerCase().startsWith("nginx");
const isDockerProxy = (name: string | null) => (name ?? "").toLowerCase() === "docker-proxy";

/**
 * Whether two claimants on the same port are actually in each other's way.
 *
 * The rules encode who owns the listening socket:
 * - two processes never conflict (the kernel already refused the second bind;
 *   two entries are the ipv4/ipv6 halves of one listener);
 * - two sites never conflict (nginx dispatches on server_name);
 * - a running container's socket IS the docker-proxy process, and a site's
 *   socket IS the nginx process, so those pairs are the same owner;
 * - everything else — a stopped container against anything, a running
 *   container against a site, a non-nginx process against a site — is a real
 *   collision: something either already failed to bind or will when started.
 */
function conflicts(a: Claimant, b: Claimant): boolean {
  if (a.kind === b.kind) {
    return a.kind === "container";
  }
  const [proc, other] = a.kind === "process" ? [a, b] : b.kind === "process" ? [b, a] : [null, null];
  if (!proc || !other) {
    // container vs site
    return true;
  }
  if (other.kind === "container") {
    // A stopped container's port held by anything at all means it cannot start.
    if (!other.running) return true;
    // Running container: its own docker-proxy, or (host networking / rootless)
    // whatever process the daemon spawned, already holds the socket. We cannot
    // distinguish "a stranger holds it" from "the container holds it" — the
    // kernel would have refused the second bind — so treat the pair as one owner.
    return false;
  }
  // site vs process
  return !isNginx(proc.label);
}

/**
 * Groups every claim by host port and keeps the ports where at least one pair
 * of claimants collides. All claimants on such a port are reported, not only
 * the colliding pair: seeing "nginx (pid 12) · site app.test · container web
 * (exited)" is what tells you *which* one to move.
 */
export function findPortConflicts(input: PortClaimsInput): PortConflict[] {
  const byKey = new Map<string, { port: number; proto: Proto; claimants: Claimant[] }>();

  const add = (port: number, proto: Proto, claimant: Claimant) => {
    const key = `${port}/${proto}`;
    const slot = byKey.get(key) ?? { port, proto, claimants: [] };
    slot.claimants.push(claimant);
    byKey.set(key, slot);
  };

  // ss lists the ipv4 and ipv6 sockets of one listener separately; one process
  // per port is enough to name it.
  const seenProc = new Set<string>();
  for (const l of input.listening) {
    const key = `${l.port}:${l.pid ?? l.process ?? "?"}`;
    if (seenProc.has(key)) continue;
    seenProc.add(key);
    const claimant: Claimant = {
      kind: "process",
      label: l.process ?? "unknown process",
      detail: l.pid !== null ? `pid ${l.pid}` : undefined,
    };
    add(l.port, "tcp", claimant);
  }

  for (const c of input.containers) {
    const running = isRunning(c.state);
    for (const { port, proto } of parseHostPorts(c.ports)) {
      add(port, proto, { kind: "container", label: c.name, detail: c.state, running });
    }
  }

  for (const s of input.sites) {
    for (const port of new Set(s.ports)) {
      add(port, "tcp", { kind: "site", label: s.name });
    }
  }

  const out: PortConflict[] = [];
  for (const { port, proto, claimants } of byKey.values()) {
    if (claimants.length < 2) continue;
    let collides = false;
    for (let i = 0; i < claimants.length && !collides; i++) {
      for (let j = i + 1; j < claimants.length; j++) {
        if (conflicts(claimants[i], claimants[j])) {
          collides = true;
          break;
        }
      }
    }
    if (collides) out.push({ port, proto, claimants });
  }
  return out.sort((a, b) => a.port - b.port || a.proto.localeCompare(b.proto));
}

export interface StartCollision {
  container: string;
  port: number;
  /** Who holds the port: "node (pid 42)" or "container other-web" for a docker-proxy. */
  holder: string;
}

/**
 * The Docker-page question: "if I start these containers now, which of their
 * host ports is a process already sitting on?" Running targets are skipped —
 * their ports are their own. When the holder is a docker-proxy and `all`
 * includes the running container it fronts, that container is named instead
 * of the proxy, because "docker-proxy (pid 999)" tells nobody what to stop.
 */
export function startCollisions(
  targets: ContainerInput[],
  listening: ListeningInput[],
  all: ContainerInput[] = [],
): StartCollision[] {
  const holders = new Map<number, ListeningInput>();
  for (const l of listening) if (!holders.has(l.port)) holders.set(l.port, l);

  const runningOwner = new Map<number, string>();
  for (const c of all) {
    if (!isRunning(c.state)) continue;
    for (const { port, proto } of parseHostPorts(c.ports)) {
      if (proto === "tcp" && !runningOwner.has(port)) runningOwner.set(port, c.name);
    }
  }

  const out: StartCollision[] = [];
  for (const target of targets) {
    if (isRunning(target.state)) continue;
    for (const { port, proto } of parseHostPorts(target.ports)) {
      if (proto !== "tcp") continue;
      const holder = holders.get(port);
      if (!holder) continue;
      const owner = isDockerProxy(holder.process) ? runningOwner.get(port) : undefined;
      const process = holder.process ?? "an unknown process";
      out.push({
        container: target.name,
        port,
        holder: owner
          ? `container "${owner}"`
          : holder.pid !== null
            ? `${process} (pid ${holder.pid})`
            : process,
      });
    }
  }
  return out;
}

/** One line per collision, ready for a warning Callout or confirm body. */
export function describeStartCollisions(collisions: StartCollision[]): string {
  return collisions
    .map((c) => `port ${c.port} for "${c.container}" is already used by ${c.holder}`)
    .join("; ");
}
