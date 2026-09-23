import { useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useModule } from "../../../app/ModuleRegistry";
import { usePopoutHandoff } from "../../../app/popoutState";
import { Badge, Callout, Page, statusOf, useResource } from "../../../ui";
import { SshHost, listHosts, sftpConnect, sftpDisconnect } from "../../ssh/api";
import { useConnectFlow } from "../../ssh/components/useConnectFlow";
import { errorMessage } from "../../ssh/utils";
import SftpTab, {
  PaneLayout,
  SftpControls,
  SftpSessionInfo,
  readPaneLayout,
  storePaneLayout,
} from "../components/SftpTab";

/**
 * The SFTP module: a two-pane file browser per connected host. Opens with
 * `?host=<id>` (from the SSH module's host rows) to connect straight away;
 * the parameter is consumed once so a reload doesn't reconnect.
 */
export default function SftpPage() {
  const module = useModule("sftp");
  const hosts = useResource(listHosts);
  const connectFlow = useConnectFlow();
  const [params, setParams] = useSearchParams();

  const [error, setError] = useState<string | null>(null);
  const [sessions, setSessions] = useState<SftpSessionInfo[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [layout, setLayout] = useState<PaneLayout>(readPaneLayout);
  function changeLayout(next: PaneLayout) {
    setLayout(next);
    storePaneLayout(next);
  }

  const hostList = hosts.data ?? [];

  // A popped-out window reconnects to the host in front (`?host=`, the same
  // deep link the SSH module uses); with nothing connected it starts empty.
  usePopoutHandoff(() => {
    const session = sessions.find((s) => s.id === active);
    return session ? { params: { host: session.host.id } } : null;
  });

  async function connect(host: SshHost) {
    setConnecting(true);
    setError(null);
    try {
      const id = await connectFlow.run(host, (creds) =>
        sftpConnect(host.id, creds),
      );
      if (id === null) return;
      setSessions((prev) => [...prev, { id, host }]);
      setActive(id);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setConnecting(false);
    }
  }

  async function disconnect(id: string) {
    setSessions((prev) => {
      const next = prev.filter((s) => s.id !== id);
      if (active === id) setActive(next[next.length - 1]?.id ?? null);
      return next;
    });
    await sftpDisconnect(id).catch(() => undefined);
  }

  // Deep link: wait for the host list, connect once, drop the parameter.
  const wanted = params.get("host");
  const connectRef = useRef(connect);
  connectRef.current = connect;
  useEffect(() => {
    if (!wanted || !hosts.data) return;
    const host = hosts.data.find((h) => h.id === wanted);
    setParams({}, { replace: true });
    if (host) void connectRef.current(host);
    else setError("That host no longer exists.");
  }, [wanted, hosts.data, setParams]);

  // Sessions are ours; drop them with the page.
  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;
  useEffect(() => {
    return () => {
      for (const s of sessionsRef.current)
        void sftpDisconnect(s.id).catch(() => undefined);
    };
  }, []);

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(hosts)}
      meta={
        sessions.length > 0 && (
          <Badge tone="accent">{sessions.length} connected</Badge>
        )
      }
      actions={
        <SftpControls
          sessions={sessions}
          activeId={active}
          hosts={hostList}
          connecting={connecting}
          layout={layout}
          onLayoutChange={changeLayout}
          onActivate={setActive}
          onConnect={connect}
          onDisconnect={disconnect}
        />
      }
      fill
    >
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}
      <SftpTab
        sessions={sessions}
        activeId={active}
        hosts={hostList}
        layout={layout}
      />
      {connectFlow.element}
    </Page>
  );
}
