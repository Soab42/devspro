//! Ties `ca`, `nginx`, `hosts`, and `ports` together into the add/remove/enable
//! flows described in the plan: render the vhost and issue the cert unprivileged,
//! then apply exactly one elevation manifest (write conf, ensure the nginx.conf
//! include line, resync /etc/hosts, `nginx -t`, reload) per action.

use super::{ca, laravel, nginx, php_fpm};
use crate::elevate::{self, Operation, OperationManifest};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, State};

pub fn migrations() -> Vec<&'static str> {
    vec![
        r#"
        CREATE TABLE nginx_ssl_sites (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            slug TEXT NOT NULL UNIQUE,
            hostname TEXT NOT NULL UNIQUE,
            port INTEGER NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            ssl_not_after TEXT,
            created_at TEXT NOT NULL
        );
        "#,
        // v2: sites can now be backed by a Laravel project (served by PHP-FPM
        // through public/index.php) instead of only a proxied port. SQLite can't
        // relax a NOT NULL column in place, so the table is recreated.
        r#"
        CREATE TABLE nginx_ssl_sites_v2 (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            slug TEXT NOT NULL UNIQUE,
            hostname TEXT NOT NULL UNIQUE,
            backend_type TEXT NOT NULL DEFAULT 'port',
            port INTEGER,
            project_path TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            ssl_not_after TEXT,
            created_at TEXT NOT NULL
        );
        INSERT INTO nginx_ssl_sites_v2 (id, name, slug, hostname, backend_type, port, project_path, enabled, ssl_not_after, created_at)
        SELECT id, name, slug, hostname, 'port', port, NULL, enabled, ssl_not_after, created_at FROM nginx_ssl_sites;
        DROP TABLE nginx_ssl_sites;
        ALTER TABLE nginx_ssl_sites_v2 RENAME TO nginx_ssl_sites;
        "#,
        // v3: a Laravel site can pin which installed PHP-FPM pool it runs
        // against instead of always taking whichever socket `require_socket()`
        // picks. Nullable column, so a plain `ALTER TABLE ADD COLUMN` is enough —
        // no NOT NULL constraint to relax, unlike v2's rebuild.
        "ALTER TABLE nginx_ssl_sites ADD COLUMN php_version TEXT;",
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Backend {
    Port { port: u16 },
    /// Served directly by nginx + PHP-FPM via the project's `public/` front
    /// controller — no `artisan serve`, no port for the user to pick.
    Laravel {
        project_path: String,
        /// Which installed PHP-FPM pool to bind to (e.g. `"8.1"`). `None` means
        /// "system default" — today's behaviour of picking whichever pool
        /// `php_fpm::require_socket()` finds.
        php_version: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Site {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub hostname: String,
    pub backend: Backend,
    pub enabled: bool,
    pub ssl_not_after: Option<String>,
    pub created_at: String,
}

fn valid_hostname(hostname: &str) -> bool {
    if hostname.is_empty() || hostname.len() > 253 {
        return false;
    }
    let ok_chars = hostname
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    ok_chars
        && !hostname.starts_with('-')
        && !hostname.starts_with('.')
        && !hostname.ends_with('-')
        && !hostname.ends_with('.')
}

fn slugify(name: &str) -> String {
    let raw: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    raw.trim_matches('-').to_string()
}

const SITE_COLUMNS: &str =
    "id, name, slug, hostname, backend_type, port, project_path, php_version, enabled, ssl_not_after, created_at";

fn row_to_site(row: &rusqlite::Row) -> rusqlite::Result<Site> {
    let backend_type: String = row.get(4)?;
    let port: Option<i64> = row.get(5)?;
    let project_path: Option<String> = row.get(6)?;
    let php_version: Option<String> = row.get(7)?;
    let backend = match backend_type.as_str() {
        "laravel" => Backend::Laravel {
            project_path: project_path.unwrap_or_default(),
            php_version,
        },
        _ => Backend::Port {
            port: port.unwrap_or_default() as u16,
        },
    };
    Ok(Site {
        id: row.get(0)?,
        name: row.get(1)?,
        slug: row.get(2)?,
        hostname: row.get(3)?,
        backend,
        enabled: row.get::<_, i64>(8)? != 0,
        ssl_not_after: row.get(9)?,
        created_at: row.get(10)?,
    })
}

fn list(conn: &Connection) -> rusqlite::Result<Vec<Site>> {
    let mut stmt = conn.prepare(&format!("SELECT {SITE_COLUMNS} FROM nginx_ssl_sites ORDER BY created_at"))?;
    let rows = stmt.query_map([], row_to_site)?;
    rows.collect()
}

fn find(conn: &Connection, id: &str) -> Result<Site, String> {
    conn.query_row(
        &format!("SELECT {SITE_COLUMNS} FROM nginx_ssl_sites WHERE id = ?1"),
        [id],
        row_to_site,
    )
    .map_err(|_| "site not found".to_string())
}

/// Cross-module lookup for callers (e.g. `tunnel`) that need one site by id
/// but shouldn't need to know this module's `Connection`/row-parsing details.
pub(crate) fn find_by_id(db: &crate::db::Db, id: &str) -> Result<Site, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    find(&conn, id)
}

fn enabled_hostnames(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT hostname FROM nginx_ssl_sites WHERE enabled = 1")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

fn allowed_write_prefix(app: &AppHandle) -> Result<String, String> {
    Ok(super::app_root_dir(app)?.to_string_lossy().to_string())
}

fn conf_path_string(app: &AppHandle, slug: &str) -> Result<String, String> {
    Ok(super::nginx_sites_dir(app)?
        .join(format!("{slug}.conf"))
        .to_string_lossy()
        .to_string())
}

fn require_nginx() -> Result<(String, String), String> {
    let detection = nginx::detect();
    let binary_path = detection
        .binary_path
        .ok_or_else(|| "nginx was not found on this machine — install it first (see the Nginx panel)".to_string())?;
    let config_path = detection
        .config_path
        .ok_or_else(|| "could not determine nginx's main config path".to_string())?;
    Ok((binary_path, config_path))
}

fn backend_target(backend: &Backend) -> Result<nginx::render::BackendTarget, String> {
    match backend {
        Backend::Port { port } => Ok(nginx::render::BackendTarget::Proxy { port: *port }),
        Backend::Laravel { project_path, php_version } => {
            let php_fpm_socket = match php_version {
                // The user explicitly pinned a version — if it's no longer running,
                // that's a hard error rather than silently falling back to whatever
                // else happens to be up: a site that looks fine but is quietly
                // running under the wrong PHP version is worse than one that fails
                // to reload at all.
                Some(version) => php_fpm::socket_for_version(version).ok_or_else(|| {
                    format!(
                        "this site is pinned to PHP {version}, but it's not currently running (no live socket in /run/php) — start php{version}-fpm, or change the site's PHP version"
                    )
                })?,
                None => php_fpm::require_socket()?,
            };
            let public_root = PathBuf::from(project_path)
                .join("public")
                .to_string_lossy()
                .to_string();
            Ok(nginx::render::BackendTarget::Laravel {
                public_root,
                php_fpm_socket,
            })
        }
    }
}

fn render_conf_for(app: &AppHandle, hostname: &str, backend: &Backend, slug: &str) -> Result<String, String> {
    let certs_dir = super::certs_dir(app)?;
    let cert_path = certs_dir.join(format!("{hostname}.pem"));
    let key_path = certs_dir.join(format!("{hostname}-key.pem"));
    let logs_dir = super::logs_dir(app)?;
    let access_log = logs_dir.join(format!("{slug}.access.log"));
    let error_log = logs_dir.join(format!("{slug}.error.log"));
    let target = backend_target(backend)?;
    Ok(nginx::render::render_site_conf(
        hostname,
        &target,
        &cert_path.to_string_lossy(),
        &key_path.to_string_lossy(),
        &access_log.to_string_lossy(),
        &error_log.to_string_lossy(),
    ))
}

#[tauri::command]
pub fn nginx_ssl_list_sites(db: State<crate::db::Db>) -> Result<Vec<Site>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn nginx_ssl_hostname_conflict(hostname: String) -> bool {
    super::hosts::resolves_elsewhere(&hostname)
}

#[tauri::command]
pub fn nginx_ssl_add_site(
    app: AppHandle,
    db: State<crate::db::Db>,
    name: String,
    hostname: String,
    backend_kind: String,
    port: Option<u16>,
    project_path: Option<String>,
    php_version: Option<String>,
) -> Result<Site, String> {
    let hostname = hostname.trim().to_lowercase();
    if !valid_hostname(&hostname) {
        return Err("hostname must contain only letters, numbers, dots and hyphens".to_string());
    }
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    let slug = slugify(&name);
    if slug.is_empty() {
        return Err("name must contain at least one letter or number".to_string());
    }

    let backend = match backend_kind.as_str() {
        "port" => {
            let port = port.ok_or("a port is required for a port-proxy site")?;
            Backend::Port { port }
        }
        "laravel" => {
            let project_path = project_path.ok_or("a project folder is required for a Laravel site")?;
            if !laravel::is_laravel_project(std::path::Path::new(&project_path)) {
                return Err(
                    "that folder doesn't look like a Laravel project (expected an `artisan` file and `public/index.php`)"
                        .to_string(),
                );
            }
            match &php_version {
                Some(version) => {
                    php_fpm::socket_for_version(version).ok_or_else(|| {
                        format!(
                            "PHP {version} is not currently running (no live socket in /run/php) — install/start php{version}-fpm, or leave the version unset to use the system default"
                        )
                    })?;
                }
                None => {
                    php_fpm::require_socket()?;
                }
            }
            Backend::Laravel { project_path, php_version }
        }
        other => return Err(format!("unknown backend kind: {other}")),
    };

    let (binary_path, config_path) = require_nginx()?;

    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM nginx_ssl_sites WHERE hostname = ?1 OR slug = ?2",
                (&hostname, &slug),
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists > 0 {
            return Err(format!("a site named \"{name}\" or using {hostname} already exists"));
        }
    }

    let certs_dir = super::certs_dir(&app)?;
    ca::ensure_root_ca(&certs_dir)?;
    let leaf = ca::leaf::issue(&certs_dir, &hostname)?;

    let conf_contents = render_conf_for(&app, &hostname, &backend, &slug)?;
    let conf_path = conf_path_string(&app, &slug)?;
    let sites_glob = super::nginx_sites_dir(&app)?.join("*.conf").to_string_lossy().to_string();

    let mut hostnames = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        enabled_hostnames(&conn).map_err(|e| e.to_string())?
    };
    hostnames.push(hostname.clone());

    let mut operations = vec![Operation::WriteFile {
        path: conf_path,
        contents: conf_contents,
    }];
    if let Backend::Laravel { project_path, .. } = &backend {
        operations.push(Operation::FixLaravelPermissions {
            project_path: project_path.clone(),
        });
    }
    operations.extend([
        nginx::cleanup_legacy_include_block_op(&config_path),
        nginx::include_op(&config_path, &sites_glob)?,
        super::hosts::sync_op(&hostnames),
        Operation::RunNginxTest {
            binary_path: binary_path.clone(),
        },
        Operation::ReloadNginx { binary_path },
    ]);

    let manifest = OperationManifest {
        allowed_write_prefix: allowed_write_prefix(&app)?,
        operations,
    };

    let results = elevate::run(&app, manifest)?;
    if let Some(failed) = results.iter().find(|r| !r.success) {
        return Err(format!("failed to activate site: {}", failed.message));
    }

    let site = Site {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        slug,
        hostname,
        backend,
        enabled: true,
        ssl_not_after: Some(leaf.not_after),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let (backend_type, db_port, db_project_path, db_php_version) = match &site.backend {
        Backend::Port { port } => ("port", Some(*port as i64), None::<String>, None::<String>),
        Backend::Laravel { project_path, php_version } => {
            ("laravel", None, Some(project_path.clone()), php_version.clone())
        }
    };

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO nginx_ssl_sites (id, name, slug, hostname, backend_type, port, project_path, php_version, enabled, ssl_not_after, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?10)",
        (
            &site.id,
            &site.name,
            &site.slug,
            &site.hostname,
            backend_type,
            db_port,
            db_project_path,
            db_php_version,
            &site.ssl_not_after,
            &site.created_at,
        ),
    )
    .map_err(|e| e.to_string())?;

    Ok(site)
}

#[tauri::command]
pub fn nginx_ssl_set_site_enabled(
    app: AppHandle,
    db: State<crate::db::Db>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let site = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        find(&conn, &id)?
    };
    let (binary_path, config_path) = require_nginx()?;
    let conf_path = conf_path_string(&app, &site.slug)?;
    let sites_glob = super::nginx_sites_dir(&app)?.join("*.conf").to_string_lossy().to_string();

    let file_op = if enabled {
        Operation::WriteFile {
            path: conf_path,
            contents: render_conf_for(&app, &site.hostname, &site.backend, &site.slug)?,
        }
    } else {
        Operation::RemoveFile { path: conf_path }
    };

    let mut hostnames = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        enabled_hostnames(&conn).map_err(|e| e.to_string())?
    };
    hostnames.retain(|h| h != &site.hostname);
    if enabled {
        hostnames.push(site.hostname.clone());
    }

    let manifest = OperationManifest {
        allowed_write_prefix: allowed_write_prefix(&app)?,
        operations: vec![
            file_op,
            nginx::cleanup_legacy_include_block_op(&config_path),
            nginx::include_op(&config_path, &sites_glob)?,
            super::hosts::sync_op(&hostnames),
            Operation::RunNginxTest {
                binary_path: binary_path.clone(),
            },
            Operation::ReloadNginx { binary_path },
        ],
    };

    let results = elevate::run(&app, manifest)?;
    if let Some(failed) = results.iter().find(|r| !r.success) {
        return Err(format!("failed to update site: {}", failed.message));
    }

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE nginx_ssl_sites SET enabled = ?1 WHERE id = ?2",
        (enabled as i64, &id),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Changes which installed PHP-FPM pool a Laravel site is pinned to (or clears
/// the pin, reverting to system-default), re-rendering and reloading nginx the
/// same way `nginx_ssl_set_site_enabled` does.
#[tauri::command]
pub fn nginx_ssl_set_site_php_version(
    app: AppHandle,
    db: State<crate::db::Db>,
    id: String,
    php_version: Option<String>,
) -> Result<(), String> {
    let site = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        find(&conn, &id)?
    };
    let Backend::Laravel { project_path, .. } = &site.backend else {
        return Err("this site isn't a Laravel project".to_string());
    };
    let new_backend = Backend::Laravel {
        project_path: project_path.clone(),
        php_version: php_version.clone(),
    };

    let (binary_path, config_path) = require_nginx()?;
    let conf_path = conf_path_string(&app, &site.slug)?;
    let sites_glob = super::nginx_sites_dir(&app)?.join("*.conf").to_string_lossy().to_string();

    let file_op = if site.enabled {
        Operation::WriteFile {
            path: conf_path,
            contents: render_conf_for(&app, &site.hostname, &new_backend, &site.slug)?,
        }
    } else {
        Operation::RemoveFile { path: conf_path }
    };

    let manifest = OperationManifest {
        allowed_write_prefix: allowed_write_prefix(&app)?,
        operations: vec![
            file_op,
            nginx::cleanup_legacy_include_block_op(&config_path),
            nginx::include_op(&config_path, &sites_glob)?,
            Operation::RunNginxTest {
                binary_path: binary_path.clone(),
            },
            Operation::ReloadNginx { binary_path },
        ],
    };

    let results = elevate::run(&app, manifest)?;
    if let Some(failed) = results.iter().find(|r| !r.success) {
        return Err(format!("failed to update site: {}", failed.message));
    }

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE nginx_ssl_sites SET php_version = ?1 WHERE id = ?2",
        (&php_version, &id),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Standalone re-run of the storage/bootstrap-cache permission fix, for Laravel
/// sites that were added before this app started applying it automatically at
/// add-time (or whose ownership got reset by something outside this app).
#[tauri::command]
pub fn nginx_ssl_fix_laravel_permissions(app: AppHandle, db: State<crate::db::Db>, id: String) -> Result<String, String> {
    let site = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        find(&conn, &id)?
    };
    let Backend::Laravel { project_path, .. } = &site.backend else {
        return Err("this site isn't a Laravel project".to_string());
    };

    let manifest = OperationManifest {
        allowed_write_prefix: String::new(),
        operations: vec![Operation::FixLaravelPermissions {
            project_path: project_path.clone(),
        }],
    };
    let results = elevate::run(&app, manifest)?;
    match results.first() {
        Some(r) if r.success => Ok(r.message.clone()),
        Some(r) => Err(r.message.clone()),
        None => Err("helper returned no results".to_string()),
    }
}

#[tauri::command]
pub fn nginx_ssl_remove_site(app: AppHandle, db: State<crate::db::Db>, id: String) -> Result<(), String> {
    let site = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        find(&conn, &id)?
    };
    let (binary_path, config_path) = require_nginx()?;
    let conf_path = conf_path_string(&app, &site.slug)?;
    let sites_glob = super::nginx_sites_dir(&app)?.join("*.conf").to_string_lossy().to_string();

    let mut hostnames = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        enabled_hostnames(&conn).map_err(|e| e.to_string())?
    };
    hostnames.retain(|h| h != &site.hostname);

    let manifest = OperationManifest {
        allowed_write_prefix: allowed_write_prefix(&app)?,
        operations: vec![
            Operation::RemoveFile { path: conf_path },
            nginx::cleanup_legacy_include_block_op(&config_path),
            nginx::include_op(&config_path, &sites_glob)?,
            super::hosts::sync_op(&hostnames),
            Operation::RunNginxTest {
                binary_path: binary_path.clone(),
            },
            Operation::ReloadNginx { binary_path },
        ],
    };

    let results = elevate::run(&app, manifest)?;
    if let Some(failed) = results.iter().find(|r| !r.success) {
        return Err(format!("failed to remove site: {}", failed.message));
    }

    let certs_dir = super::certs_dir(&app)?;
    let _ = std::fs::remove_file(certs_dir.join(format!("{}.pem", site.hostname)));
    let _ = std::fs::remove_file(certs_dir.join(format!("{}-key.pem", site.hostname)));

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM nginx_ssl_sites WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_hostname_accepts_ordinary_names() {
        assert!(valid_hostname("myapp.test"));
        assert!(valid_hostname("my-app.local"));
        assert!(valid_hostname("a"));
    }

    #[test]
    fn valid_hostname_rejects_bad_shapes() {
        assert!(!valid_hostname(""));
        assert!(!valid_hostname("-leading.test"));
        assert!(!valid_hostname("trailing.test-"));
        assert!(!valid_hostname(".leading.test"));
        assert!(!valid_hostname("trailing.test."));
        assert!(!valid_hostname("has space.test"));
        assert!(!valid_hostname("has_underscore.test"));
        assert!(!valid_hostname(&"a".repeat(254)));
    }

    #[test]
    fn slugify_lowercases_and_dashes() {
        assert_eq!(slugify("My Cool App"), "my-cool-app");
        assert_eq!(slugify("  Leading/Trailing  "), "leading-trailing");
        assert_eq!(slugify("already-slug"), "already-slug");
    }

    #[test]
    fn slugify_of_only_symbols_is_empty() {
        assert_eq!(slugify("!!!"), "");
    }
}
