import { useState } from "react";
import { useModule } from "../../../app/ModuleRegistry";
import { isWindows } from "../../../app/platform";
import { Page, StatusPill, Tabs, useResource } from "../../../ui";
import { Globe, Server, ShieldCheck } from "../../../ui/icons";
import {
  caStatus,
  detectApache,
  detectNginx,
  getWebServer,
  installApache,
  installNginx,
  listSites,
  WebServer,
} from "../api";
import SitesTab from "./SitesTab";
import SslTab from "./SslTab";
import WebServerTab from "./WebServerTab";

const NGINX_NOT_FOUND_HINT = isWindows
  ? "Install it manually, then recheck — unzip the Windows build from nginx.org/en/download.html to C:\\nginx, or add its folder to PATH."
  : "Install it manually, then recheck — e.g. sudo apt install nginx on Debian/Ubuntu, or brew install nginx on macOS.";

/**
 * Pure router for the module. It owns the one <h1> and the tab strip; the tabs
 * below render only Sections and never a Page of their own.
 *
 * Server, CA and web-server-choice detection are lifted here rather than living
 * in their own tabs, so their health shows in the header from any tab — you
 * shouldn't have to leave Sites to find out that the server isn't there.
 */
export default function Dashboard() {
  const module = useModule("nginx-ssl");
  const [active, setActive] = useState("sites");

  const nginx = useResource(detectNginx);
  const apache = useResource(detectApache);
  const webServer = useResource(getWebServer);
  const ca = useResource(caStatus);
  const sites = useResource(listSites);

  const serving = webServer.data === "apache" ? apache.data : nginx.data;
  const servingLabel = webServer.data === "apache" ? "Apache" : "nginx";

  // The server tab that serves the sites carries a badge, so which one is in use
  // reads from the tab strip without opening either tab.
  const inUse = (server: WebServer) =>
    webServer.data === server ? <StatusPill variant="warn" label="In use" /> : undefined;
  const tabs = [
    { id: "sites", label: "Sites", icon: Globe },
    { id: "nginx", label: "Nginx", icon: Server, badge: inUse("nginx") },
    { id: "apache", label: "Apache", icon: Server, badge: inUse("apache") },
    { id: "ssl", label: "SSL", icon: ShieldCheck },
  ];

  function handleSwitched() {
    void webServer.refresh();
    void sites.refresh();
    void apache.refresh();
  }

  return (
    <Page
      title={module.label}
      icon={module.icon}
      meta={
        <>
          {webServer.data && serving && (
            <StatusPill
              variant={serving.found ? "ok" : "bad"}
              label={serving.found ? `${servingLabel} ready` : `${servingLabel} missing`}
            />
          )}
          {ca.data && (
            <StatusPill
              variant={ca.data.exists ? "ok" : "warn"}
              label={ca.data.exists ? "CA created" : "no CA"}
            />
          )}
        </>
      }
      tabs={
        <Tabs
          tabs={tabs}
          active={active}
          onChange={setActive}
          ariaLabel="Sites, web servers and SSL sections"
        />
      }
    >
      {active === "sites" && <SitesTab sites={sites} webServer={webServer.data} />}
      {active === "nginx" && (
        <WebServerTab
          server="nginx"
          label="Nginx"
          detection={nginx}
          install={installNginx}
          notFoundHint={NGINX_NOT_FOUND_HINT}
          active={webServer.data}
          onSwitched={handleSwitched}
        />
      )}
      {active === "apache" && (
        <WebServerTab
          server="apache"
          label="Apache"
          detection={apache}
          install={installApache}
          notFoundHint={apache.data?.install_hint ?? ""}
          active={webServer.data}
          onSwitched={handleSwitched}
        />
      )}
      {active === "ssl" && <SslTab ca={ca} />}
    </Page>
  );
}
