import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useModule } from "../../../app/ModuleRegistry";
import { Button, Page, Tabs, statusOf, useResource } from "../../../ui";
import { KeyRound, Plus, RefreshCw, Server } from "../../../ui/icons";
import { listHosts, listKeys } from "../api";
import HostsTab from "../components/HostsTab";
import KeysTab from "../components/KeysTab";

type TabId = "hosts" | "keys";

/**
 * Saved connections and keys. Terminals and SFTP are modules of their own
 * (`/terminal`, `/sftp`); a host row hands off to them with `?host=<id>`.
 */
export default function SshPage() {
  const module = useModule("ssh");
  const hosts = useResource(listHosts);
  const keys = useResource(listKeys);
  const navigate = useNavigate();
  const [tab, setTab] = useState<TabId>("hosts");

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(hosts)}
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
          <Button variant="primary" icon={Plus} onClick={() => navigate("/terminal?local=1")}>
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
            { id: "keys", label: "Keys", icon: KeyRound },
          ]}
        />
      }
    >
      {tab === "hosts" && (
        <HostsTab
          hosts={hosts}
          keys={keys.data ?? []}
          onOpenTerminal={(host) => navigate(`/terminal?host=${encodeURIComponent(host.id)}`)}
          onOpenSftp={(host) => navigate(`/sftp?host=${encodeURIComponent(host.id)}`)}
        />
      )}
      {tab === "keys" && <KeysTab keys={keys} />}
    </Page>
  );
}
