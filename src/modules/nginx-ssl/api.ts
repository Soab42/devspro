import { invoke } from "@tauri-apps/api/core";

export interface NginxDetection {
  found: boolean;
  binary_path: string | null;
  version: string | null;
  config_path: string | null;
  linux_package_manager: string | null;
}

export interface CaStatus {
  exists: boolean;
  not_after: string | null;
}

export type Backend =
  | { type: "port"; port: number }
  | { type: "laravel"; project_path: string; php_version: string | null };

export interface Site {
  id: string;
  name: string;
  slug: string;
  hostname: string;
  backend: Backend;
  enabled: boolean;
  ssl_not_after: string | null;
  created_at: string;
}

export interface PhpFpmDetection {
  found: boolean;
  socket_path: string | null;
  version: string | null;
}

export interface PhpFpmVersion {
  version: string;
  socket_path: string;
}

export const detectNginx = () => invoke<NginxDetection>("nginx_ssl_detect_nginx");
export const installNginx = () => invoke<string>("nginx_ssl_install_nginx");

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
