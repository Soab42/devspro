import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Protocol = "vnc" | "rdp";

export interface RemoteDesktop {
  id: string;
  name: string;
  protocol: Protocol;
  host: string;
  port: number;
  username: string;
  domain: string;
  view_only: boolean;
  scale: boolean;
  fullscreen: boolean;
  created_at: string;
}

export interface DesktopInput {
  name: string;
  protocol: Protocol;
  host: string;
  port: number;
  username: string;
  domain: string;
  view_only: boolean;
  scale: boolean;
  fullscreen: boolean;
}

/* ---- saved connections ---- */

export const listDesktops = () => invoke<RemoteDesktop[]>("remote_desktop_list");

export const saveDesktop = (id: string | null, input: DesktopInput) =>
  invoke<RemoteDesktop>("remote_desktop_save", { id, input });

export const deleteDesktop = (id: string) => invoke<void>("remote_desktop_delete", { id });

/* ---- vnc ---- */

/** A loopback WebSocket that the Rust side pipes to the VNC server. */
export interface VncBridge {
  session: string;
  url: string;
}

export const openVncBridge = (id: string) => invoke<VncBridge>("remote_desktop_vnc_open", { id });

export const closeVncBridge = (session: string) =>
  invoke<void>("remote_desktop_vnc_close", { session });

export interface VncBridgeEvent {
  session: string;
  kind: "connected" | "error" | "closed";
  message: string;
}

export const onVncBridge = (handler: (e: VncBridgeEvent) => void) =>
  listen<VncBridgeEvent>("remote-desktop:vnc", (e) => handler(e.payload));

/* ---- rdp ---- */

export interface RdpClient {
  id: string;
  label: string;
  path: string;
  /** How the password reaches this client: its command line, or Windows Credential Manager. */
  password_note: string;
}

export interface RdpLaunch {
  pid: number;
  client: string;
}

export interface RdpExitEvent {
  pid: number;
  client: string;
  code: number | null;
  output: string;
}

export const listRdpClients = () => invoke<RdpClient[]>("remote_desktop_rdp_clients");

/** The password is used for this launch only and never stored. */
export const launchRdp = (id: string, client: string, password: string) =>
  invoke<RdpLaunch>("remote_desktop_rdp_launch", { id, client, password: password || null });

export const onRdpExit = (handler: (e: RdpExitEvent) => void) =>
  listen<RdpExitEvent>("remote-desktop:rdp-exit", (e) => handler(e.payload));
