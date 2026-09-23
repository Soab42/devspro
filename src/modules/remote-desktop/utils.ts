import type { Protocol, RdpExitEvent, RemoteDesktop } from "./api";

export const DEFAULT_PORT: Record<Protocol, number> = { vnc: 5900, rdp: 3389 };

export const PROTOCOL_LABEL: Record<Protocol, string> = { vnc: "VNC", rdp: "RDP" };

/** "alice@pc.local:3390" — the user only when set, the port only when it isn't the default. */
export function desktopLabel(d: Pick<RemoteDesktop, "protocol" | "host" | "port" | "username">): string {
  const user = d.username ? `${d.username}@` : "";
  const port = d.port === DEFAULT_PORT[d.protocol] ? "" : `:${d.port}`;
  return `${user}${d.host}${port}`;
}

/**
 * When the protocol changes, a port the user has not touched follows it —
 * 5900 becomes 3389 — but a port they typed stays put.
 */
export function portForProtocol(current: string, from: Protocol, to: Protocol): string {
  return current === String(DEFAULT_PORT[from]) || current.trim() === "" ? String(DEFAULT_PORT[to]) : current;
}

/** Text a connection form can be rejected with before a round trip to Rust. */
export function validateDesktopForm(input: {
  name: string;
  protocol: Protocol;
  host: string;
  port: string;
  username: string;
}): string | null {
  if (!input.name.trim()) return "Name is required.";
  if (!input.host.trim()) return "Host is required.";
  if (/\s/.test(input.host.trim())) return "Host cannot contain spaces.";
  const port = Number(input.port);
  if (!Number.isInteger(port) || port < 1 || port > 65535) return "Port must be between 1 and 65535.";
  if (input.protocol === "rdp" && !input.username.trim()) return "A username is required for RDP.";
  return null;
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * One line for the notice shown when an RDP client exits. A clean exit is
 * just "closed"; anything else surfaces the client's last words, which for
 * FreeRDP is where "authentication failed" or "connection refused" lands.
 */
export function describeRdpExit(e: Pick<RdpExitEvent, "client" | "code" | "output">): {
  tone: "success" | "danger";
  text: string;
} {
  if (e.code === 0) return { tone: "success", text: `${e.client} closed.` };
  const why = e.output.trim().split("\n").pop()?.trim();
  const code = e.code === null ? "was terminated" : `exited with code ${e.code}`;
  return { tone: "danger", text: why ? `${e.client} ${code}: ${why}` : `${e.client} ${code}.` };
}

/** Distro package hints for when no RDP client is installed. */
export const RDP_INSTALL_HINTS: { label: string; command: string }[] = [
  { label: "Debian / Ubuntu", command: "sudo apt install freerdp3-x11" },
  { label: "Fedora", command: "sudo dnf install freerdp" },
  { label: "Arch", command: "sudo pacman -S freerdp" },
];
