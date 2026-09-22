import { useState } from "react";
import { useModule } from "../../../app/ModuleRegistry";
import { Badge, Button, Callout, Page, Tabs, statusOf, useResource } from "../../../ui";
import { Folder, KeyRound, Plus, RefreshCw, Server, SquareTerminal } from "../../../ui/icons";
import { PtyCommand, SshHost, listHosts, listKeys, sftpConnect, sftpDisconnect } from "../api";
import { errorMessage, terminalTitle } from "../utils";
import HostsTab from "../components/HostsTab";
import KeysTab from "../components/KeysTab";
import SftpTab, { SftpSessionInfo } from "../components/SftpTab";
import TerminalsTab, { TerminalSession } from "../components/TerminalsTab";
import { useConnectFlow } from "../components/useConnectFlow";

type TabId = "hosts" | "terminals" | "sftp" | "keys";

/**
 * Sessions (open terminals, SFTP connections) live here, above the tabs, so
 * switching tabs doesn't tear them down: a terminal keeps running while you
 * browse files, and the SFTP browser keeps its place while you type.
 */
export default function SshPage() {
  const module = useModule("ssh");
  const hosts = useResource(listHosts);
  const keys = useResource(listKeys);
  const connectFlow = useConnectFlow();

  const [tab, setTab] = useState<TabId>("hosts");
  const [error, setError] = useState<string | null>(null);

  const [terminals, setTerminals] = useState<TerminalSession[]>([]);
  const [activeTerminal, setActiveTerminal] = useState<string | null>(null);

  const [sftpSessions, setSftpSessions] = useState<SftpSessionInfo[]>([]);
  const [activeSftp, setActiveSftp] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);

  const hostList = hosts.data ?? [];
  const keyList = keys.data ?? [];

  function openTerminal(command: PtyCommand) {
    const id = crypto.randomUUID();
    const same = terminals.filter((t) =>
      command === "local" ? t.command === "local" : t.command !== "local" && t.command.ssh === command.ssh,
    ).length;
    const session: TerminalSession = {
      id,
      command,
      title: terminalTitle(command, hostList, same + 1),
      exited: false,
    };
    setTerminals((prev) => [...prev, session]);
    setActiveTerminal(id);
    setTab("terminals");
  }

  function closeTerminal(id: string) {
    setTerminals((prev) => {
      const next = prev.filter((t) => t.id !== id);
      if (activeTerminal === id) setActiveTerminal(next[next.length - 1]?.id ?? null);
      return next;
    });
  }

  async function openSftp(host: SshHost) {
    setConnecting(true);
    setError(null);
    try {
      const id = await connectFlow.run(host, (creds) => sftpConnect(host.id, creds));
      if (id === null) return;
      setSftpSessions((prev) => [...prev, { id, host }]);
      setActiveSftp(id);
      setTab("sftp");
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setConnecting(false);
    }
  }

  async function closeSftp(id: string) {
    setSftpSessions((prev) => {
      const next = prev.filter((s) => s.id !== id);
      if (activeSftp === id) setActiveSftp(next[next.length - 1]?.id ?? null);
      return next;
    });
    await sftpDisconnect(id).catch(() => undefined);
  }

  const openCount = terminals.filter((t) => !t.exited).length + sftpSessions.length;

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(hosts)}
      meta={openCount > 0 && <Badge tone="accent">{openCount} open</Badge>}
      actions={
        <>
          <Button
            variant="secondary"
            icon={RefreshCw}
            loading={hosts.refreshing || keys.refreshing}
            onClick={() => {
              void hosts.refresh();
              void keys.refresh();
            }}
          >
            Refresh
          </Button>
          <Button variant="primary" icon={Plus} onClick={() => openTerminal("local")}>
            Local terminal
          </Button>
        </>
      }
      tabs={
        <Tabs
          ariaLabel="SSH views"
          active={tab}
          onChange={(id) => setTab(id as TabId)}
          tabs={[
            { id: "hosts", label: "Hosts", icon: Server },
            {
              id: "terminals",
              label: terminals.length > 0 ? `Terminals (${terminals.length})` : "Terminals",
              icon: SquareTerminal,
            },
            {
              id: "sftp",
              label: sftpSessions.length > 0 ? `SFTP (${sftpSessions.length})` : "SFTP",
              icon: Folder,
            },
            { id: "keys", label: "Keys", icon: KeyRound },
          ]}
        />
      }
    >
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}

      {tab === "hosts" && (
        <HostsTab
          hosts={hosts}
          keys={keyList}
          onOpenTerminal={(host) => openTerminal({ ssh: host.id })}
          onOpenSftp={openSftp}
        />
      )}

      {/* Terminals stay mounted across tab switches (hidden, not unmounted)
          so their processes and scrollback survive. */}
      <TerminalsTab
        sessions={terminals}
        activeId={activeTerminal}
        hidden={tab !== "terminals"}
        onActivate={setActiveTerminal}
        onClose={closeTerminal}
        onExited={(id) => setTerminals((prev) => prev.map((t) => (t.id === id ? { ...t, exited: true } : t)))}
        onError={setError}
        onNewLocal={() => openTerminal("local")}
      />

      {tab === "sftp" && (
        <SftpTab
          sessions={sftpSessions}
          activeId={activeSftp}
          hosts={hostList}
          connecting={connecting}
          onActivate={setActiveSftp}
          onConnect={openSftp}
          onDisconnect={closeSftp}
        />
      )}

      {tab === "keys" && <KeysTab keys={keys} />}

      {connectFlow.element}
    </Page>
  );
}
