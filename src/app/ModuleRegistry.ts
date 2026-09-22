import { ComponentType } from "react";
import AppsPage from "../modules/apps/pages/AppsPage";
import DashboardPage from "../modules/dashboard/pages/DashboardPage";
import DatabasesPage from "../modules/databases/pages/DatabasesPage";
import DockerPage from "../modules/docker/pages/DockerPage";
import EnvDiffPage from "../modules/env-diff/pages/EnvDiffPage";
import HostsPage from "../modules/hosts/pages/HostsPage";
import HttpPage from "../modules/http/pages/HttpPage";
import LogsPage from "../modules/logs/pages/LogsPage";
import MailCatcherPage from "../modules/mailcatcher/pages/MailCatcherPage";
import NginxSslDashboard from "../modules/nginx-ssl/pages/Dashboard";
import ProcessesPage from "../modules/processes/pages/ProcessesPage";
import SettingsPage from "../modules/settings/SettingsPage";
import SshPage from "../modules/ssh/pages/SshPage";
import RedisPage from "../modules/redis/pages/RedisPage";
import ScheduledPage from "../modules/scheduled/pages/ScheduledPage";
import WorkersPage from "../modules/workers/pages/WorkersPage";
import {
  BookUser,
  Container,
  Database,
  FileDiff,
  Globe,
  LayoutDashboard,
  Mail,
  Network,
  Package,
  ScrollText,
  Send,
  Settings,
  SquareTerminal,
  Workflow,
  CalendarClock,
  DatabaseZap,
} from "../ui/icons";
import type { LucideIcon } from "../ui/icons";

/**
 * Sidebar grouping. Ten flat entries gave the navigation no shape and no
 * ordering logic, so:
 *   services   — things with a lifecycle you start, stop, and remove
 *   monitoring — read-mostly views onto the machine
 *   system     — the machine and the app itself
 * Dashboard is ungrouped and pinned to the top: it's the home surface, not a
 * peer of the modules it summarises.
 */
export type ModuleGroup = "overview" | "services" | "monitoring" | "system";

export const MODULE_GROUPS: { id: ModuleGroup; label: string | null }[] = [
  { id: "overview", label: null },
  { id: "services", label: "Services" },
  { id: "monitoring", label: "Monitoring" },
  { id: "system", label: "System" },
];

export interface ModuleDefinition {
  id: string;
  label: string;
  /** A lucide component, not a string. Pages reuse it for their PageHeader. */
  icon: LucideIcon;
  group: ModuleGroup;
  path: string;
  Component: ComponentType;
  /** Optional one-liner shown under the page title. */
  description?: string;
}

// Adding a module later = one entry here + its own folder under src/modules/.
// The sidebar and router both render off this array — nothing else needs to change.
export const modules: ModuleDefinition[] = [
  {
    id: "dashboard",
    label: "Dashboard",
    icon: LayoutDashboard,
    group: "overview",
    path: "/dashboard",
    Component: DashboardPage,
  },
  {
    id: "nginx-ssl",
    label: "Nginx & SSL",
    icon: Globe,
    group: "services",
    path: "/nginx-ssl",
    Component: NginxSslDashboard,
  },
  {
    id: "docker",
    label: "Docker",
    icon: Container,
    group: "services",
    path: "/docker",
    Component: DockerPage,
  },
  {
    id: "databases",
    label: "Databases",
    icon: Database,
    group: "services",
    path: "/databases",
    Component: DatabasesPage,
  },
  {
    id: "mailcatcher",
    label: "Mail Catcher",
    icon: Mail,
    group: "services",
    path: "/mailcatcher",
    Component: MailCatcherPage,
  },
  {
    id: "workers",
    label: "Workers",
    icon: Workflow,
    group: "services",
    path: "/workers",
    Component: WorkersPage,
  },
  {
    id: "scheduled",
    label: "Scheduled tasks",
    icon: CalendarClock,
    group: "services",
    path: "/scheduled",
    Component: ScheduledPage,
  },
  {
    id: "redis",
    label: "Redis",
    icon: DatabaseZap,
    group: "services",
    path: "/redis",
    Component: RedisPage,
  },
  {
    id: "processes",
    label: "Processes",
    icon: Network,
    group: "monitoring",
    path: "/processes",
    Component: ProcessesPage,
  },
  {
    id: "logs",
    label: "Logs",
    icon: ScrollText,
    group: "monitoring",
    path: "/logs",
    Component: LogsPage,
  },
  {
    id: "http",
    label: "HTTP client",
    icon: Send,
    group: "monitoring",
    path: "/http",
    Component: HttpPage,
  },
  {
    id: "apps",
    label: "Apps",
    icon: Package,
    group: "system",
    path: "/apps",
    Component: AppsPage,
  },
  {
    id: "env-diff",
    label: "Env Diff",
    icon: FileDiff,
    group: "system",
    path: "/env-diff",
    Component: EnvDiffPage,
  },
  {
    id: "hosts",
    label: "Hosts file",
    icon: BookUser,
    group: "system",
    path: "/hosts",
    Component: HostsPage,
  },
  {
    id: "ssh",
    label: "SSH",
    icon: SquareTerminal,
    group: "system",
    path: "/ssh",
    Component: SshPage,
  },
  {
    id: "settings",
    label: "Settings",
    icon: Settings,
    group: "system",
    path: "/settings",
    Component: SettingsPage,
  },
];

/** Lookup for pages that want their own registry entry (icon, label). */
export function useModule(id: string): ModuleDefinition {
  const found = modules.find((m) => m.id === id);
  if (!found) throw new Error(`Unknown module: ${id}`);
  return found;
}
