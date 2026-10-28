import { ComponentType, lazy } from "react";
import {
  AppWindow,
  BookUser,
  Bot,
  Container,
  Database,
  Eraser,
  FileDiff,
  Files,
  Globe,
  LayoutDashboard,
  Mail,
  MemoryStick,
  Network,
  Package,
  ScreenShare,
  ScrollText,
  Send,
  Settings,
  SquareTerminal,
  Table2,
  Terminal,
  FolderSync,
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

/**
 * Every module is an application in its own right: it opens in its own tab,
 * it can be switched off in Settings, and it is found through the launcher.
 */
export interface ModuleDefinition {
  id: string;
  label: string;
  /** A lucide component, not a string. Pages reuse it for their PageHeader. */
  icon: LucideIcon;
  group: ModuleGroup;
  path: string;
  /**
   * Always a lazy() component. The page's code — and everything it pulls in —
   * is fetched the first time the module is opened, so a module the user has
   * switched off costs nothing at startup.
   */
  Component: ComponentType;
  /** One-liner under the page title, and the second line in the launcher. */
  description?: string;
  /** Extra launcher search terms: synonyms and the things the module manages. */
  keywords?: string[];
  /**
   * Cannot be disabled and its tab cannot be closed. Only the Dashboard (the
   * home surface) and Settings (where modules are switched back on) qualify.
   */
  core?: boolean;
}

// Adding a module later = one entry here + its own folder under src/modules/.
// The sidebar, router, launcher and the Settings "Modules" section all render
// off this array — nothing else needs to change.
export const modules: ModuleDefinition[] = [
  {
    id: "dashboard",
    label: "Dashboard",
    icon: LayoutDashboard,
    group: "overview",
    path: "/dashboard",
    Component: lazy(() => import("../modules/dashboard/pages/DashboardPage")),
    description: "System health, resource usage and port conflicts at a glance.",
    keywords: ["home", "overview", "cpu", "memory", "disk", "ports"],
    core: true,
  },
  {
    id: "nginx-ssl",
    label: "Web Server & SSL",
    icon: Globe,
    group: "services",
    path: "/nginx-ssl",
    Component: lazy(() => import("../modules/nginx-ssl/pages/Dashboard")),
    description: "Local sites on nginx or Apache, virtual hosts, PHP versions and trusted certificates.",
    keywords: ["sites", "vhost", "https", "certificate", "ca", "php", "laravel", "tunnel", "nginx", "apache", "httpd"],
  },
  {
    id: "docker",
    label: "Docker",
    icon: Container,
    group: "services",
    path: "/docker",
    Component: lazy(() => import("../modules/docker/pages/DockerPage")),
    description: "Containers and compose stacks: start, stop, logs and ports.",
    keywords: ["containers", "compose", "images"],
  },
  {
    id: "databases",
    label: "Databases",
    icon: Database,
    group: "services",
    path: "/databases",
    Component: lazy(() => import("../modules/databases/pages/DatabasesPage")),
    description: "MySQL and PostgreSQL databases, users and backups.",
    keywords: ["mysql", "postgres", "postgresql", "sql", "backup", "users"],
  },
  {
    id: "data-browser",
    label: "Data browser",
    icon: Table2,
    group: "services",
    path: "/data-browser",
    Component: lazy(
      () => import("../modules/databases/pages/DataBrowserPage"),
    ),
    description:
      "Browse and edit rows in any MySQL or PostgreSQL database, local or remote.",
    keywords: [
      "sql",
      "rows",
      "tables",
      "query",
      "mysql",
      "postgres",
      "postgresql",
      "browse",
      "grid",
    ],
  },
  {
    id: "mailcatcher",
    label: "Mail Catcher",
    icon: Mail,
    group: "services",
    path: "/mailcatcher",
    Component: lazy(() => import("../modules/mailcatcher/pages/MailCatcherPage")),
    description: "Catch outgoing email from local apps and read it here.",
    keywords: ["email", "smtp", "mailhog", "mailpit", "inbox"],
  },
  {
    id: "workers",
    label: "Workers",
    icon: Workflow,
    group: "services",
    path: "/workers",
    Component: lazy(() => import("../modules/workers/pages/WorkersPage")),
    description: "Long-running queue workers and their output.",
    keywords: ["queue", "horizon", "supervisor", "jobs"],
  },
  {
    id: "scheduled",
    label: "Scheduled tasks",
    icon: CalendarClock,
    group: "services",
    path: "/scheduled",
    Component: lazy(() => import("../modules/scheduled/pages/ScheduledPage")),
    description: "Cron-style tasks that run on a schedule.",
    keywords: ["cron", "schedule", "timer", "artisan"],
  },
  {
    id: "redis",
    label: "Redis",
    icon: DatabaseZap,
    group: "services",
    path: "/redis",
    Component: lazy(() => import("../modules/redis/pages/RedisPage")),
    description: "Browse keys and inspect queues on a Redis server.",
    keywords: ["cache", "keys", "queue", "kv"],
  },
  {
    id: "processes",
    label: "Processes",
    icon: Network,
    group: "monitoring",
    path: "/processes",
    Component: lazy(() => import("../modules/processes/pages/ProcessesPage")),
    description: "Listening ports and the processes behind them.",
    keywords: ["ports", "listen", "kill", "pid", "netstat"],
  },
  {
    id: "memory",
    label: "Memory",
    icon: MemoryStick,
    group: "monitoring",
    path: "/memory",
    Component: lazy(() => import("../modules/memory/pages/MemoryPage")),
    description:
      "See which apps use your RAM, stop them, and let a watchdog close idle ones when memory runs out.",
    keywords: ["killer", "ram", "apps", "kill", "watchdog", "swap", "cache", "task manager"],
  },
  {
    id: "cleaner",
    label: "Cleaner",
    icon: Eraser,
    group: "system",
    path: "/cleaner",
    Component: lazy(() => import("../modules/cleaner/pages/CleanerPage")),
    description:
      "Find what is eating disk space (caches, Docker, large files, project junk) and clear it after a review.",
    keywords: ["cache", "npm", "pnpm", "docker", "prune", "disk", "space", "large files", "node_modules"],
  },
  {
    id: "logs",
    label: "Logs",
    icon: ScrollText,
    group: "monitoring",
    path: "/logs",
    Component: lazy(() => import("../modules/logs/pages/LogsPage")),
    description: "Tail application and service log files.",
    keywords: ["tail", "errors", "laravel.log", "nginx"],
  },
  {
    id: "http",
    label: "HTTP client",
    icon: Send,
    group: "monitoring",
    path: "/http",
    Component: lazy(() => import("../modules/http/pages/HttpPage")),
    description: "Send requests to your local sites and inspect the response.",
    keywords: ["rest", "api", "request", "postman", "curl"],
  },
  {
    id: "browser",
    label: "Browser",
    icon: AppWindow,
    group: "monitoring",
    path: "/browser",
    Component: lazy(() => import("../modules/browser/pages/BrowserPage")),
    description: "A built-in browser: tabs, bookmarks, saved logins, history, split view, device presets and the Web Inspector.",
    keywords: ["web", "preview", "webview", "chrome", "safari", "firefox", "bookmarks", "passwords", "inspect", "devtools", "split"],
  },
  {
    id: "apps",
    label: "Apps",
    icon: Package,
    group: "system",
    path: "/apps",
    Component: lazy(() => import("../modules/apps/pages/AppsPage")),
    description: "Install and update developer tools on this machine.",
    keywords: ["install", "packages", "tools", "node", "php", "composer"],
  },
  {
    id: "file-manager",
    label: "Files",
    icon: Files,
    group: "system",
    path: "/files",
    Component: lazy(() => import("../modules/file-manager/pages/FileManagerPage")),
    description: "Browse, open and edit files on this machine.",
    keywords: ["finder", "explorer", "folders", "directory"],
  },
  {
    id: "env-diff",
    label: "Env Diff",
    icon: FileDiff,
    group: "system",
    path: "/env-diff",
    Component: lazy(() => import("../modules/env-diff/pages/EnvDiffPage")),
    description: "Compare two .env files side by side and sync variables.",
    keywords: [".env", "environment", "variables", "compare"],
  },
  {
    id: "hosts",
    label: "Hosts file",
    icon: BookUser,
    group: "system",
    path: "/hosts",
    Component: lazy(() => import("../modules/hosts/pages/HostsPage")),
    description: "Edit /etc/hosts entries for local domains.",
    keywords: ["/etc/hosts", "dns", "domain", "localhost"],
  },
  {
    id: "ssh",
    label: "SSH",
    icon: SquareTerminal,
    group: "system",
    path: "/ssh",
    Component: lazy(() => import("../modules/ssh/pages/SshPage")),
    description: "Saved SSH connections and keys.",
    keywords: ["remote", "server", "hosts", "keys"],
  },
  {
    id: "ai",
    label: "AI agents",
    icon: Bot,
    group: "system",
    path: "/ai",
    Component: lazy(() => import("../modules/ai/pages/AiPage")),
    description: "Run Claude Code, Codex, Gemini CLI, OpenCode and other terminal coding agents in a project.",
    keywords: ["claude", "codex", "gemini", "opencode", "copilot", "aider", "agent", "llm", "assistant"],
  },
  {
    id: "terminal",
    label: "Terminal",
    icon: Terminal,
    group: "system",
    path: "/terminal",
    Component: lazy(() => import("../modules/terminal/pages/TerminalPage")),
    description: "Local and SSH shells: split panes, windows and a file browser that follows the shell.",
    keywords: ["shell", "console", "tmux", "ssh", "pty"],
  },
  {
    id: "sftp",
    label: "SFTP",
    icon: FolderSync,
    group: "system",
    path: "/sftp",
    Component: lazy(() => import("../modules/sftp/pages/SftpPage")),
    description: "Browse and transfer files on a saved SSH host.",
    keywords: ["files", "remote", "upload", "download", "ssh"],
  },
  {
    id: "remote-desktop",
    label: "Remote Desktop",
    icon: ScreenShare,
    group: "system",
    path: "/remote-desktop",
    Component: lazy(() => import("../modules/remote-desktop/pages/RemoteDesktopPage")),
    description: "VNC screens viewed in-app and RDP sessions in your system's client.",
    keywords: ["vnc", "rdp", "remote", "desktop", "screen", "freerdp", "remmina", "novnc", "windows"],
  },
  {
    id: "settings",
    label: "Settings",
    icon: Settings,
    group: "system",
    path: "/settings",
    Component: lazy(() => import("../modules/settings/SettingsPage")),
    description: "Appearance, background running, modules and backups.",
    keywords: ["preferences", "theme", "modules", "backup", "restore", "tray"],
    core: true,
  },
];

const byId = new Map(modules.map((m) => [m.id, m]));

/** The home module: the tab that is always open and the fallback route. */
export const HOME_MODULE = modules[0];

export function moduleById(id: string): ModuleDefinition | undefined {
  return byId.get(id);
}

/** Matches "/docker" and "/docker/anything", but not "/dockerfiles". */
export function moduleByPath(pathname: string): ModuleDefinition | undefined {
  return modules.find((m) => pathname === m.path || pathname.startsWith(`${m.path}/`));
}

/** Lookup for pages that want their own registry entry (icon, label). */
export function useModule(id: string): ModuleDefinition {
  const found = byId.get(id);
  if (!found) throw new Error(`Unknown module: ${id}`);
  return found;
}
