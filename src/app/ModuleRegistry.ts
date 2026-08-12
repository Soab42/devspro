import { ComponentType } from "react";
import AppsPage from "../modules/apps/pages/AppsPage";
import DashboardPage from "../modules/dashboard/pages/DashboardPage";
import DatabasesPage from "../modules/databases/pages/DatabasesPage";
import DockerPage from "../modules/docker/pages/DockerPage";
import NginxSslDashboard from "../modules/nginx-ssl/pages/Dashboard";
import ProcessesPage from "../modules/processes/pages/ProcessesPage";
import SettingsPage from "../modules/settings/SettingsPage";

export interface ModuleDefinition {
  id: string;
  label: string;
  icon: string;
  path: string;
  Component: ComponentType;
}

// Adding a module later = one entry here + its own folder under src/modules/.
// The sidebar and router both render off this array — nothing else needs to change.
export const modules: ModuleDefinition[] = [
  { id: "dashboard", label: "Dashboard", icon: "📊", path: "/dashboard", Component: DashboardPage },
  { id: "nginx-ssl", label: "Nginx & SSL", icon: "🌐", path: "/nginx-ssl", Component: NginxSslDashboard },
  { id: "docker", label: "Docker", icon: "🐳", path: "/docker", Component: DockerPage },
  { id: "databases", label: "Databases", icon: "🗄️", path: "/databases", Component: DatabasesPage },
  { id: "apps", label: "Apps", icon: "🧰", path: "/apps", Component: AppsPage },
  { id: "processes", label: "Processes", icon: "🔌", path: "/processes", Component: ProcessesPage },
  { id: "settings", label: "Settings", icon: "⚙️", path: "/settings", Component: SettingsPage },
];
