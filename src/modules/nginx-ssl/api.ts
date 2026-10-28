import { invoke } from "@tauri-apps/api/core";
import type { WizardPlan, WizardStepReport } from "./utils";

export interface NginxDetection {
  found: boolean;
  binary_path: string | null;
  version: string | null;
  config_path: string | null;
  package_manager: string | null;
}

export interface ApacheDetection {
  found: boolean;
  /** `apachectl`/`apache2ctl` on Linux and macOS, `httpd.exe` on Windows. */
  binary_path: string | null;
  version: string | null;
  config_path: string | null;
  server_root: string | null;
  pid_file: string | null;
  /** Whether Apache's pid file exists right now. */
  running: boolean;
  package_manager: string | null;
  /** What to tell the user when there is no install button to offer. */
  install_hint: string;
}

/** Which web server DevsPro writes its sites for. One at a time — both want 80/443. */
export type WebServer = "nginx" | "apache";

export interface CaStatus {
  exists: boolean;
  not_after: string | null;
}

export type Backend =
  | { type: "port"; port: number }
  | { type: "laravel"; project_path: string; php_version: string | null };

/**
 * Per-site nginx/PHP tuning. `null` on every field means "system default" — the
 * vhost renders without the directive at all rather than with a value copied
 * from php.ini.
 *
 * `max_upload_size` deliberately drives three underlying directives at once
 * (nginx `client_max_body_size`, PHP `upload_max_filesize` and `post_max_size`);
 * see the Rust `limits` module for why they are not exposed separately.
 */
export interface SiteLimits {
  max_upload_size: string | null;
  memory_limit: string | null;
  max_execution_time: number | null;
  max_input_vars: number | null;
  extra_nginx: string | null;
  /** The same escape hatch for Apache — the two syntaxes have nothing in common. */
  extra_apache: string | null;
}

export const EMPTY_LIMITS: SiteLimits = {
  max_upload_size: null,
  memory_limit: null,
  max_execution_time: null,
  max_input_vars: null,
  extra_nginx: null,
  extra_apache: null,
};

export interface Site {
  id: string;
  name: string;
  slug: string;
  hostname: string;
  backend: Backend;
  limits: SiteLimits;
  enabled: boolean;
  ssl_not_after: string | null;
  created_at: string;
}

export interface PhpFpmDetection {
  found: boolean;
  /** What nginx is given as its `fastcgi_pass`: `unix:<socket>` on Linux and macOS, a
   *  `host:port` on Windows, where PHP runs as `php-cgi.exe` over a loopback port. */
  fastcgi_pass: string | null;
  version: string | null;
  /** What to tell the user when nothing usable was found. Comes from the backend because
   *  the right advice is `apt install php8.3-fpm` on one OS and "install PHP" on another. */
  install_hint: string;
}

export interface PhpFpmVersion {
  version: string;
  fastcgi_pass: string;
}

export const detectNginx = () => invoke<NginxDetection>("nginx_ssl_detect_nginx");
export const installNginx = () => invoke<string>("nginx_ssl_install_nginx");
export const detectApache = () => invoke<ApacheDetection>("nginx_ssl_detect_apache");
export const installApache = () => invoke<string>("nginx_ssl_install_apache");

export const getWebServer = () => invoke<WebServer>("nginx_ssl_get_web_server");
/**
 * Moves every site to `server`: re-renders them in its syntax, tests and reloads it,
 * and only then records the choice. Returns what the user should know afterwards.
 */
export const setWebServer = (server: WebServer) => invoke<string>("nginx_ssl_set_web_server", { server });

export const caStatus = () => invoke<CaStatus>("nginx_ssl_ca_status");
export const caEnsure = () => invoke<CaStatus>("nginx_ssl_ca_ensure");
export const caInstall = () => invoke<string>("nginx_ssl_ca_install");

export const suggestPort = (start: number) => invoke<number | null>("nginx_ssl_suggest_port", { start });
export const checkPortReachable = (port: number) => invoke<boolean>("nginx_ssl_check_port", { port });

export const listSites = () => invoke<Site[]>("nginx_ssl_list_sites");
export const hostnameConflict = (hostname: string) =>
  invoke<boolean>("nginx_ssl_hostname_conflict", { hostname });

export const addPortSite = (name: string, hostname: string, port: number) =>
  invoke<Site>("nginx_ssl_add_site", {
    name,
    hostname,
    backendKind: "port",
    port,
    projectPath: null,
    phpVersion: null,
  });
export const addLaravelSite = (name: string, hostname: string, projectPath: string, phpVersion: string | null) =>
  invoke<Site>("nginx_ssl_add_site", {
    name,
    hostname,
    backendKind: "laravel",
    port: null,
    projectPath,
    phpVersion,
  });

export const setSiteEnabled = (id: string, enabled: boolean) =>
  invoke<void>("nginx_ssl_set_site_enabled", { id, enabled });
export const setSitePhpVersion = (id: string, phpVersion: string | null) =>
  invoke<void>("nginx_ssl_set_site_php_version", { id, phpVersion });
export const setSiteLimits = (id: string, limits: SiteLimits) =>
  invoke<void>("nginx_ssl_set_site_limits", { id, limits });
/**
 * Re-signs a site's leaf certificate with the root CA currently on disk. The
 * repair for a site whose leaf was signed by an earlier CA — the browser can
 * verify the signature but cannot build a chain, so that one site fails TLS
 * while its neighbours are fine.
 */
export const reissueSiteCert = (id: string) =>
  invoke<string>("nginx_ssl_reissue_site_cert", { id });
export const fixLaravelPermissions = (id: string) =>
  invoke<string>("nginx_ssl_fix_laravel_permissions", { id });
export const removeSite = (id: string) => invoke<void>("nginx_ssl_remove_site", { id });

export const detectPhpFpm = () => invoke<PhpFpmDetection>("nginx_ssl_detect_php_fpm");
export const listPhpVersions = () => invoke<PhpFpmVersion[]>("nginx_ssl_list_php_versions");
export const validateLaravelProject = (path: string) =>
  invoke<void>("nginx_ssl_validate_laravel_project", { path });

export const listNvmVersions = () => invoke<string[]>("apps_list_nvm_versions");
export const writeNvmrc = (projectPath: string, version: string) =>
  invoke<void>("apps_write_nvmrc", { projectPath, version });

export const scaffoldCreateLaravelProject = (parentDir: string, projectName: string, hostname: string) =>
  invoke<Site>("scaffold_create_laravel_project", { parentDir, projectName, hostname });

/**
 * Runs the whole New Project set-up in one call. Returns per-step results:
 * the backend stops at the first failure but reports the steps that already
 * completed, so the UI can show which ones do not need redoing.
 */
export const scaffoldWizardRun = (plan: WizardPlan) =>
  invoke<WizardStepReport[]>("scaffold_wizard_run", { plan });

export interface TunnelDetection {
  found: boolean;
}

export interface TunnelStatus {
  site_id: string;
  hostname: string;
  public_url: string | null;
  pid: number;
  exited: boolean;
  last_log: string | null;
}

export const detectTunnelCli = () => invoke<TunnelDetection>("tunnel_detect");
export const tunnelStart = (siteId: string) => invoke<TunnelStatus>("tunnel_start", { siteId });
export const tunnelStop = (siteId: string) => invoke<void>("tunnel_stop", { siteId });
export const tunnelList = () => invoke<TunnelStatus[]>("tunnel_list");
