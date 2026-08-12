import { useState } from "react";
import { useModule } from "../../../app/ModuleRegistry";
import { Page, StatusPill, Tabs, useResource } from "../../../ui";
import { Globe, Server, ShieldCheck } from "../../../ui/icons";
import { caStatus, detectNginx, listSites } from "../api";
import NginxTab from "./NginxTab";
import SitesTab from "./SitesTab";
import SslTab from "./SslTab";

const TABS = [
  { id: "sites", label: "Sites", icon: Globe },
  { id: "nginx", label: "Nginx", icon: Server },
  { id: "ssl", label: "SSL", icon: ShieldCheck },
];

/**
 * Pure router for the module. It owns the one <h1> and the tab strip; the three
 * tabs below render only Sections and never a Page of their own.
 *
 * nginx and CA detection are lifted here rather than living in their own tabs,
 * so their health shows in the header from any tab — you shouldn't have to
 * leave Sites to find out that nginx isn't running.
 */
export default function Dashboard() {
  const module = useModule("nginx-ssl");
  const [active, setActive] = useState("sites");

  const nginx = useResource(detectNginx);
  const ca = useResource(caStatus);
  const sites = useResource(listSites);

  return (
    <Page
      title={module.label}
      icon={module.icon}
      meta={
        <>
          {nginx.data && (
            <StatusPill
              variant={nginx.data.found ? "ok" : "bad"}
              label={nginx.data.found ? "nginx ready" : "nginx missing"}
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
          tabs={TABS}
          active={active}
          onChange={setActive}
          ariaLabel="Nginx and SSL sections"
        />
      }
    >
      {active === "sites" && <SitesTab sites={sites} />}
      {active === "nginx" && <NginxTab nginx={nginx} />}
      {active === "ssl" && <SslTab ca={ca} />}
    </Page>
  );
}
