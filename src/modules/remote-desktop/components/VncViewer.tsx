import { useCallback, useEffect, useRef, useState } from "react";
import RFB from "@novnc/novnc";
import { Button, Callout, Field, Form, FormRow, IconButton, Modal, Stack, StatusPill, TextInput } from "../../../ui";
import { ClipboardPaste, Expand, Eye, EyeOff, KeyRound, Keyboard, Maximize, Minimize, RefreshCw, Shrink, Unplug } from "../../../ui/icons";
import { RemoteDesktop, VncBridge, onVncBridge } from "../api";
import { desktopLabel } from "../utils";

type Phase = "connecting" | "connected" | "disconnected";

interface CredentialAsk {
  username: boolean;
  error: string | null;
}

/**
 * The in-app VNC viewer: noVNC drawing into a div, fed through the loopback
 * bridge the Rust side opened for this session.
 *
 * Everything RFB-level — auth, encodings, input — is noVNC's; this component
 * owns the lifecycle (one RFB per bridge URL, torn down on unmount), the
 * toolbar, and the two dialogs noVNC can't draw itself: the password prompt
 * and the reason for a disconnect. The bridge reports TCP failures over an
 * event because from noVNC's side they all look like "the socket closed".
 */
export default function VncViewer({
  desktop,
  bridge,
  onDisconnect,
}: {
  desktop: RemoteDesktop;
  bridge: VncBridge;
  onDisconnect: () => void;
}) {
  const screenRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<HTMLDivElement>(null);
  const rfbRef = useRef<RFB | null>(null);
  const [phase, setPhase] = useState<Phase>("connecting");
  const [reason, setReason] = useState<string | null>(null);
  const [desktopName, setDesktopName] = useState<string | null>(null);
  const [ask, setAsk] = useState<CredentialAsk | null>(null);
  const [scale, setScale] = useState(desktop.scale);
  const [viewOnly, setViewOnly] = useState(desktop.view_only);
  const [fullscreen, setFullscreen] = useState(false);
  const [attempt, setAttempt] = useState(0);

  // Bridge-side failures (refused, timed out) arrive here; noVNC only sees a
  // closed socket. Kept in a ref so the disconnect handler can read the
  // latest without re-creating the RFB.
  const bridgeError = useRef<string | null>(null);
  useEffect(() => {
    const unlisten = onVncBridge((e) => {
      if (e.session !== bridge.session) return;
      if (e.kind === "error") bridgeError.current = e.message;
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [bridge.session]);

  useEffect(() => {
    const target = screenRef.current;
    if (!target) return;
    bridgeError.current = null;
    setPhase("connecting");
    setReason(null);
    setAsk(null);

    const rfb = new RFB(target, bridge.url, {
      credentials: desktop.username ? { username: desktop.username } : undefined,
    });
    rfb.background = "transparent";
    rfb.scaleViewport = scale;
    rfb.viewOnly = viewOnly;
    rfb.resizeSession = false;
    rfbRef.current = rfb;

    const onConnect = () => setPhase("connected");
    const onDisconnected = (e: Event) => {
      const clean = (e as CustomEvent<{ clean: boolean }>).detail?.clean;
      setPhase("disconnected");
      setReason((current) => current ?? bridgeError.current ?? (clean ? null : "The connection was lost."));
    };
    const onCredentials = (e: Event) => {
      const types = (e as CustomEvent<{ types: string[] }>).detail?.types ?? [];
      setAsk({ username: types.includes("username"), error: null });
    };
    const onSecurityFailure = (e: Event) => {
      const detail = (e as CustomEvent<{ reason?: string }>).detail;
      setReason(detail?.reason ? `Authentication failed: ${detail.reason}` : "Authentication failed.");
    };
    const onDesktopName = (e: Event) => setDesktopName((e as CustomEvent<{ name: string }>).detail?.name ?? null);

    rfb.addEventListener("connect", onConnect);
    rfb.addEventListener("disconnect", onDisconnected);
    rfb.addEventListener("credentialsrequired", onCredentials);
    rfb.addEventListener("securityfailure", onSecurityFailure);
    rfb.addEventListener("desktopname", onDesktopName);

    return () => {
      rfbRef.current = null;
      rfb.removeEventListener("connect", onConnect);
      rfb.removeEventListener("disconnect", onDisconnected);
      rfb.removeEventListener("credentialsrequired", onCredentials);
      rfb.removeEventListener("securityfailure", onSecurityFailure);
      rfb.removeEventListener("desktopname", onDesktopName);
      rfb.disconnect();
    };
    // A new attempt (Reconnect) or a new bridge rebuilds the RFB; toolbar
    // toggles are pushed to the live instance below instead.
  }, [bridge.url, attempt]);

  useEffect(() => {
    if (rfbRef.current) rfbRef.current.scaleViewport = scale;
  }, [scale]);
  useEffect(() => {
    if (rfbRef.current) rfbRef.current.viewOnly = viewOnly;
  }, [viewOnly]);

  useEffect(() => {
    const sync = () => setFullscreen(document.fullscreenElement === frameRef.current);
    document.addEventListener("fullscreenchange", sync);
    return () => document.removeEventListener("fullscreenchange", sync);
  }, []);

  const toggleFullscreen = useCallback(() => {
    if (document.fullscreenElement) void document.exitFullscreen();
    else void frameRef.current?.requestFullscreen();
  }, []);

  async function pasteClipboard() {
    try {
      const text = await navigator.clipboard.readText();
      if (text) rfbRef.current?.clipboardPasteFrom(text);
    } catch {
      /* clipboard access denied — nothing to paste */
    }
  }

  const connected = phase === "connected";
  const status =
    phase === "connected"
      ? { variant: "ok" as const, label: desktopName ?? "Connected" }
      : phase === "connecting"
        ? { variant: "pending" as const, label: "Connecting" }
        : { variant: "bad" as const, label: "Disconnected" };

  return (
    <div className={fullscreen ? "rd-viewer rd-viewer-fullscreen" : "rd-viewer"} ref={frameRef}>
      <div className="rd-toolbar">
        <div className="rd-toolbar-title">
          <span className="rd-toolbar-name">{desktop.name}</span>
          <span className="rd-toolbar-address">{desktopLabel(desktop)}</span>
          <StatusPill size="sm" variant={status.variant} label={status.label} />
        </div>
        <div className="rd-toolbar-actions">
          <IconButton
            icon={scale ? Expand : Shrink}
            label={scale ? "Show at actual size" : "Scale to fit"}
            onClick={() => setScale((v) => !v)}
          />
          <IconButton
            icon={viewOnly ? EyeOff : Eye}
            label={viewOnly ? "View only — click to allow input" : "Input enabled — click for view only"}
            onClick={() => setViewOnly((v) => !v)}
          />
          <IconButton icon={Keyboard} label="Send Ctrl+Alt+Del" disabled={!connected || viewOnly} onClick={() => rfbRef.current?.sendCtrlAltDel()} />
          <IconButton icon={ClipboardPaste} label="Paste clipboard" disabled={!connected || viewOnly} onClick={pasteClipboard} />
          <IconButton icon={fullscreen ? Minimize : Maximize} label={fullscreen ? "Exit fullscreen" : "Fullscreen"} onClick={toggleFullscreen} />
          <IconButton icon={Unplug} label="Disconnect" onClick={onDisconnect} />
        </div>
      </div>

      {phase === "disconnected" && (
        <div className="rd-notice">
          <Callout
            tone={reason ? "danger" : "info"}
            actions={
              <>
                <Button size="sm" variant="secondary" icon={RefreshCw} onClick={() => setAttempt((n) => n + 1)}>
                  Reconnect
                </Button>
                <Button size="sm" variant="secondary" onClick={onDisconnect}>
                  Close
                </Button>
              </>
            }
          >
            {reason ?? "The session ended."}
          </Callout>
        </div>
      )}

      <div className="rd-screen" ref={screenRef} />

      {ask && (
        <CredentialsModal
          desktop={desktop}
          askUsername={ask.username}
          error={ask.error}
          onSubmit={(username, password) => {
            setAsk(null);
            rfbRef.current?.sendCredentials(ask.username ? { username, password } : { password });
          }}
          onCancel={() => {
            setAsk(null);
            onDisconnect();
          }}
        />
      )}
    </div>
  );
}

function CredentialsModal({
  desktop,
  askUsername,
  error,
  onSubmit,
  onCancel,
}: {
  desktop: RemoteDesktop;
  askUsername: boolean;
  error: string | null;
  onSubmit: (username: string, password: string) => void;
  onCancel: () => void;
}) {
  const [username, setUsername] = useState(desktop.username);
  const [password, setPassword] = useState("");
  return (
    <Modal
      size="sm"
      title="Password required"
      description={`${desktopLabel(desktop)} asks for a password. It is used for this connection only.`}
      icon={KeyRound}
      onClose={onCancel}
    >
      <Form onSubmit={() => onSubmit(username, password)}>
        <Stack gap={5}>
          {askUsername && (
            <Field label="Username" htmlFor="rd-vnc-user">
              <TextInput id="rd-vnc-user" mono value={username} onChange={(e) => setUsername(e.target.value)} autoFocus />
            </Field>
          )}
          <Field label="Password" htmlFor="rd-vnc-password" error={error ?? undefined}>
            <TextInput
              id="rd-vnc-password"
              type="password"
              autoComplete="off"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoFocus={!askUsername}
            />
          </Field>
          <FormRow className="form-actions">
            <Button variant="secondary" onClick={onCancel}>
              Cancel
            </Button>
            <Button type="submit" variant="primary">
              Connect
            </Button>
          </FormRow>
        </Stack>
      </Form>
    </Modal>
  );
}
