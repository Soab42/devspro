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
