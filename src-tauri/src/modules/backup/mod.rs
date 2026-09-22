//! Config export and import: one JSON bundle of everything DevsPro *defines*
//! (sites, workers, preferences, the root CA) so a setup can be carried to a new
//! machine. It deliberately carries no *derived* state — no nginx vhosts, no leaf
//! certificates, no /etc/hosts lines — because all of that is re-creatable from
//! the definitions, and re-creating it needs elevation plus checks (nginx present,
//! PHP-FPM pools, free ports) that belong to the Sites flows, not to a file import.
//! So `backup_import` only writes rows and CA files, then tells the UI to re-apply.
//!
//! Format: `{ format, version, exported_at, sites, workers, settings, ca, ... }`.
//! Every section is optional on read and unknown sections are ignored, so a
//! future module (ssh_hosts, scheduled_tasks) can add its own key without a
//! version bump; `version` only changes when an existing section's shape does.

use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use tauri::{AppHandle, State};

use super::nginx_ssl::ca;

pub const FORMAT: &str = "devspro-backup";
pub const VERSION: u32 = 1;

/// Written by `auth` to remember which profile is unlocked. It is this machine's
/// login state, not configuration: exporting it would silently unlock the same
/// profile on the destination, and replacing settings would otherwise log the
/// importing user out mid-import. Mirrors `auth::SESSION_SETTING_KEY`.
const SESSION_SETTING_KEY: &str = "session_unlocked_profile_id";

/// Bundles bigger than this are not something this app produced.
const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// A `nginx_ssl_sites` row, column for column. Kept as the raw row rather than
/// `sites::Site` so the bundle survives future `Site` refactors and the insert
/// needs no re-derivation of `backend_type`/`port`/`project_path`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteRow {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub hostname: String,
    pub backend_type: String,
    pub port: Option<i64>,
    pub project_path: Option<String>,
    pub php_version: Option<String>,
    pub enabled: bool,
    pub ssl_not_after: Option<String>,
    pub created_at: String,
    /// The JSON blob from the `limits` column, passed through untouched.
    pub limits: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerRow {
    pub id: String,
    pub name: String,
    pub command: String,
    pub working_dir: String,
    pub site_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaBundle {
    pub cert_pem: String,
    pub key_pem: String,
    /// Contents of the `rootCA-expiry.txt` sidecar, when it exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    #[serde(default)]
    pub sites: Vec<SiteRow>,
    #[serde(default)]
    pub workers: Vec<WorkerRow>,
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca: Option<CaBundle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Sites,
    Workers,
    Settings,
    Ca,
}

impl Section {
    fn parse(name: &str) -> Result<Section, String> {
        match name {
            "sites" => Ok(Section::Sites),
            "workers" => Ok(Section::Workers),
            "settings" => Ok(Section::Settings),
            "ca" => Ok(Section::Ca),
            other => Err(format!("unknown backup section \"{other}\"")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Upsert by id; rows the bundle doesn't mention are kept.
    Merge,
    /// Delete everything in the section first, then insert.
    Replace,
}

#[derive(Debug, Serialize)]
pub struct SectionCount {
    pub section: Section,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub exported_at: String,
    pub version: u32,
    pub sections: Vec<SectionCount>,
}

#[derive(Debug, Serialize)]
pub struct ImportSummary {
    pub imported: Vec<SectionCount>,
    /// Paths the previous CA files were moved to, if the CA section was written.
    pub ca_backups: Vec<String>,
    pub needs_apply: bool,
    pub note: String,
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

pub fn serialize(bundle: &Bundle) -> Result<String, String> {
    serde_json::to_string_pretty(bundle).map_err(|e| format!("could not serialise backup: {e}"))
}

/// Parses and validates a bundle. Format and version are checked here — before
/// any row is touched — so a wrong file fails with one clear message instead of
/// a half-applied transaction.
pub fn parse(text: &str) -> Result<Bundle, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let format = value.get("format").and_then(|v| v.as_str()).unwrap_or("");
    if format != FORMAT {
        return Err(format!(
            "not a DevsPro backup (expected format \"{FORMAT}\", found \"{format}\")"
        ));
    }
    let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    if version == 0 || version > VERSION as u64 {
        return Err(format!(
            "backup version {version} is not supported by this version of DevsPro (supports up to {VERSION})"
        ));
    }
    serde_json::from_value(value).map_err(|e| format!("malformed backup: {e}"))
}

pub fn inspect(bundle: &Bundle) -> Inspection {
    Inspection {
        exported_at: bundle.exported_at.clone(),
        version: bundle.version,
        sections: vec![
            SectionCount { section: Section::Sites, count: bundle.sites.len() },
            SectionCount { section: Section::Workers, count: bundle.workers.len() },
            SectionCount { section: Section::Settings, count: bundle.settings.len() },
            SectionCount { section: Section::Ca, count: usize::from(bundle.ca.is_some()) },
        ],
    }
}

fn parse_sections(names: &[String]) -> Result<Vec<Section>, String> {
    if names.is_empty() {
        return Err("no sections selected".to_string());
    }
    let mut sections = Vec::new();
    for name in names {
        let section = Section::parse(name)?;
        if !sections.contains(&section) {
            sections.push(section);
        }
    }
    Ok(sections)
}

fn exportable_setting(key: &str) -> bool {
    key != SESSION_SETTING_KEY
}

// ---------------------------------------------------------------------------
// Database read / write
// ---------------------------------------------------------------------------

const SITE_COLUMNS: &str =
    "id, name, slug, hostname, backend_type, port, project_path, php_version, enabled, ssl_not_after, created_at, limits";
const WORKER_COLUMNS: &str = "id, name, command, working_dir, site_id, created_at";

fn read_sites(conn: &Connection) -> Result<Vec<SiteRow>, String> {
    let mut stmt = conn
        .prepare(&format!("SELECT {SITE_COLUMNS} FROM nginx_ssl_sites ORDER BY created_at"))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SiteRow {
                id: row.get(0)?,
                name: row.get(1)?,
                slug: row.get(2)?,
                hostname: row.get(3)?,
                backend_type: row.get(4)?,
                port: row.get(5)?,
                project_path: row.get(6)?,
                php_version: row.get(7)?,
                enabled: row.get::<_, i64>(8)? != 0,
                ssl_not_after: row.get(9)?,
                created_at: row.get(10)?,
                limits: row.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

fn read_workers(conn: &Connection) -> Result<Vec<WorkerRow>, String> {
    let mut stmt = conn
        .prepare(&format!("SELECT {WORKER_COLUMNS} FROM workers ORDER BY created_at"))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(WorkerRow {
                id: row.get(0)?,
                name: row.get(1)?,
                command: row.get(2)?,
                working_dir: row.get(3)?,
                site_id: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

fn read_settings(conn: &Connection) -> Result<BTreeMap<String, String>, String> {
    let mut stmt = conn
        .prepare("SELECT key, value FROM settings ORDER BY key")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut settings = BTreeMap::new();
    for row in rows {
        let (key, value) = row.map_err(|e| e.to_string())?;
        if exportable_setting(&key) {
            settings.insert(key, value);
        }
    }
    Ok(settings)
}

fn read_ca(certs_dir: &Path) -> Result<Option<CaBundle>, String> {
    let files = ca::root_ca_files(certs_dir);
    if !files.cert.is_file() || !files.key.is_file() {
        return Ok(None);
    }
    let cert_pem = fs::read_to_string(&files.cert).map_err(|e| format!("could not read root CA cert: {e}"))?;
    let key_pem = fs::read_to_string(&files.key).map_err(|e| format!("could not read root CA key: {e}"))?;
    let not_after = fs::read_to_string(&files.expiry).ok().map(|s| s.trim().to_string());
    Ok(Some(CaBundle { cert_pem, key_pem, not_after }))
}

/// Everything the bundle holds, read from the database and the certs dir.
pub fn collect(conn: &Connection, certs_dir: &Path, exported_at: String) -> Result<Bundle, String> {
    Ok(Bundle {
        format: FORMAT.to_string(),
        version: VERSION,
        exported_at,
        sites: read_sites(conn)?,
        workers: read_workers(conn)?,
        settings: read_settings(conn)?,
        ca: read_ca(certs_dir)?,
    })
}

/// A hostname/slug clash under Merge means the bundle's row and an existing row
/// with a *different* id want the same site. The raw SQLite message
/// ("UNIQUE constraint failed: nginx_ssl_sites.hostname") is accurate but tells
/// the user nothing about which site or what to do.
fn site_insert_error(site: &SiteRow, err: rusqlite::Error) -> String {
    let message = err.to_string();
    if message.contains("UNIQUE constraint failed") {
        format!(
            "site \"{}\" ({}) clashes with an existing site that has a different id — remove it first, or import with Replace",
            site.name, site.hostname
        )
    } else {
        format!("could not import site \"{}\": {message}", site.name)
    }
}

fn write_sites(tx: &Transaction, sites: &[SiteRow], mode: Mode) -> Result<usize, String> {
    if mode == Mode::Replace {
        tx.execute("DELETE FROM nginx_ssl_sites", []).map_err(|e| e.to_string())?;
    }
    for site in sites {
        tx.execute(
            &format!(
                "INSERT INTO nginx_ssl_sites ({SITE_COLUMNS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name, slug = excluded.slug, hostname = excluded.hostname,
                   backend_type = excluded.backend_type, port = excluded.port,
                   project_path = excluded.project_path, php_version = excluded.php_version,
                   enabled = excluded.enabled, ssl_not_after = excluded.ssl_not_after,
                   created_at = excluded.created_at, limits = excluded.limits"
            ),
            params![
                site.id,
                site.name,
                site.slug,
                site.hostname,
                site.backend_type,
                site.port,
                site.project_path,
                site.php_version,
                i64::from(site.enabled),
                site.ssl_not_after,
                site.created_at,
                site.limits,
            ],
        )
        .map_err(|e| site_insert_error(site, e))?;
    }
    Ok(sites.len())
}

fn write_workers(tx: &Transaction, workers: &[WorkerRow], mode: Mode) -> Result<usize, String> {
    if mode == Mode::Replace {
        tx.execute("DELETE FROM workers", []).map_err(|e| e.to_string())?;
    }
    for worker in workers {
        tx.execute(
            &format!(
                "INSERT INTO workers ({WORKER_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name, command = excluded.command,
                   working_dir = excluded.working_dir, site_id = excluded.site_id,
                   created_at = excluded.created_at"
            ),
            params![
                worker.id,
                worker.name,
                worker.command,
                worker.working_dir,
                worker.site_id,
                worker.created_at
            ],
        )
        .map_err(|e| format!("could not import worker \"{}\": {e}", worker.name))?;
    }
    Ok(workers.len())
}

fn write_settings(tx: &Transaction, settings: &BTreeMap<String, String>, mode: Mode) -> Result<usize, String> {
    if mode == Mode::Replace {
        // Never the session key — see SESSION_SETTING_KEY.
        tx.execute("DELETE FROM settings WHERE key != ?1", [SESSION_SETTING_KEY])
            .map_err(|e| e.to_string())?;
    }
    let mut count = 0;
    for (key, value) in settings {
        if !exportable_setting(key) {
            continue;
        }
        tx.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (key, value),
        )
        .map_err(|e| format!("could not import setting \"{key}\": {e}"))?;
        count += 1;
    }
    Ok(count)
}

/// Applies the database sections of a bundle in one transaction: either every
/// selected section lands or none does. The CA section is file IO and is handled
/// separately by `write_ca`, after this has committed.
pub fn apply(conn: &mut Connection, bundle: &Bundle, sections: &[Section], mode: Mode) -> Result<Vec<SectionCount>, String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut imported = Vec::new();
    for section in sections {
        let count = match section {
            Section::Sites => write_sites(&tx, &bundle.sites, mode)?,
            Section::Workers => write_workers(&tx, &bundle.workers, mode)?,
            Section::Settings => write_settings(&tx, &bundle.settings, mode)?,
            Section::Ca => continue,
        };
        imported.push(SectionCount { section: *section, count });
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(imported)
}

fn backup_suffix(timestamp: &str) -> String {
    // RFC3339 has ':' which is awkward in filenames; keep digits only.
    let digits: String = timestamp.chars().filter(|c| c.is_ascii_digit()).collect();
    format!("bak-{digits}")
}

fn move_aside(path: &Path, timestamp: &str) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let backup = path.with_file_name(format!("{name}.{}", backup_suffix(timestamp)));
    fs::rename(path, &backup).map_err(|e| format!("could not back up {name}: {e}"))?;
    Ok(Some(backup.to_string_lossy().into_owned()))
}

/// Replaces the root CA on disk with the bundle's, keeping the old files as
/// `<name>.bak-<timestamp>` — a CA the user may still have installed in a trust
/// store is not something to overwrite silently. Returns the backup paths.
pub fn write_ca(certs_dir: &Path, bundle: &CaBundle, timestamp: &str) -> Result<Vec<String>, String> {
    if !bundle.cert_pem.contains("BEGIN CERTIFICATE") {
        return Err("the backup's CA certificate is not PEM".to_string());
    }
    if !bundle.key_pem.contains("PRIVATE KEY") {
        return Err("the backup's CA key is not PEM".to_string());
    }
    fs::create_dir_all(certs_dir).map_err(|e| format!("could not create certs dir: {e}"))?;
    let files = ca::root_ca_files(certs_dir);
    let mut backups = Vec::new();
    for path in [&files.cert, &files.key, &files.expiry] {
        if let Some(moved) = move_aside(path, timestamp)? {
            backups.push(moved);
        }
    }
    ca::write_private(&files.key, &bundle.key_pem)?;
    fs::write(&files.cert, &bundle.cert_pem).map_err(|e| format!("could not write root CA cert: {e}"))?;
    if let Some(not_after) = &bundle.not_after {
        fs::write(&files.expiry, not_after).map_err(|e| format!("could not write root CA expiry: {e}"))?;
    }
    Ok(backups)
}

/// What the user must do next. Import writes definitions only, and each site's
/// vhost, leaf cert and hosts entry come from the Sites tab: "Reissue certificate"
/// (`nginx_ssl_reissue_site_cert`) issues a leaf from the CA and renders/reloads
/// the vhost; disabling and re-enabling additionally resyncs /etc/hosts.
fn apply_note(imported: &[SectionCount], ca_written: bool) -> String {
    let sites = imported.iter().any(|s| s.section == Section::Sites && s.count > 0);
    let mut parts = Vec::new();
    if ca_written {
        parts.push("The root CA was replaced: run \"Install CA\" on the Nginx & SSL page so browsers trust it.".to_string());
    }
    if sites {
        parts.push(
            "Sites were imported as definitions only. On the Sites tab, use \"Reissue certificate\" on each site to issue its certificate and write its nginx config, then disable and re-enable it if its hostname does not resolve."
                .to_string(),
        );
    }
    if parts.is_empty() {
        "Nothing needs re-applying.".to_string()
    } else {
        parts.join(" ")
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn read_bundle_file(path: &str) -> Result<Bundle, String> {
    let meta = fs::metadata(path).map_err(|e| format!("could not read {path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{path} is not a file"));
    }
    if meta.len() > MAX_BYTES {
        return Err(format!("{path} is too large to be a DevsPro backup"));
    }
    let text = fs::read_to_string(path).map_err(|e| format!("could not read {path}: {e}"))?;
    parse(&text)
}

#[tauri::command]
pub fn backup_export(app: AppHandle, db: State<crate::db::Db>, path: String) -> Result<Inspection, String> {
    let certs_dir = super::nginx_ssl::certs_dir(&app)?;
    let bundle = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        collect(&conn, &certs_dir, chrono::Utc::now().to_rfc3339())?
    };
    let text = serialize(&bundle)?;
    fs::write(&path, text).map_err(|e| format!("could not write {path}: {e}"))?;
    Ok(inspect(&bundle))
}

#[tauri::command]
pub fn backup_inspect(path: String) -> Result<Inspection, String> {
    Ok(inspect(&read_bundle_file(&path)?))
}

#[tauri::command]
pub fn backup_import(
    app: AppHandle,
    db: State<crate::db::Db>,
    path: String,
    sections: Vec<String>,
    mode: Mode,
) -> Result<ImportSummary, String> {
    let sections = parse_sections(&sections)?;
    let bundle = read_bundle_file(&path)?;
    let wants_ca = sections.contains(&Section::Ca);
    if wants_ca && bundle.ca.is_none() {
        return Err("this backup has no root CA section".to_string());
    }

    let imported = {
        let mut conn = db.0.lock().map_err(|e| e.to_string())?;
        apply(&mut conn, &bundle, &sections, mode)?
    };

    let mut ca_backups = Vec::new();
    let mut imported = imported;
    if let (true, Some(ca_bundle)) = (wants_ca, &bundle.ca) {
        let certs_dir = super::nginx_ssl::certs_dir(&app)?;
        ca_backups = write_ca(&certs_dir, ca_bundle, &chrono::Utc::now().to_rfc3339())?;
        imported.push(SectionCount { section: Section::Ca, count: 1 });
    }

    let note = apply_note(&imported, wants_ca);
    Ok(ImportSummary { imported, ca_backups, needs_apply: true, note })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("devspro-backup-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn migrated_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn
    }

    fn site(id: &str, hostname: &str) -> SiteRow {
        SiteRow {
            id: id.into(),
            name: format!("Site {id}"),
            slug: format!("site-{id}"),
            hostname: hostname.into(),
            backend_type: "laravel".into(),
            port: None,
            project_path: Some("/home/me/app".into()),
            php_version: Some("8.3".into()),
            enabled: true,
            ssl_not_after: Some("2027-01-01T00:00:00Z".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
            limits: Some(r#"{"upload_max_mb":64}"#.into()),
        }
    }

    fn worker(id: &str, site_id: Option<&str>) -> WorkerRow {
        WorkerRow {
            id: id.into(),
            name: format!("worker {id}"),
            command: "php artisan queue:work".into(),
            working_dir: "/home/me/app".into(),
            site_id: site_id.map(String::from),
            created_at: "2026-01-02T00:00:00Z".into(),
        }
    }

    fn sample_bundle() -> Bundle {
        Bundle {
            format: FORMAT.into(),
            version: VERSION,
            exported_at: "2026-09-22T10:00:00Z".into(),
            sites: vec![site("a", "a.test"), site("b", "b.test")],
            workers: vec![worker("w1", Some("a")), worker("w2", None)],
            settings: BTreeMap::from([("close_to_tray".to_string(), "true".to_string())]),
            ca: Some(CaBundle {
                cert_pem: "-----BEGIN CERTIFICATE-----\nabc\n-----END CERTIFICATE-----\n".into(),
                key_pem: "-----BEGIN PRIVATE KEY-----\nxyz\n-----END PRIVATE KEY-----\n".into(),
                not_after: Some("2036-01-01".into()),
            }),
        }
    }

    #[test]
    fn serialize_then_parse_round_trips() {
        let bundle = sample_bundle();
        let text = serialize(&bundle).unwrap();
        assert_eq!(parse(&text).unwrap(), bundle);
    }

    #[test]
    fn parse_rejects_wrong_format_and_future_version() {
        assert!(parse("{").unwrap_err().contains("not valid JSON"));
        let err = parse(r#"{"format":"other","version":1,"exported_at":""}"#).unwrap_err();
        assert!(err.contains("not a DevsPro backup"), "{err}");
        let err = parse(r#"{"format":"devspro-backup","version":99,"exported_at":""}"#).unwrap_err();
        assert!(err.contains("version 99"), "{err}");
    }

    #[test]
    fn parse_tolerates_missing_and_unknown_sections() {
        let bundle = parse(
            r#"{"format":"devspro-backup","version":1,"exported_at":"x","ssh_hosts":[{"id":"1"}]}"#,
        )
        .unwrap();
        assert!(bundle.sites.is_empty());
        assert!(bundle.ca.is_none());
        let counts = inspect(&bundle);
        assert_eq!(counts.sections.len(), 4);
        assert!(counts.sections.iter().all(|s| s.count == 0));
    }

    #[test]
    fn inspect_counts_each_section() {
        let counts = inspect(&sample_bundle());
        let get = |s: Section| counts.sections.iter().find(|c| c.section == s).unwrap().count;
        assert_eq!(get(Section::Sites), 2);
        assert_eq!(get(Section::Workers), 2);
        assert_eq!(get(Section::Settings), 1);
        assert_eq!(get(Section::Ca), 1);
    }

    #[test]
    fn parse_sections_rejects_unknown_and_dedupes() {
        assert!(parse_sections(&[]).is_err());
        assert!(parse_sections(&["nope".to_string()]).unwrap_err().contains("nope"));
        let s = parse_sections(&["sites".to_string(), "sites".to_string(), "ca".to_string()]).unwrap();
        assert_eq!(s, vec![Section::Sites, Section::Ca]);
    }

    #[test]
    fn session_key_is_never_exported() {
        let conn = migrated_db();
        conn.execute("INSERT INTO settings (key, value) VALUES (?1, 'p1'), ('theme', 'dark')", [SESSION_SETTING_KEY])
            .unwrap();
        let settings = read_settings(&conn).unwrap();
        assert_eq!(settings.get("theme").map(String::as_str), Some("dark"));
        assert!(!settings.contains_key(SESSION_SETTING_KEY));
    }

    #[test]
    fn export_from_one_db_imports_into_a_fresh_one() {
        let tmp = TempDir::new();
        let certs = tmp.0.join("certs");
        let source = {
            let mut conn = migrated_db();
            apply(&mut conn, &sample_bundle(), &[Section::Sites, Section::Workers, Section::Settings], Mode::Replace)
                .unwrap();
            conn
        };
        write_ca(&certs, sample_bundle().ca.as_ref().unwrap(), "2026-09-22T10:00:00Z").unwrap();

        let exported = collect(&source, &certs, "now".into()).unwrap();
        assert_eq!(exported.sites, sample_bundle().sites);
        assert_eq!(exported.workers, sample_bundle().workers);
        assert_eq!(exported.settings, sample_bundle().settings);
        assert_eq!(exported.ca, sample_bundle().ca);

        let text = serialize(&exported).unwrap();
        let parsed = parse(&text).unwrap();
        let mut fresh = migrated_db();
        let imported = apply(&mut fresh, &parsed, &[Section::Sites, Section::Workers, Section::Settings], Mode::Merge).unwrap();
        assert_eq!(imported.iter().map(|c| c.count).sum::<usize>(), 5);
        assert_eq!(read_sites(&fresh).unwrap(), exported.sites);
        assert_eq!(read_workers(&fresh).unwrap(), exported.workers);
        assert_eq!(read_settings(&fresh).unwrap(), exported.settings);
    }

    #[test]
    fn merge_upserts_by_id_and_keeps_other_rows() {
        let mut conn = migrated_db();
        let mut existing = sample_bundle();
        existing.sites.push(site("c", "c.test"));
        apply(&mut conn, &existing, &[Section::Sites], Mode::Replace).unwrap();

        let mut update = sample_bundle();
        update.sites[0].name = "renamed".into();
        apply(&mut conn, &update, &[Section::Sites], Mode::Merge).unwrap();

        let sites = read_sites(&conn).unwrap();
        assert_eq!(sites.len(), 3);
        assert_eq!(sites.iter().find(|s| s.id == "a").unwrap().name, "renamed");
        assert!(sites.iter().any(|s| s.id == "c"));
    }

    #[test]
    fn replace_drops_rows_not_in_bundle_but_keeps_session_setting() {
        let mut conn = migrated_db();
        let mut existing = sample_bundle();
        existing.sites.push(site("c", "c.test"));
        apply(&mut conn, &existing, &[Section::Sites, Section::Settings], Mode::Replace).unwrap();
        conn.execute("INSERT INTO settings (key, value) VALUES (?1, 'p1')", [SESSION_SETTING_KEY]).unwrap();

        apply(&mut conn, &sample_bundle(), &[Section::Sites, Section::Settings], Mode::Replace).unwrap();
        assert_eq!(read_sites(&conn).unwrap().len(), 2);
        let session: String = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [SESSION_SETTING_KEY], |r| r.get(0))
            .unwrap();
        assert_eq!(session, "p1");
    }

    #[test]
    fn merge_hostname_clash_fails_readably_and_rolls_back() {
        let mut conn = migrated_db();
        apply(&mut conn, &sample_bundle(), &[Section::Sites], Mode::Replace).unwrap();
        let mut clash = sample_bundle();
        clash.sites = vec![site("z", "zz.test"), site("other-id", "a.test")];
        let err = apply(&mut conn, &clash, &[Section::Sites], Mode::Merge).unwrap_err();
        assert!(err.contains("a.test"), "{err}");
        // The whole transaction rolled back: "z" must not have landed.
        assert!(!read_sites(&conn).unwrap().iter().any(|s| s.id == "z"));
    }

    #[test]
    fn write_ca_backs_up_existing_files_first() {
        let tmp = TempDir::new();
        let certs = tmp.0.join("certs");
        let ca_bundle = sample_bundle().ca.unwrap();
        assert!(write_ca(&certs, &ca_bundle, "t1").unwrap().is_empty());

        let mut newer = ca_bundle.clone();
        newer.cert_pem = "-----BEGIN CERTIFICATE-----\nnew\n-----END CERTIFICATE-----\n".into();
        let backups = write_ca(&certs, &newer, "2026-09-22T10:00:00Z").unwrap();
        assert_eq!(backups.len(), 3);
        assert!(backups[0].ends_with("rootCA.pem.bak-20260922100000"), "{}", backups[0]);
        assert_eq!(read_ca(&certs).unwrap().unwrap(), newer);
        assert!(fs::read_to_string(&backups[0]).unwrap().contains("abc"));
    }

    #[test]
    fn write_ca_rejects_non_pem() {
        let tmp = TempDir::new();
        let bad = CaBundle { cert_pem: "nope".into(), key_pem: "nope".into(), not_after: None };
        assert!(write_ca(&tmp.0, &bad, "t").is_err());
    }

    #[test]
    fn note_names_the_reapply_steps() {
        let note = apply_note(&[SectionCount { section: Section::Sites, count: 2 }], true);
        assert!(note.contains("Reissue certificate"));
        assert!(note.contains("Install CA"));
        assert_eq!(apply_note(&[], false), "Nothing needs re-applying.");
    }
}
