import { useState } from "react";
import Tabs from "../../../components/Tabs";
import SitesTab from "./SitesTab";
import NginxTab from "./NginxTab";
import SslTab from "./SslTab";

const TABS = [
  { id: "sites", label: "Sites" },
  { id: "nginx", label: "Nginx" },
  { id: "ssl", label: "SSL" },
];

export default function Dashboard() {
  const [active, setActive] = useState("sites");

  return (
    <div>
      <h1>Nginx & SSL</h1>
      <Tabs tabs={TABS} active={active} onChange={setActive} />
      {active === "sites" && <SitesTab />}
      {active === "nginx" && <NginxTab />}
      {active === "ssl" && <SslTab />}
    </div>
  );
}
