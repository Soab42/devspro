//! Ties `ca`, the active web server (`web_server` — nginx or Apache), `hosts`, and
//! `ports` together into the add/remove/enable flows described in the plan: render the
//! vhost and issue the cert unprivileged, then apply exactly one elevation manifest
//! (write conf, ensure the server's include, resync /etc/hosts, config test, reload) per
//! action.

use super::limits::{self, SiteLimits};
use super::web_server::{self, Ready, WebServer};
use super::{ca, laravel, nginx, php_fpm, port_conflict};
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
        // NOTHING MORE GOES IN THIS LIST — see `MIGRATION_SITE_LIMITS` below and
        // the append-only zone in `db::later_migrations`.
    ]
}

/// Per-site nginx/PHP tuning (upload size, memory, timeouts, raw directives),
/// stored as one JSON blob rather than a column per setting. The set of knobs is
/// expected to grow, and each new one would otherwise cost a migration;
/// `limits::from_column` already degrades unparsable or NULL values to system
/// defaults, which is exactly the behaviour a new field needs on an old row.
///
/// Lives outside `migrations()` because that list is position-indexed and frozen;
/// it is registered in `db::later_migrations` instead. Adding it to the list above
/// shifted every later module's migration by one and re-ran them against real
/// databases.
pub const MIGRATION_SITE_LIMITS: &str = "ALTER TABLE nginx_ssl_sites ADD COLUMN limits TEXT;";

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
    pub limits: SiteLimits,
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
    "id, name, slug, hostname, backend_type, port, project_path, php_version, enabled, ssl_not_after, created_at, limits";

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
        limits: limits::from_column(row.get(11)?),
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

/// The web server the user picked, found on this machine and ready to drive.
fn active_server(db: &crate::db::Db) -> Result<Ready, String> {
    let server = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        web_server::active(&conn)
    };
    web_server::require(server)
}

/// Runs a manifest and turns the first failure into the message the user sees — with
/// a port clash spelled out (see `port_conflict`) rather than left as the server's raw
/// `bind()` error.
fn apply(app: &AppHandle, server: &Ready, manifest: OperationManifest, context: &str) -> Result<(), String> {
    let results = elevate::run(app, manifest)?;
    if let Some(failed) = results.iter().find(|r| !r.success) {
        return Err(format!("{context}: {}", port_conflict::explain(&failed.message, server.kind())));
    }
    Ok(())
}

/// The server's post-manifest step (restarting Apache on Windows). Its note, if any, is
/// returned for the callers that have somewhere to show it.
fn finish(server: &Ready) -> Result<Option<String>, String> {
    server
        .after_apply()
        .map_err(|e| port_conflict::explain(&e, server.kind()))
}

fn backend_target(backend: &Backend) -> Result<nginx::render::BackendTarget, String> {
    match backend {
        Backend::Port { port } => Ok(nginx::render::BackendTarget::Proxy { port: *port }),
        Backend::Laravel { project_path, php_version } => {
            let fastcgi_pass = match php_version {
                // The user explicitly pinned a version — if it can't be used, that's a hard
                // error rather than silently falling back to whatever else happens to be
                // available: a site that looks fine but is quietly running under the wrong
                // PHP version is worse than one that fails to reload at all. Why it can't
                // be used differs per OS, so the explanation comes from `php_fpm`.
                Some(version) => php_fpm::target_for_version(version).ok_or_else(|| {
                    format!(
                        "this site is pinned to PHP {version}. {}",
                        php_fpm::missing_version_hint(version)
                    )
                })?,
                None => php_fpm::require_target()?,
            };
            let public_root = PathBuf::from(project_path)
                .join("public")
                .to_string_lossy()
                .to_string();
            Ok(nginx::render::BackendTarget::Laravel {
                public_root,
                fastcgi_pass,
            })
        }
    }
}

fn render_conf_for(
    app: &AppHandle,
    server: &Ready,
    hostname: &str,
    backend: &Backend,
    limits: &SiteLimits,
    slug: &str,
) -> Result<String, String> {
    let certs_dir = super::certs_dir(app)?;
    let cert_path = certs_dir.join(format!("{hostname}.pem"));
    let key_path = certs_dir.join(format!("{hostname}-key.pem"));
    let logs_dir = super::logs_dir(app)?;
    let access_log = logs_dir.join(format!("{slug}.access.log"));
    let error_log = logs_dir.join(format!("{slug}.error.log"));
    let target = backend_target(backend)?;
    Ok(server.render(
        hostname,
        &target,
        limits,
        &cert_path.to_string_lossy(),
        &key_path.to_string_lossy(),
        &access_log.to_string_lossy(),
        &error_log.to_string_lossy(),
    ))
}

#[tauri::command(async)]
pub fn nginx_ssl_list_sites(db: State<crate::db::Db>) -> Result<Vec<Site>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list(&conn).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn nginx_ssl_hostname_conflict(hostname: String) -> bool {
    super::hosts::resolves_elsewhere(&hostname)
}

#[tauri::command(async)]
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
                    php_fpm::target_for_version(version)
                        .ok_or_else(|| php_fpm::missing_version_hint(version))?;
                }
                None => {
                    php_fpm::require_target()?;
                }
            }
            Backend::Laravel { project_path, php_version }
        }
        other => return Err(format!("unknown backend kind: {other}")),
    };

    let server = active_server(&db)?;

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

    // A new site starts at system defaults; tuning is a follow-up edit rather than
    // another decision to make in the add form.
    let limits = SiteLimits::default();
    let conf_contents = render_conf_for(&app, &server, &hostname, &backend, &limits, &slug)?;
    let conf_path = server.conf_path(&app, &slug)?;

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
    operations.extend(server.wire_in_ops(&app)?);
    operations.push(super::hosts::sync_op(&hostnames));
    operations.extend(server.test_and_reload_ops());

    let manifest = OperationManifest {
        allowed_write_prefix: allowed_write_prefix(&app)?,
        operations,
    };
    apply(&app, &server, manifest, "failed to activate site")?;

    let site = Site {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        slug,
        hostname,
        backend,
        limits,
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
    drop(conn);

    // After the row is saved: the site's files are all in place by now, so a failed
    // restart should leave it listed — fixable — rather than orphaned on disk.
    finish(&server).map_err(|e| format!("the site was added, but {e}"))?;
    Ok(site)
}

#[tauri::command(async)]
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
    let server = active_server(&db)?;
    let conf_path = server.conf_path(&app, &site.slug)?;

    let file_op = if enabled {
        Operation::WriteFile {
            path: conf_path,
            contents: render_conf_for(&app, &server, &site.hostname, &site.backend, &site.limits, &site.slug)?,
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

    let mut operations = vec![file_op];
    operations.extend(server.wire_in_ops(&app)?);
    operations.push(super::hosts::sync_op(&hostnames));
    operations.extend(server.test_and_reload_ops());

    let manifest = OperationManifest {
        allowed_write_prefix: allowed_write_prefix(&app)?,
        operations,
    };
    apply(&app, &server, manifest, "failed to update site")?;

    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE nginx_ssl_sites SET enabled = ?1 WHERE id = ?2",
            (enabled as i64, &id),
        )
        .map_err(|e| e.to_string())?;
    }
    finish(&server)?;
    Ok(())
}

/// Changes which installed PHP-FPM pool a Laravel site is pinned to (or clears
/// the pin, reverting to system-default), re-rendering and reloading nginx the
/// same way `nginx_ssl_set_site_enabled` does.
#[tauri::command(async)]
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

    let server = active_server(&db)?;
    let conf_path = server.conf_path(&app, &site.slug)?;

    let file_op = if site.enabled {
        Operation::WriteFile {
            path: conf_path,
            contents: render_conf_for(&app, &server, &site.hostname, &new_backend, &site.limits, &site.slug)?,
        }
    } else {
        Operation::RemoveFile { path: conf_path }
    };

    // No hosts resync here, unlike enable/disable: changing a Laravel site's PHP version
    // doesn't change which hostnames exist.
    let mut operations = vec![file_op];
    operations.extend(server.wire_in_ops(&app)?);
    operations.extend(server.test_and_reload_ops());

    let manifest = OperationManifest {
        allowed_write_prefix: allowed_write_prefix(&app)?,
        operations,
    };
    apply(&app, &server, manifest, "failed to update site")?;

    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE nginx_ssl_sites SET php_version = ?1 WHERE id = ?2",
            (&php_version, &id),
        )
        .map_err(|e| e.to_string())?;
    }
    finish(&server)?;
    Ok(())
}

/// Applies per-site nginx/PHP tuning — upload size, memory, timeouts, and raw
/// directives. See `limits` for why none of this touches php.ini.
///
/// Unlike the sibling setters, this one **rolls the conf back** if nginx rejects
/// it. The others only ever write values this app generated and already
/// validated; this one writes a free-text directive box, so a config nginx
/// refuses is an expected outcome rather than a bug. Without the rollback the
/// bad conf would stay on disk, and the *next* unrelated save — enabling a site,
/// switching PHP version — would fail its `nginx -t` on a config the user had
/// already been told was rejected.
#[tauri::command(async)]
pub fn nginx_ssl_set_site_limits(
    app: AppHandle,
    db: State<crate::db::Db>,
    id: String,
    limits: SiteLimits,
) -> Result<(), String> {
    let site = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        find(&conn, &id)?
    };
    let limits = limits.validate()?;

    // A disabled site has no conf on disk, so there is nothing to reload or roll
    // back — persist and let the next enable render it.
    if !site.enabled {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE nginx_ssl_sites SET limits = ?1 WHERE id = ?2",
            (limits::to_column(&limits), &id),
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }

    let server = active_server(&db)?;
    let conf_path = server.conf_path(&app, &site.slug)?;
    let allowed_write_prefix = allowed_write_prefix(&app)?;

    let previous_conf = render_conf_for(&app, &server, &site.hostname, &site.backend, &site.limits, &site.slug)?;
    let next_conf = render_conf_for(&app, &server, &site.hostname, &site.backend, &limits, &site.slug)?;

    let build = |contents: String| -> Result<OperationManifest, String> {
        let mut operations = vec![Operation::WriteFile {
            path: conf_path.clone(),
            contents,
        }];
        operations.extend(server.wire_in_ops(&app)?);
        operations.extend(server.test_and_reload_ops());
        Ok(OperationManifest {
            allowed_write_prefix: allowed_write_prefix.clone(),
            operations,
        })
    };
    // Both built up front so an unsupported server layout fails here, before any
    // prompt, rather than halfway through a rollback.
    let forward = build(next_conf)?;
    let rollback = build(previous_conf)?;

    let label = server.label();
    let results = elevate::run(&app, forward)?;
    if let Some(failed) = results.iter().find(|r| !r.success) {
        let reason = port_conflict::explain(&failed.message, server.kind());
        // Second prompt, but only on the failure path — and the alternative is
        // leaving the server holding a config it just rejected.
        return match elevate::run(&app, rollback) {
            Ok(_) => Err(format!("{label} rejected these settings, so they were not applied: {reason}")),
            Err(rollback_err) => Err(format!(
                "{label} rejected these settings: {reason}\n\nRestoring the previous config also failed: {rollback_err}. Fix the settings and save again, or disable and re-enable the site."
            )),
        };
    }

    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE nginx_ssl_sites SET limits = ?1 WHERE id = ?2",
            (limits::to_column(&limits), &id),
        )
        .map_err(|e| e.to_string())?;
    }
    finish(&server)?;
    Ok(())
}

/// Reissues a site's leaf certificate from the root CA currently on disk, then
/// re-renders and reloads its vhost.
///
/// Needed because a leaf is only as good as the CA that signed it: a leaf signed
/// by an earlier CA (a different name, or a regenerated key) still sits on disk
/// looking valid, but no browser can build a chain for it, so the site fails TLS
/// while its neighbours work. Reissuing is the repair, and it is cheap enough to
/// offer per site rather than making the user remove and re-add one.
#[tauri::command(async)]
pub fn nginx_ssl_reissue_site_cert(
    app: AppHandle,
    db: State<crate::db::Db>,
    id: String,
) -> Result<String, String> {
    let site = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        find(&conn, &id)?
    };

    let certs_dir = super::certs_dir(&app)?;
    ca::ensure_root_ca(&certs_dir)?;
    let leaf = ca::leaf::issue(&certs_dir, &site.hostname)?;

    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE nginx_ssl_sites SET ssl_not_after = ?1 WHERE id = ?2",
            (&leaf.not_after, &id),
        )
        .map_err(|e| e.to_string())?;
    }

    // A disabled site has no conf on disk; the new cert is on disk either way and
    // the next enable will render a vhost pointing at it.
    if !site.enabled {
        return Ok(format!(
            "Reissued the certificate for {}. Enable the site to serve it.",
            site.hostname
        ));
    }

    let server = active_server(&db)?;
    let conf_path = server.conf_path(&app, &site.slug)?;
    let contents = render_conf_for(&app, &server, &site.hostname, &site.backend, &site.limits, &site.slug)?;

    let mut operations = vec![Operation::WriteFile {
        path: conf_path,
        contents,
    }];
    operations.extend(server.wire_in_ops(&app)?);
    operations.extend(server.test_and_reload_ops());
    let manifest = OperationManifest {
        allowed_write_prefix: allowed_write_prefix(&app)?,
        operations,
    };
    apply(&app, &server, manifest, "the certificate was reissued, but the server rejected the config")?;

    let message = format!(
        "Reissued the certificate for {} and reloaded {}.",
        site.hostname,
        server.label()
    );
    Ok(match finish(&server)? {
        Some(note) => format!("{message} {note}"),
        None => message,
    })
}

/// Standalone re-run of the storage/bootstrap-cache permission fix, for Laravel
/// sites that were added before this app started applying it automatically at
/// add-time (or whose ownership got reset by something outside this app).
#[tauri::command(async)]
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

#[tauri::command(async)]
pub fn nginx_ssl_remove_site(app: AppHandle, db: State<crate::db::Db>, id: String) -> Result<(), String> {
    let site = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        find(&conn, &id)?
    };
    let server = active_server(&db)?;
    let conf_path = server.conf_path(&app, &site.slug)?;

    let mut hostnames = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        enabled_hostnames(&conn).map_err(|e| e.to_string())?
    };
    hostnames.retain(|h| h != &site.hostname);

    let mut operations = vec![Operation::RemoveFile { path: conf_path }];
    operations.extend(server.wire_in_ops(&app)?);
    operations.push(super::hosts::sync_op(&hostnames));
    operations.extend(server.test_and_reload_ops());

    let manifest = OperationManifest {
        allowed_write_prefix: allowed_write_prefix(&app)?,
        operations,
    };
    apply(&app, &server, manifest, "failed to remove site")?;

    let certs_dir = super::certs_dir(&app)?;
    let _ = std::fs::remove_file(certs_dir.join(format!("{}.pem", site.hostname)));
    let _ = std::fs::remove_file(certs_dir.join(format!("{}-key.pem", site.hostname)));

    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM nginx_ssl_sites WHERE id = ?1", [&id])
            .map_err(|e| e.to_string())?;
    }
    finish(&server)?;
    Ok(())
}

/// Moves every site to the other web server: renders each enabled site's vhost in the
/// new server's syntax, wires the new server in, tests and reloads it, and only then
/// removes the old server's copies and records the choice. Nothing is switched unless
/// the new server accepted the whole set.
///
/// The old server is left running — DevsPro did not start it and does not stop it — but
/// while it holds ports 80/443 the new one cannot serve, so the result says so.
#[tauri::command(async)]
pub fn nginx_ssl_set_web_server(
    app: AppHandle,
    db: State<crate::db::Db>,
    server: WebServer,
) -> Result<String, String> {
    let (current, sites) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        (web_server::active(&conn), list(&conn).map_err(|e| e.to_string())?)
    };
    let target = web_server::require(server)?;
    let label = target.label();

    let enabled: Vec<&Site> = sites.iter().filter(|s| s.enabled).collect();
    if !enabled.is_empty() {
        let mut operations = Vec::new();
        for site in &enabled {
            operations.push(Operation::WriteFile {
                path: target.conf_path(&app, &site.slug)?,
                contents: render_conf_for(&app, &target, &site.hostname, &site.backend, &site.limits, &site.slug)?,
            });
        }
        operations.extend(target.wire_in_ops(&app)?);
        operations.extend(target.test_and_reload_ops());
        if current != server {
            let old_dir = current.sites_dir(&app)?;
            for site in &sites {
                operations.push(Operation::RemoveFile {
                    path: old_dir.join(format!("{}.conf", site.slug)).to_string_lossy().to_string(),
                });
            }
        }
        let manifest = OperationManifest {
            allowed_write_prefix: allowed_write_prefix(&app)?,
            operations,
        };
        apply(&app, &target, manifest, &format!("could not move the sites to {label}"))?;
    }

    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        web_server::set_active(&conn, server)?;
    }

    let mut message = if enabled.is_empty() {
        format!("New sites will be served by {label}.")
    } else {
        format!(
            "{} site{} now served by {label}.",
            enabled.len(),
            if enabled.len() == 1 { " is" } else { "s are" }
        )
    };
    if !enabled.is_empty() {
        if let Some(note) = finish(&target)? {
            message.push(' ');
            message.push_str(&note);
        }
    }
    if current != server {
        message.push_str(&format!(
            " If {} is still running it keeps ports 80 and 443 — stop it so {label} can serve them.",
            current.label()
        ));
    }
    Ok(message)
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
