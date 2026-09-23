import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import {
  Badge,
  Button,
  Callout,
  Card,
  EmptyState,
  Icon,
  Modal,
  Mono,
  Page,
  SideList,
  SplitView,
  Stack,
  statusOf,
  useResource,
} from "../../../ui";
import { Monitor, MonitorUp, Pencil, Plug, Plus, RefreshCw, ScreenShare, Trash2 } from "../../../ui/icons";
import { RemoteDesktop, VncBridge, closeVncBridge, deleteDesktop, listDesktops, onRdpExit, openVncBridge } from "../api";
import ConnectionForm from "../components/ConnectionForm";
import RdpLaunchModal from "../components/RdpLaunchModal";
import VncViewer from "../components/VncViewer";
import { PROTOCOL_LABEL, describeRdpExit, desktopLabel, errorMessage } from "../utils";
import "./RemoteDesktopPage.css";

interface LiveVnc {
  desktop: RemoteDesktop;
  bridge: VncBridge;
}

/**
 * Saved connections down the side; the selected one in the main area, either
 * as a summary with a Connect button or, for VNC, as the live viewer. RDP
 * connects out to the system client, so its "session" is a launch dialog and
 * a notice when the client exits.
 */
export default function RemoteDesktopPage() {
  const module = useModule("remote-desktop");
  const desktops = useResource(listDesktops);
  const confirmAction = useConfirm();
  const [params, setParams] = useSearchParams();
  const [selectedId, setSelectedId] = useState<string | null>(params.get("connection"));
  const [formFor, setFormFor] = useState<RemoteDesktop | "new" | null>(null);
  const [live, setLive] = useState<LiveVnc | null>(null);
  const [rdpFor, setRdpFor] = useState<RemoteDesktop | null>(null);
  const [notice, setNotice] = useState<{ tone: "success" | "danger" | "info"; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const rows = useMemo(() => desktops.data ?? [], [desktops.data]);
  const selected = rows.find((d) => d.id === selectedId) ?? null;

  // A deep link (`?connection=<id>`) is consumed once so the address bar
  // doesn't keep re-selecting it after the user moves on.
  useEffect(() => {
    if (params.has("connection")) {
      const next = new URLSearchParams(params);
      next.delete("connection");
      setParams(next, { replace: true });
    }
  }, [params, setParams]);

  useEffect(() => {
    const unlisten = onRdpExit((e) => setNotice(describeRdpExit(e)));
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  // The bridge is a live listener in Rust; leaving the page closes it.
  useEffect(() => {
    return () => {
      if (live) void closeVncBridge(live.bridge.session);
    };
  }, [live]);

  async function connect(desktop: RemoteDesktop) {
    setNotice(null);
    if (desktop.protocol === "rdp") {
      setRdpFor(desktop);
      return;
    }
    setBusy(true);
    try {
      if (live) await closeVncBridge(live.bridge.session);
      const bridge = await openVncBridge(desktop.id);
      setLive({ desktop, bridge });
    } catch (e) {
      setNotice({ tone: "danger", text: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }

  async function disconnect() {
    if (!live) return;
    const { session } = live.bridge;
    setLive(null);
    try {
      await closeVncBridge(session);
    } catch (e) {
      setNotice({ tone: "danger", text: errorMessage(e) });
    }
  }

  async function handleDelete(desktop: RemoteDesktop) {
    const ok = await confirmAction(`"${desktop.name}" (${desktopLabel(desktop)}) will be removed from the list.`, {
      title: "Delete connection?",
      confirmLabel: "Delete",
      danger: true,
    });
    if (!ok) return;
    try {
      if (live?.desktop.id === desktop.id) await disconnect();
      await deleteDesktop(desktop.id);
      if (selectedId === desktop.id) setSelectedId(null);
      await desktops.refresh();
    } catch (e) {
      setNotice({ tone: "danger", text: errorMessage(e) });
    }
  }

  function select(desktop: RemoteDesktop) {
    setSelectedId(desktop.id);
    setNotice(null);
  }

  const group = (protocol: RemoteDesktop["protocol"]) =>
    rows
      .filter((d) => d.protocol === protocol)
      .map((d) => ({
        key: d.id,
        label: d.name,
        icon: <Icon icon={protocol === "vnc" ? Monitor : MonitorUp} size="sm" />,
        meta: live?.desktop.id === d.id ? "live" : undefined,
        active: d.id === selectedId,
        onSelect: () => select(d),
      }));
  const vncItems = group("vnc");
  const rdpItems = group("rdp");

  // The viewer stays mounted while the user browses other connections in the
  // aside; it is only replaced by a summary when a different one is selected.
  const showViewer = live !== null && (selected === null || selected.id === live.desktop.id);

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(desktops)}
      fill
      actions={
        <>
          <Button variant="secondary" icon={RefreshCw} loading={desktops.refreshing} onClick={() => void desktops.refresh()}>
            Refresh
          </Button>
          <Button variant="primary" icon={Plus} onClick={() => setFormFor("new")}>
            New connection
          </Button>
        </>
      }
    >
      <SplitView
        asideLabel="Saved connections"
        asideWidth={240}
        className="rd-split"
        aside={
          rows.length === 0 ? (
            <EmptyState
              size="sm"
              icon={ScreenShare}
              title="No connections"
              description="Save a VNC or RDP machine once and connect with one click."
            />
          ) : (
            <>
              {vncItems.length > 0 && <SideList title="VNC" items={vncItems} />}
              {rdpItems.length > 0 && <SideList title="RDP" items={rdpItems} />}
            </>
          )
        }
      >
        <div className="rd-main">
          {notice && (
            <div className="rd-notice">
              <Callout tone={notice.tone} onDismiss={() => setNotice(null)}>
                {notice.text}
              </Callout>
            </div>
          )}

          {showViewer && live ? (
            <VncViewer key={live.bridge.session} desktop={live.desktop} bridge={live.bridge} onDisconnect={() => void disconnect()} />
          ) : selected ? (
            <div className="rd-summary">
              <Card
                title={selected.name}
                description={desktopLabel(selected)}
                icon={selected.protocol === "vnc" ? Monitor : MonitorUp}
                actions={<Badge tone={selected.protocol === "vnc" ? "accent" : "info"}>{PROTOCOL_LABEL[selected.protocol]}</Badge>}
              >
                <Stack gap={5}>
                  <dl className="rd-facts">
                    <dt>Host</dt>
                    <dd>
                      <Mono>
                        {selected.host}:{selected.port}
                      </Mono>
                    </dd>
                    <dt>Username</dt>
                    <dd>{selected.username ? <Mono>{selected.username}</Mono> : "—"}</dd>
                    {selected.protocol === "rdp" && (
                      <>
                        <dt>Domain</dt>
                        <dd>{selected.domain ? <Mono>{selected.domain}</Mono> : "—"}</dd>
                        <dt>Window</dt>
                        <dd>{selected.fullscreen ? "Fullscreen" : "Resizable"}</dd>
                      </>
                    )}
                    {selected.protocol === "vnc" && (
                      <>
                        <dt>Display</dt>
                        <dd>{selected.scale ? "Scaled to fit" : "Actual size"}</dd>
                        <dt>Input</dt>
                        <dd>{selected.view_only ? "View only" : "Mouse and keyboard"}</dd>
                      </>
                    )}
                  </dl>
                  <Stack direction="row" gap={3} wrap>
                    <Button variant="primary" icon={Plug} loading={busy} onClick={() => void connect(selected)}>
                      Connect
                    </Button>
                    <Button variant="secondary" icon={Pencil} onClick={() => setFormFor(selected)}>
                      Edit
                    </Button>
                    <Button variant="danger" icon={Trash2} onClick={() => void handleDelete(selected)}>
                      Delete
                    </Button>
                  </Stack>
                  {selected.protocol === "rdp" && (
                    <Callout tone="info">
                      RDP sessions open in your system's client (FreeRDP or Remmina) in a window of their own.
                    </Callout>
                  )}
                </Stack>
              </Card>
            </div>
          ) : (
            <EmptyState
              size="md"
              icon={ScreenShare}
              title={rows.length === 0 ? "No remote desktops yet" : "Pick a connection"}
              description={
                rows.length === 0
                  ? "VNC screens are viewed right here; RDP opens in your system's client."
                  : "Choose a saved machine on the left, or add a new one."
              }
              action={
                <Button variant="primary" icon={Plus} onClick={() => setFormFor("new")}>
                  New connection
                </Button>
              }
            />
          )}
        </div>
      </SplitView>

      {formFor && (
        <Modal
          size="md"
          title={formFor === "new" ? "New connection" : `Edit — ${formFor.name}`}
          icon={ScreenShare}
          onClose={() => setFormFor(null)}
        >
          <ConnectionForm
            desktop={formFor === "new" ? undefined : formFor}
            onSaved={(saved) => {
              setFormFor(null);
              setSelectedId(saved.id);
              void desktops.refresh();
            }}
            onCancel={() => setFormFor(null)}
          />
        </Modal>
      )}

      {rdpFor && (
        <RdpLaunchModal
          desktop={rdpFor}
          onLaunched={(client) => {
            setRdpFor(null);
            setNotice({ tone: "info", text: `${client} is opening ${rdpFor.name} in its own window.` });
          }}
          onClose={() => setRdpFor(null)}
        />
      )}
    </Page>
  );
}
