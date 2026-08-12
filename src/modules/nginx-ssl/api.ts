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

export type Backend = { type: "port"; port: number } | { type: "laravel"; project_path: string };

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
  invoke<Site>("nginx_ssl_add_site", { name, hostname, backendKind: "port", port, projectPath: null });
export const addLaravelSite = (name: string, hostname: string, projectPath: string) =>
  invoke<Site>("nginx_ssl_add_site", {
    name,
    hostname,
    backendKind: "laravel",
    port: null,
    projectPath,
  });

export const setSiteEnabled = (id: string, enabled: boolean) =>
  invoke<void>("nginx_ssl_set_site_enabled", { id, enabled });
export const fixLaravelPermissions = (id: string) =>
  invoke<string>("nginx_ssl_fix_laravel_permissions", { id });
export const removeSite = (id: string) => invoke<void>("nginx_ssl_remove_site", { id });

export const detectPhpFpm = () => invoke<PhpFpmDetection>("nginx_ssl_detect_php_fpm");
export const validateLaravelProject = (path: string) =>
  invoke<void>("nginx_ssl_validate_laravel_project", { path });
