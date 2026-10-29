//! Spins up databases (+ optional admin UI) as containers in DevsPro's *own*
//! docker-compose space — a single compose file this app fully owns, under
//! `<app-data>/docker/docker-compose.yml`, run under the fixed project name
//! "devspro" (`docker compose -p devspro ...`). Deliberately separate from
//! whatever other compose stacks the user already has (e.g. `~/docker/databases`)
//! — this is a sandbox DevsPro manages end-to-end, not something that edits
//! someone else's existing file the way the Docker module's port-change feature
//! does.
//!
//! Since DevsPro writes every line of this file itself, service blocks are
//! appended/removed with plain text manipulation rather than a YAML library —
//! there's no unknown external formatting to preserve, unlike the port-rewrite
//! feature in the Docker module which edits a compose file DevsPro doesn't own.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tauri::Manager;

pub mod browse;
pub mod connections;
pub mod edit;
pub mod export;
pub mod history;
pub mod schema;

pub(crate) const PROJECT_NAME: &str = "devspro";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DbKind {
    Mysql,
    Postgres,
    Mongo,
    Redis,
}

struct ServiceSpec {
    image: String,
    env: Vec<(String, String)>,
    volume_target: Option<String>,
    command: Option<String>,
    container_port: u16,
}

fn image_repo(kind: DbKind) -> &'static str {
    match kind {
        DbKind::Mysql => "mysql",
        DbKind::Postgres => "postgres",
        DbKind::Mongo => "mongo",
        DbKind::Redis => "redis",
    }
}

fn db_spec(name: &str, kind: DbKind, version: &str, username: Option<&str>, password: &str) -> ServiceSpec {
    let image = format!("{}:{version}", image_repo(kind));
    match kind {
        DbKind::Mysql => {
            let mut env = vec![
                ("MYSQL_ROOT_PASSWORD".to_string(), password.to_string()),
                ("MYSQL_DATABASE".to_string(), name.to_string()),
            ];
            if let Some(user) = username.filter(|u| !u.is_empty() && *u != "root") {
                env.push(("MYSQL_USER".to_string(), user.to_string()));
                env.push(("MYSQL_PASSWORD".to_string(), password.to_string()));
            }
            ServiceSpec {
                image,
                env,
                volume_target: Some("/var/lib/mysql".to_string()),
                command: None,
                container_port: 3306,
            }
        }
        DbKind::Postgres => {
            let user = username.filter(|u| !u.is_empty()).unwrap_or("postgres");
            ServiceSpec {
                image,
                env: vec![
                    ("POSTGRES_USER".to_string(), user.to_string()),
                    ("POSTGRES_PASSWORD".to_string(), password.to_string()),
                    ("POSTGRES_DB".to_string(), name.to_string()),
                ],
                volume_target: Some("/var/lib/postgresql/data".to_string()),
                command: None,
                container_port: 5432,
            }
        }
        DbKind::Mongo => {
            let user = username.filter(|u| !u.is_empty()).unwrap_or("root");
            ServiceSpec {
                image,
                env: vec![
                    ("MONGO_INITDB_ROOT_USERNAME".to_string(), user.to_string()),
                    ("MONGO_INITDB_ROOT_PASSWORD".to_string(), password.to_string()),
                ],
                volume_target: Some("/data/db".to_string()),
                command: None,
                container_port: 27017,
            }
        }
        DbKind::Redis => ServiceSpec {
            image,
            env: vec![],
            volume_target: Some("/data".to_string()),
            command: Some(format!("redis-server --requirepass {password}")),
            container_port: 6379,
        },
    }
}

fn ui_spec(db_name: &str, kind: DbKind, username: Option<&str>, password: &str) -> Option<(String, ServiceSpec)> {
    let ui_name = format!("{db_name}_ui");
    let spec = match kind {
        // Adminer deliberately never accepts a password via env var or URL —
        // that's an intentional Adminer security choice, not something this app
        // can work around. ADMINER_DEFAULT_SERVER pre-fills the server field;
        // username/db get pre-filled via query string on the "Open" link
        // instead (see the frontend), and the password still has to be typed
        // once when the login page loads.
        DbKind::Mysql | DbKind::Postgres => {
            let server = if matches!(kind, DbKind::Postgres) {
                format!("pgsql://{db_name}")
            } else {
                db_name.to_string()
            };
            ServiceSpec {
                image: "adminer:latest".to_string(),
                env: vec![("ADMINER_DEFAULT_SERVER".to_string(), server)],
                volume_target: None,
                command: None,
                container_port: 8080,
            }
        }
        DbKind::Mongo => {
            let user = username.filter(|u| !u.is_empty()).unwrap_or("root");
            ServiceSpec {
                image: "mongo-express:latest".to_string(),
                env: vec![
                    ("ME_CONFIG_MONGODB_SERVER".to_string(), db_name.to_string()),
                    ("ME_CONFIG_MONGODB_ADMINUSERNAME".to_string(), user.to_string()),
                    ("ME_CONFIG_MONGODB_ADMINPASSWORD".to_string(), password.to_string()),
                    // mongo-express's own web UI login (separate from the Mongo
                    // connection itself) — set to the same credentials so there's
                    // only one username/password to remember, not two.
                    ("ME_CONFIG_BASICAUTH_USERNAME".to_string(), user.to_string()),
                    ("ME_CONFIG_BASICAUTH_PASSWORD".to_string(), password.to_string()),
                ],
                volume_target: None,
                command: None,
                container_port: 8081,
            }
        }
        DbKind::Redis => ServiceSpec {
            image: "rediscommander/redis-commander:latest".to_string(),
            env: vec![
                ("REDIS_HOST".to_string(), db_name.to_string()),
                ("REDIS_PASSWORD".to_string(), password.to_string()),
            ],
            volume_target: None,
            command: None,
            container_port: 8081,
        },
    };
    Some((ui_name, spec))
}

fn render_service_block(name: &str, host_port: u16, spec: &ServiceSpec) -> String {
    let mut s = format!(
        "  {name}:\n    image: {}\n    container_name: devspro_{name}\n    restart: unless-stopped\n",
        spec.image
    );
    if !spec.env.is_empty() {
        s.push_str("    environment:\n");
        for (k, v) in &spec.env {
            s.push_str(&format!("      {k}: \"{v}\"\n"));
        }
    }
    if let Some(cmd) = &spec.command {
        s.push_str(&format!("    command: {cmd}\n"));
    }
    s.push_str(&format!("    ports:\n      - \"{host_port}:{}\"\n", spec.container_port));
    if let Some(target) = &spec.volume_target {
        s.push_str(&format!("    volumes:\n      - {}:{target}\n", volume_source(name)));
    }
    s
}

/// Where a database keeps its data: a folder beside the compose file on Unix.
#[cfg(not(windows))]
fn volume_source(name: &str) -> String {
    format!("./volumes/{name}_data")
}

/// On Windows, a Docker named volume instead. Docker Desktop's containers run in a Linux
/// VM, and a bind mount of a Windows folder reaches them through a file-sharing layer
/// that has no real Unix ownership or modes: Postgres refuses to initialise a data
/// directory it can't `chmod 0700`, and MySQL is many times slower on one. A named
/// volume lives inside the VM's own filesystem, which is what Docker recommends for
/// database storage on Windows.
#[cfg(windows)]
fn volume_source(name: &str) -> String {
    named_volume(name)
}

/// The compose-level name of `name`'s volume. Hyphenated because database names can't
/// contain a hyphen (`valid_name`), so a volume key can never be mistaken for a service
/// key by `service_block_text` — `shop_data` could be a database, `shop-data` can't.
#[cfg(windows)]
fn named_volume(name: &str) -> String {
    format!("{name}-data")
}

/// Declares `volume` in the compose file's top-level `volumes:` section, creating the
/// section at the top of the file if there isn't one. At the top rather than the end
/// because new services are appended at the end, and they must stay under `services:`.
#[cfg(any(windows, test))]
fn declare_volume(contents: &str, volume: &str) -> String {
    let entry = format!("  {volume}:");
    let lines: Vec<&str> = contents.lines().collect();
    let Some(header) = lines.iter().position(|l| l.trim_end() == "volumes:") else {
        return format!("volumes:\n{entry}\n{contents}");
    };
    let section_end = lines[(header + 1)..]
        .iter()
        .position(|l| !l.trim().is_empty() && indent_of(l) == 0)
        .map(|i| header + 1 + i)
        .unwrap_or(lines.len());
    if lines[header + 1..section_end].iter().any(|l| l.trim_end() == entry) {
        return contents.to_string();
    }
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 1);
    out.extend(&lines[..=header]);
    out.push(&entry);
    out.extend(&lines[header + 1..]);
    out.join("\n") + "\n"
}

/// Removes `volume`'s declaration from the top-level `volumes:` section, if it has one.
/// The section header stays even when it empties: compose reads a bare `volumes:` as no
/// volumes, the same way it reads the bare `services:` a fresh file starts with.
#[cfg(any(windows, test))]
fn undeclare_volume(contents: &str, volume: &str) -> String {
    let entry = format!("  {volume}:");
    let mut lines: Vec<&str> = contents.lines().collect();
    let Some(header) = lines.iter().position(|l| l.trim_end() == "volumes:") else {
        return contents.to_string();
    };
    let Some(at) = lines[header + 1..]
        .iter()
        .take_while(|l| l.trim().is_empty() || indent_of(l) > 0)
        .position(|l| l.trim_end() == entry)
    else {
        return contents.to_string();
    };
    lines.remove(header + 1 + at);
    lines.join("\n") + "\n"
}

fn random_password() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..16].to_string()
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
}

fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 40
        && version.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

fn valid_username(username: &str) -> bool {
    !username.is_empty() && username.len() <= 32 && username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// Values land inside a double-quoted YAML string (and, for Redis, directly on a
// shell command line) — restricted to a charset that can't break either.
fn valid_password(password: &str) -> bool {
    !password.is_empty()
        && password.len() <= 72
        && password
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.!@#%^*+=".contains(c))
}

fn compose_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("docker");
    fs::create_dir_all(dir.join("volumes")).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn compose_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let file = compose_dir(app)?.join("docker-compose.yml");
    if !file.exists() {
        fs::write(&file, "services:\n").map_err(|e| e.to_string())?;
    }
    Ok(file)
}

fn indent_of(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

fn service_block_text(contents: &str, service: &str) -> Option<String> {
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines.iter().position(|l| l.trim_start().trim_end() == format!("{service}:"))?;
    let base_indent = indent_of(lines[start]);
    let end = lines[(start + 1)..]
        .iter()
        .position(|l| !l.trim().is_empty() && indent_of(l) <= base_indent)
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());
    Some(lines[start..end].join("\n"))
}

fn remove_service_block(contents: &str, service: &str) -> String {
    let lines: Vec<&str> = contents.lines().collect();
    let Some(start) = lines.iter().position(|l| l.trim_start().trim_end() == format!("{service}:")) else {
        return contents.to_string();
    };
    let base_indent = indent_of(lines[start]);
    let end = lines[(start + 1)..]
        .iter()
        .position(|l| !l.trim().is_empty() && indent_of(l) <= base_indent)
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());
    let mut out: Vec<&str> = Vec::new();
    out.extend(&lines[..start]);
    out.extend(&lines[end..]);
    let mut result = out.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }
    result
}

fn extract_env_value(block: &str, key: &str) -> Option<String> {
    let idx = block.find(key)?;
    let rest = &block[idx..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start().trim_start_matches('"');
    let end = after.find('"').unwrap_or(after.len());
    Some(after[..end].trim().to_string())
}

fn extract_password_from_block(block: &str) -> Option<String> {
    for key in ["MYSQL_ROOT_PASSWORD", "POSTGRES_PASSWORD", "MONGO_INITDB_ROOT_PASSWORD"] {
        if let Some(v) = extract_env_value(block, key) {
            return Some(v);
        }
    }
    if let Some(idx) = block.find("--requirepass") {
        let rest = block[idx + "--requirepass".len()..].trim_start();
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        return Some(rest[..end].to_string());
    }
    None
}

fn extract_username_from_block(block: &str) -> Option<String> {
    for key in ["MYSQL_USER", "POSTGRES_USER", "MONGO_INITDB_ROOT_USERNAME"] {
        if let Some(v) = extract_env_value(block, key) {
            return Some(v);
        }
    }
    None
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDatabaseInput {
    pub name: String,
    pub kind: DbKind,
    pub version: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub with_ui: bool,
    pub ui_port: Option<u16>,
}

#[derive(Debug, Serialize)]
pub struct CreateDatabaseResult {
    pub message: String,
    pub username: Option<String>,
    pub password: String,
    pub db_port: u16,
    pub ui_port: Option<u16>,
}

#[tauri::command(async)]
pub fn databases_create(app: tauri::AppHandle, input: CreateDatabaseInput) -> Result<CreateDatabaseResult, String> {
    let name = input.name.trim().to_lowercase().replace('-', "_");
    if !valid_name(&name) {
        return Err(
            "name must start with a lowercase letter and contain only lowercase letters, numbers, and underscores"
                .to_string(),
        );
    }
    let version = input.version.trim();
    if !valid_version(version) {
        return Err("version can only contain letters, numbers, dots, dashes, and underscores".to_string());
    }
    let username = match input.username.as_deref().map(str::trim) {
        Some(u) if !u.is_empty() => {
            if !valid_username(u) {
                return Err("username must contain only letters, numbers, and underscores".to_string());
            }
            Some(u.to_string())
        }
        _ => None,
    };
    let password = match input.password.as_deref().map(str::trim) {
        Some(p) if !p.is_empty() => {
            if !valid_password(p) {
                return Err("password can only contain letters, numbers, and - _ . ! @ # % ^ * + =".to_string());
            }
            p.to_string()
        }
        _ => random_password(),
    };

    let file = compose_file(&app)?;
    let existing = fs::read_to_string(&file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
    if service_block_text(&existing, &name).is_some() {
        return Err(format!("a database named '{name}' already exists in this space"));
    }

    let spec = db_spec(&name, input.kind, version, username.as_deref(), &password);
    let mut appended = render_service_block(&name, input.port, &spec);

    let mut services_to_start = vec![name.clone()];
    let mut ui_port_out = None;
    if input.with_ui {
        if let Some((ui_name, ui_spec)) = ui_spec(&name, input.kind, username.as_deref(), &password) {
            let ui_port = input.ui_port.unwrap_or(input.port + 1);
            appended.push_str(&render_service_block(&ui_name, ui_port, &ui_spec));
            services_to_start.push(ui_name);
            ui_port_out = Some(ui_port);
        }
    }

    let mut new_content = existing;
    if !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&appended);
    #[cfg(windows)]
    if spec.volume_target.is_some() {
        new_content = declare_volume(&new_content, &named_volume(&name));
    }
    fs::write(&file, new_content).map_err(|e| format!("could not write {}: {e}", file.display()))?;

    let file_str = file.to_string_lossy().to_string();
    let mut args: Vec<&str> = vec!["-p", PROJECT_NAME, "-f", &file_str, "up", "-d"];
    args.extend(services_to_start.iter().map(String::as_str));
    let output = super::compose::build(&args)?
        .output()
        .map_err(|e| format!("failed to run docker compose: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "wrote the compose file but starting containers failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(CreateDatabaseResult {
        message: format!("{name} is starting on port {}", input.port),
        username,
        password,
        db_port: input.port,
        ui_port: ui_port_out,
    })
}

#[derive(Debug, Deserialize)]
struct RawContainer {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Names")]
    names: String,
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "State")]
    state: String,
    #[serde(rename = "Status")]
    status: String,
    #[serde(rename = "Ports")]
    ports: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct DevsproContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub ports: String,
}

#[tauri::command(async)]
pub fn databases_list() -> Result<Vec<DevsproContainer>, String> {
    let output = crate::modules::tools::command("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("label=com.docker.compose.project={PROJECT_NAME}"),
            "--format",
            "{{json .}}",
        ])
        .output()
        .map_err(|e| format!("failed to run docker: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let raw: RawContainer =
                serde_json::from_str(line).map_err(|e| format!("failed to parse docker output: {e}"))?;
            Ok(DevsproContainer {
                id: raw.id,
                name: raw.names,
                image: raw.image,
                state: raw.state,
                status: raw.status,
                ports: raw.ports,
            })
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct Credentials {
    pub username: Option<String>,
    pub password: Option<String>,
}

#[tauri::command(async)]
pub fn databases_get_credentials(app: tauri::AppHandle, name: String) -> Result<Credentials, String> {
    let file = compose_file(&app)?;
    let contents = fs::read_to_string(&file).unwrap_or_default();
    let block = service_block_text(&contents, &name);
    Ok(Credentials {
        username: block.as_deref().and_then(extract_username_from_block),
        password: block.as_deref().and_then(extract_password_from_block),
    })
}

#[tauri::command(async)]
pub fn databases_remove(app: tauri::AppHandle, name: String, delete_data: bool) -> Result<String, String> {
    let file = compose_file(&app)?;
    let existing = fs::read_to_string(&file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
    let ui_name = format!("{name}_ui");
    let has_ui = service_block_text(&existing, &ui_name).is_some();

    let file_str = file.to_string_lossy().to_string();
    let mut args: Vec<&str> = vec!["-p", PROJECT_NAME, "-f", &file_str, "rm", "-sf", &name];
    if has_ui {
        args.push(&ui_name);
    }
    let output = super::compose::build(&args)?
        .output()
        .map_err(|e| format!("failed to run docker compose: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let mut updated = remove_service_block(&existing, &name);
    if has_ui {
        updated = remove_service_block(&updated, &ui_name);
    }
    #[cfg(windows)]
    {
        updated = undeclare_volume(&updated, &named_volume(&name));
    }
    fs::write(&file, updated).map_err(|e| format!("could not write {}: {e}", file.display()))?;

    if delete_data {
        let dir = compose_dir(&app)?;
        // Also on Windows: a database created before named volumes were used there
        // still keeps its data in this folder.
        let _ = fs::remove_dir_all(dir.join("volumes").join(format!("{name}_data")));
        // Compose prefixes the volumes it creates with the project name. Best-effort,
        // like the folder above: there is no volume for a database made the old way.
        #[cfg(windows)]
        let _ = crate::modules::tools::command("docker")
            .args(["volume", "rm", &format!("{PROJECT_NAME}_{}", named_volume(&name))])
            .output();
    }

    Ok(format!("removed {name}"))
}

// ---------------------------------------------------------------------------
// Per-engine user management. Compose env vars only set the *initial* admin
// user at first container creation — they have no effect on an already-running
// database — so adding/removing additional users has to actually talk to the
// running engine. Done via `docker exec <container> <client-cli> ...` using
// whichever client each official image already bundles (mysql/psql/mongosh/
// redis-cli), rather than pulling in a Rust DB driver crate per engine.

fn container_name(base: &str) -> String {
    format!("devspro_{base}")
}

fn docker_exec(container: &str, args: &[&str]) -> Result<std::process::Output, String> {
    let mut full: Vec<&str> = vec!["exec", container];
    full.extend(args);
    crate::modules::tools::command("docker")
        .args(&full)
        .output()
        .map_err(|e| format!("failed to run docker exec: {e}"))
}

/// Determines the engine and admin credentials for an existing database purely
/// from its own compose service block — no separate metadata store needed,
/// consistent with how `databases_get_credentials` already works.
fn admin_context(app: &tauri::AppHandle, base: &str) -> Result<(DbKind, String, String), String> {
    let file = compose_file(app)?;
    let contents = fs::read_to_string(&file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
    let block = service_block_text(&contents, base)
        .ok_or_else(|| format!("could not find '{base}' in the compose file"))?;

    let kind = if block.contains("MYSQL_ROOT_PASSWORD") {
        DbKind::Mysql
    } else if block.contains("POSTGRES_PASSWORD") {
        DbKind::Postgres
    } else if block.contains("MONGO_INITDB_ROOT_PASSWORD") {
        DbKind::Mongo
    } else if block.contains("--requirepass") {
        DbKind::Redis
    } else {
        return Err(format!("could not determine the database type for '{base}'"));
    };

    let admin_password =
        extract_password_from_block(&block).ok_or_else(|| "could not find the admin password".to_string())?;
    let admin_username = match kind {
        DbKind::Mysql => "root".to_string(),
        DbKind::Postgres => extract_username_from_block(&block).unwrap_or_else(|| "postgres".to_string()),
        DbKind::Mongo => extract_username_from_block(&block).unwrap_or_else(|| "root".to_string()),
        DbKind::Redis => String::new(),
    };
    Ok((kind, admin_username, admin_password))
}

fn command_failed(output: &std::process::Output) -> Option<String> {
    if output.status.success() {
        None
    } else {
        let mut msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if msg.is_empty() {
            msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
        Some(msg)
    }
}

fn mysql_list_users(container: &str, admin_password: &str) -> Result<Vec<String>, String> {
    let out = docker_exec(
        container,
        &[
            "mysql",
            "-uroot",
            &format!("-p{admin_password}"),
            "-N",
            "-e",
            "SELECT User FROM mysql.user WHERE Host='%' AND User != 'root';",
        ],
    )?;
    if let Some(err) = command_failed(&out) {
        return Err(err);
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

fn mysql_create_user(container: &str, admin_password: &str, db: &str, user: &str, pass: &str) -> Result<(), String> {
    let sql =
        format!("CREATE USER '{user}'@'%' IDENTIFIED BY '{pass}'; GRANT ALL PRIVILEGES ON `{db}`.* TO '{user}'@'%'; FLUSH PRIVILEGES;");
    let out = docker_exec(container, &["mysql", "-uroot", &format!("-p{admin_password}"), "-e", &sql])?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn mysql_drop_user(container: &str, admin_password: &str, user: &str) -> Result<(), String> {
    let sql = format!("DROP USER IF EXISTS '{user}'@'%'; FLUSH PRIVILEGES;");
    let out = docker_exec(container, &["mysql", "-uroot", &format!("-p{admin_password}"), "-e", &sql])?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn postgres_list_users(container: &str, admin_user: &str) -> Result<Vec<String>, String> {
    let out = docker_exec(
        container,
        &[
            "psql",
            "-U",
            admin_user,
            "-d",
            "postgres",
            "-t",
            "-A",
            "-c",
            "SELECT usename FROM pg_catalog.pg_user WHERE usename != current_user;",
        ],
    )?;
    if let Some(err) = command_failed(&out) {
        return Err(err);
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

fn postgres_create_user(container: &str, admin_user: &str, db: &str, user: &str, pass: &str) -> Result<(), String> {
    let sql = format!(
        "CREATE USER \"{user}\" WITH PASSWORD '{pass}'; GRANT ALL PRIVILEGES ON DATABASE \"{db}\" TO \"{user}\";"
    );
    let out = docker_exec(container, &["psql", "-U", admin_user, "-d", "postgres", "-c", &sql])?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn postgres_drop_user(container: &str, admin_user: &str, user: &str) -> Result<(), String> {
    let sql = format!("DROP USER IF EXISTS \"{user}\";");
    let out = docker_exec(container, &["psql", "-U", admin_user, "-d", "postgres", "-c", &sql])?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn mongo_list_users(container: &str, admin_user: &str, admin_password: &str, db: &str) -> Result<Vec<String>, String> {
    let eval = format!("db.getSiblingDB('{db}').getUsers().forEach(u => print(u.user))");
    let out = docker_exec(
        container,
        &[
            "mongosh",
            "-u",
            admin_user,
            "-p",
            admin_password,
            "--authenticationDatabase",
            "admin",
            "--quiet",
            "--eval",
            &eval,
        ],
    )?;
    if let Some(err) = command_failed(&out) {
        return Err(err);
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

fn mongo_create_user(
    container: &str,
    admin_user: &str,
    admin_password: &str,
    db: &str,
    user: &str,
    pass: &str,
) -> Result<(), String> {
    let eval = format!(
        "db.getSiblingDB('{db}').createUser({{user: '{user}', pwd: '{pass}', roles: [{{role: 'readWrite', db: '{db}'}}]}})"
    );
    let out = docker_exec(
        container,
        &[
            "mongosh",
            "-u",
            admin_user,
            "-p",
            admin_password,
            "--authenticationDatabase",
            "admin",
            "--quiet",
            "--eval",
            &eval,
        ],
    )?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn mongo_drop_user(container: &str, admin_user: &str, admin_password: &str, db: &str, user: &str) -> Result<(), String> {
    let eval = format!("db.getSiblingDB('{db}').dropUser('{user}')");
    let out = docker_exec(
        container,
        &[
            "mongosh",
            "-u",
            admin_user,
            "-p",
            admin_password,
            "--authenticationDatabase",
            "admin",
            "--quiet",
            "--eval",
            &eval,
        ],
    )?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn redis_list_users(container: &str, admin_password: &str) -> Result<Vec<String>, String> {
    let out = docker_exec(container, &["redis-cli", "-a", admin_password, "--no-auth-warning", "ACL", "LIST"])?;
    if let Some(err) = command_failed(&out) {
        return Err(err);
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
        .filter(|u| u != "default")
        .collect())
}

fn redis_create_user(container: &str, admin_password: &str, user: &str, pass: &str) -> Result<(), String> {
    let out = docker_exec(
        container,
        &[
            "redis-cli",
            "-a",
            admin_password,
            "--no-auth-warning",
            "ACL",
            "SETUSER",
            user,
            "on",
            &format!(">{pass}"),
            "~*",
            "+@all",
        ],
    )?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn redis_drop_user(container: &str, admin_password: &str, user: &str) -> Result<(), String> {
    let out = docker_exec(container, &["redis-cli", "-a", admin_password, "--no-auth-warning", "ACL", "DELUSER", user])?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn mysql_change_password(container: &str, admin_password: &str, user: &str, new_password: &str) -> Result<(), String> {
    let sql = format!("ALTER USER '{user}'@'%' IDENTIFIED BY '{new_password}'; FLUSH PRIVILEGES;");
    let out = docker_exec(container, &["mysql", "-uroot", &format!("-p{admin_password}"), "-e", &sql])?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn postgres_change_password(container: &str, admin_user: &str, user: &str, new_password: &str) -> Result<(), String> {
    let sql = format!("ALTER USER \"{user}\" WITH PASSWORD '{new_password}';");
    let out = docker_exec(container, &["psql", "-U", admin_user, "-d", "postgres", "-c", &sql])?;
    command_failed(&out).map_or(Ok(()), Err)
}

/// `auth_db` is "admin" for the root user (that's where MongoDB keeps it) or the
/// app database name for a user created scoped to that database.
fn mongo_change_password(
    container: &str,
    admin_user: &str,
    admin_password: &str,
    auth_db: &str,
    user: &str,
    new_password: &str,
) -> Result<(), String> {
    let eval = format!("db.getSiblingDB('{auth_db}').changeUserPassword('{user}', '{new_password}')");
    let out = docker_exec(
        container,
        &[
            "mongosh",
            "-u",
            admin_user,
            "-p",
            admin_password,
            "--authenticationDatabase",
            "admin",
            "--quiet",
            "--eval",
            &eval,
        ],
    )?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn redis_change_user_password(container: &str, admin_password: &str, user: &str, new_password: &str) -> Result<(), String> {
    let out = docker_exec(
        container,
        &["redis-cli", "-a", admin_password, "--no-auth-warning", "ACL", "SETUSER", user, &format!(">{new_password}")],
    )?;
    command_failed(&out).map_or(Ok(()), Err)
}

/// Live-updates Redis's own `requirepass` (works immediately, no restart) —
/// callers still need to persist the new value into the compose file's
/// `command:` line so a future restart doesn't revert to the old password.
fn redis_change_requirepass(container: &str, admin_password: &str, new_password: &str) -> Result<(), String> {
    let out = docker_exec(
        container,
        &["redis-cli", "-a", admin_password, "--no-auth-warning", "CONFIG", "SET", "requirepass", new_password],
    )?;
    command_failed(&out).map_or(Ok(()), Err)
}

/// Rewrites a `KEY: "value"` environment line inside `service`'s block,
/// preserving indentation — the write-side counterpart to `extract_env_value`.
fn set_env_value(contents: &str, service: &str, key: &str, new_value: &str) -> Result<String, String> {
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim_start().trim_end() == format!("{service}:"))
        .ok_or_else(|| format!("could not find service '{service}' in the compose file"))?;
    let base_indent = indent_of(lines[start]);
    let end = lines[(start + 1)..]
        .iter()
        .position(|l| !l.trim().is_empty() && indent_of(l) <= base_indent)
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());

    let target_idx = lines[(start + 1)..end]
        .iter()
        .position(|l| {
            let trimmed = l.trim_start();
            trimmed.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with(':'))
        })
        .map(|i| start + 1 + i)
        .ok_or_else(|| format!("could not find {key} for service '{service}'"))?;

    let leading_ws = &lines[target_idx][..indent_of(lines[target_idx])];
    let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    new_lines[target_idx] = format!("{leading_ws}{key}: \"{new_value}\"");
    Ok(new_lines.join("\n") + "\n")
}

/// Same idea as `set_env_value` but for Redis's `command: redis-server
/// --requirepass <password>` line, which isn't a `KEY: value` environment entry.
fn set_redis_requirepass(contents: &str, service: &str, new_password: &str) -> Result<String, String> {
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim_start().trim_end() == format!("{service}:"))
        .ok_or_else(|| format!("could not find service '{service}' in the compose file"))?;
    let base_indent = indent_of(lines[start]);
    let end = lines[(start + 1)..]
        .iter()
        .position(|l| !l.trim().is_empty() && indent_of(l) <= base_indent)
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());

    let target_idx = lines[(start + 1)..end]
        .iter()
        .position(|l| l.trim_start().starts_with("command:"))
        .map(|i| start + 1 + i)
        .ok_or_else(|| format!("could not find the command: line for service '{service}'"))?;

    let leading_ws = &lines[target_idx][..indent_of(lines[target_idx])];
    let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    new_lines[target_idx] = format!("{leading_ws}command: redis-server --requirepass {new_password}");
    Ok(new_lines.join("\n") + "\n")
}

#[tauri::command(async)]
pub fn databases_change_admin_password(app: tauri::AppHandle, name: String, new_password: String) -> Result<String, String> {
    let new_password = new_password.trim();
    if !valid_password(new_password) {
        return Err("password can only contain letters, numbers, and - _ . ! @ # % ^ * + =".to_string());
    }

    let (kind, admin_user, admin_password) = admin_context(&app, &name)?;
    let container = container_name(&name);
    let file = compose_file(&app)?;
    let existing = fs::read_to_string(&file).map_err(|e| format!("could not read {}: {e}", file.display()))?;

    let updated = match kind {
        DbKind::Mysql => {
            mysql_change_password(&container, &admin_password, "root", new_password)?;
            set_env_value(&existing, &name, "MYSQL_ROOT_PASSWORD", new_password)?
        }
        DbKind::Postgres => {
            postgres_change_password(&container, &admin_user, &admin_user, new_password)?;
            set_env_value(&existing, &name, "POSTGRES_PASSWORD", new_password)?
        }
        DbKind::Mongo => {
            mongo_change_password(&container, &admin_user, &admin_password, "admin", &admin_user, new_password)?;
            set_env_value(&existing, &name, "MONGO_INITDB_ROOT_PASSWORD", new_password)?
        }
        DbKind::Redis => {
            redis_change_requirepass(&container, &admin_password, new_password)?;
            set_redis_requirepass(&existing, &name, new_password)?
        }
    };
    fs::write(&file, updated).map_err(|e| format!("could not write {}: {e}", file.display()))?;

    Ok(format!("changed the admin password for '{name}'"))
}

#[tauri::command(async)]
pub fn databases_change_user_password(
    app: tauri::AppHandle,
    name: String,
    username: String,
    new_password: String,
) -> Result<String, String> {
    let new_password = new_password.trim();
    if !valid_password(new_password) {
        return Err("password can only contain letters, numbers, and - _ . ! @ # % ^ * + =".to_string());
    }

    let (kind, admin_user, admin_password) = admin_context(&app, &name)?;
    let container = container_name(&name);
    match kind {
        DbKind::Mysql => mysql_change_password(&container, &admin_password, &username, new_password),
        DbKind::Postgres => postgres_change_password(&container, &admin_user, &username, new_password),
        DbKind::Mongo => mongo_change_password(&container, &admin_user, &admin_password, &name, &username, new_password),
        DbKind::Redis => redis_change_user_password(&container, &admin_password, &username, new_password),
    }?;
    Ok(format!("changed password for '{username}'"))
}

#[tauri::command(async)]
pub fn databases_list_users(app: tauri::AppHandle, name: String) -> Result<Vec<String>, String> {
    let (kind, admin_user, admin_password) = admin_context(&app, &name)?;
    let container = container_name(&name);
    match kind {
        DbKind::Mysql => mysql_list_users(&container, &admin_password),
        DbKind::Postgres => postgres_list_users(&container, &admin_user),
        DbKind::Mongo => mongo_list_users(&container, &admin_user, &admin_password, &name),
        DbKind::Redis => redis_list_users(&container, &admin_password),
    }
}

#[tauri::command(async)]
pub fn databases_create_user(
    app: tauri::AppHandle,
    name: String,
    username: String,
    password: String,
) -> Result<String, String> {
    let username = username.trim();
    let password = password.trim();
    if !valid_username(username) {
        return Err("username must contain only letters, numbers, and underscores".to_string());
    }
    if !valid_password(password) {
        return Err("password can only contain letters, numbers, and - _ . ! @ # % ^ * + =".to_string());
    }

    let (kind, admin_user, admin_password) = admin_context(&app, &name)?;
    let container = container_name(&name);
    match kind {
        DbKind::Mysql => mysql_create_user(&container, &admin_password, &name, username, password),
        DbKind::Postgres => postgres_create_user(&container, &admin_user, &name, username, password),
        DbKind::Mongo => mongo_create_user(&container, &admin_user, &admin_password, &name, username, password),
        DbKind::Redis => redis_create_user(&container, &admin_password, username, password),
    }?;
    Ok(format!("created user '{username}'"))
}

// ---------------------------------------------------------------------------
// Creating a *schema* inside an existing server container — distinct from
// `databases_create`, which spins up a whole container. Laravel projects
// usually want their own database on a server that already exists, which is
// what the New Project wizard asks for. Same docker-exec-into-the-bundled-
// client approach as user management; no engine driver crates.
// ---------------------------------------------------------------------------

/// Schema names are interpolated into SQL identifiers (backticks / double
/// quotes), so the charset is kept narrow enough that quoting can't be broken.
fn valid_schema_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn mysql_create_database(container: &str, admin_password: &str, db: &str) -> Result<(), String> {
    let sql = format!("CREATE DATABASE IF NOT EXISTS `{db}`;");
    let out = docker_exec(container, &["mysql", "-uroot", &format!("-p{admin_password}"), "-e", &sql])?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn postgres_database_exists(container: &str, admin_user: &str, db: &str) -> Result<bool, String> {
    let sql = format!("SELECT 1 FROM pg_database WHERE datname = '{db}';");
    let out = docker_exec(container, &["psql", "-U", admin_user, "-d", "postgres", "-t", "-A", "-c", &sql])?;
    if let Some(err) = command_failed(&out) {
        return Err(err);
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim() == "1")
}

fn postgres_create_database(container: &str, admin_user: &str, db: &str) -> Result<(), String> {
    // Postgres has no CREATE DATABASE IF NOT EXISTS, hence the probe first;
    // re-running the wizard against the same name should be a no-op, not an
    // error, to match the MySQL branch.
    if postgres_database_exists(container, admin_user, db)? {
        return Ok(());
    }
    let sql = format!("CREATE DATABASE \"{db}\";");
    let out = docker_exec(container, &["psql", "-U", admin_user, "-d", "postgres", "-c", &sql])?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn mongo_create_database(container: &str, admin_user: &str, admin_password: &str, db: &str) -> Result<(), String> {
    // Mongo databases exist once they hold a collection; an empty placeholder
    // collection is the conventional way to make one appear in `show dbs`.
    let eval = format!(
        "const d = db.getSiblingDB('{db}'); if (!d.getCollectionNames().length) d.createCollection('_devspro');"
    );
    let out = docker_exec(
        container,
        &[
            "mongosh",
            "-u",
            admin_user,
            "-p",
            admin_password,
            "--authenticationDatabase",
            "admin",
            "--quiet",
            "--eval",
            &eval,
        ],
    )?;
    command_failed(&out).map_or(Ok(()), Err)
}

/// Creates a database (schema) named `name` on the DevsPro-managed server
/// `connection` (the compose service name, e.g. `mysql8`). Idempotent: an
/// existing database of that name is left alone and reported as such.
#[tauri::command(async)]
pub fn databases_create_database(app: tauri::AppHandle, connection: String, name: String) -> Result<String, String> {
    let name = name.trim();
    if !valid_schema_name(name) {
        return Err(
            "database name must be 1-64 letters, numbers or underscores and can't start with a digit".to_string(),
        );
    }
    let (kind, admin_user, admin_password) = admin_context(&app, &connection)?;
    let container = container_name(&connection);
    match kind {
        DbKind::Mysql => mysql_create_database(&container, &admin_password, name),
        DbKind::Postgres => postgres_create_database(&container, &admin_user, name),
        DbKind::Mongo => mongo_create_database(&container, &admin_user, &admin_password, name),
        DbKind::Redis => Err("Redis has no databases to create — it is a key-value store".to_string()),
    }?;
    Ok(format!("database '{name}' is ready on {connection}"))
}

/// The host-side port from a service block's `ports:` mapping (`"3307:3306"`
/// gives 3307). Only the first mapping counts; DevsPro writes exactly one.
fn extract_host_port(block: &str) -> Option<u16> {
    let idx = block.find("ports:")?;
    let rest = &block[idx + "ports:".len()..];
    let line = rest.lines().find(|l| l.trim_start().starts_with('-'))?;
    let mapping = line.trim().trim_start_matches('-').trim().trim_matches('"');
    mapping.split(':').next()?.trim().parse().ok()
}

/// What Laravel calls the driver for each engine in `config/database.php`.
/// Redis is not a `DB_CONNECTION` and is rejected upstream.
fn laravel_driver(kind: DbKind) -> &'static str {
    match kind {
        DbKind::Mysql => "mysql",
        DbKind::Postgres => "pgsql",
        DbKind::Mongo => "mongodb",
        DbKind::Redis => "redis",
    }
}

/// The `DB_*` keys a Laravel `.env` needs to reach `db_name` on the managed
/// server `connection`. Uses the admin credentials on purpose: a local dev
/// database owned by DevsPro has no second user to hand out, and the admin
/// password is already shown in the Databases page.
pub fn laravel_env_for(app: &tauri::AppHandle, connection: &str, db_name: &str) -> Result<Vec<(String, String)>, String> {
    let (kind, admin_user, admin_password) = admin_context(app, connection)?;
    let file = compose_file(app)?;
    let contents = fs::read_to_string(&file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
    let block = service_block_text(&contents, connection)
        .ok_or_else(|| format!("could not find '{connection}' in the compose file"))?;
    let port = extract_host_port(&block).ok_or_else(|| format!("could not find the host port for '{connection}'"))?;
    Ok(vec![
        ("DB_CONNECTION".to_string(), laravel_driver(kind).to_string()),
        ("DB_HOST".to_string(), "127.0.0.1".to_string()),
        ("DB_PORT".to_string(), port.to_string()),
        ("DB_DATABASE".to_string(), db_name.to_string()),
        ("DB_USERNAME".to_string(), admin_user),
        ("DB_PASSWORD".to_string(), admin_password),
    ])
}

#[tauri::command(async)]
pub fn databases_drop_user(app: tauri::AppHandle, name: String, username: String) -> Result<String, String> {
    let (kind, admin_user, admin_password) = admin_context(&app, &name)?;
    let container = container_name(&name);
    match kind {
        DbKind::Mysql => mysql_drop_user(&container, &admin_password, &username),
        DbKind::Postgres => postgres_drop_user(&container, &admin_user, &username),
        DbKind::Mongo => mongo_drop_user(&container, &admin_user, &admin_password, &name, &username),
        DbKind::Redis => redis_drop_user(&container, &admin_password, &username),
    }?;
    Ok(format!("removed user '{username}'"))
}

// ---------------------------------------------------------------------------
// Backup / restore. Same docker-exec-into-the-official-client pattern as user
// management above — no Rust DB driver crate, just the client binary each
// image already bundles. Dumps are piped straight to/from a file instead of
// being buffered as a String, since a real dump can be sizeable: the child's
// stdout/stdin is wired directly to a file handle rather than going through
// `.output()`, which would force everything through an in-memory Vec<u8>.
//
// Redis has no SQL-style dump to speak of. The practical equivalent is
// triggering a SAVE and `docker cp`-ing out the resulting dump.rdb; restoring
// means copying it back in and restarting the container, since Redis only
// loads dump.rdb from disk at startup.

pub fn backups_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("backups");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn backup_extension(kind: DbKind) -> &'static str {
    match kind {
        DbKind::Mysql | DbKind::Postgres => "sql",
        DbKind::Mongo => "archive.gz",
        DbKind::Redis => "rdb",
    }
}

fn backup_filename(name: &str, kind: DbKind, timestamp: &str) -> String {
    format!("{name}_{timestamp}.{}", backup_extension(kind))
}

fn docker_exec_to_file(container: &str, args: &[&str], file_path: &Path) -> Result<(), String> {
    let file = fs::File::create(file_path).map_err(|e| format!("could not create {}: {e}", file_path.display()))?;
    let mut full: Vec<&str> = vec!["exec", container];
    full.extend(args);
    let child = crate::modules::tools::command("docker")
        .args(&full)
        .stdout(Stdio::from(file))
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run docker exec: {e}"))?;
    let output = child.wait_with_output().map_err(|e| format!("failed to wait for docker exec: {e}"))?;
    if let Some(err) = command_failed(&output) {
        let _ = fs::remove_file(file_path);
        return Err(err);
    }
    Ok(())
}

fn docker_exec_from_file(container: &str, args: &[&str], file_path: &Path) -> Result<(), String> {
    let file = fs::File::open(file_path).map_err(|e| format!("could not open {}: {e}", file_path.display()))?;
    let mut full: Vec<&str> = vec!["exec", "-i", container];
    full.extend(args);
    let child = crate::modules::tools::command("docker")
        .args(&full)
        .stdin(Stdio::from(file))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run docker exec: {e}"))?;
    let output = child.wait_with_output().map_err(|e| format!("failed to wait for docker exec: {e}"))?;
    command_failed(&output).map_or(Ok(()), Err)
}

fn mysql_dump_args(admin_password: &str, db: &str) -> Vec<String> {
    vec!["mysqldump".to_string(), "-uroot".to_string(), format!("-p{admin_password}"), db.to_string()]
}

fn mysql_restore_args(admin_password: &str, db: &str) -> Vec<String> {
    vec!["mysql".to_string(), "-uroot".to_string(), format!("-p{admin_password}"), db.to_string()]
}

fn postgres_dump_args(admin_user: &str, db: &str) -> Vec<String> {
    vec!["pg_dump".to_string(), "-U".to_string(), admin_user.to_string(), "-d".to_string(), db.to_string()]
}

fn postgres_restore_args(admin_user: &str, db: &str) -> Vec<String> {
    vec!["psql".to_string(), "-U".to_string(), admin_user.to_string(), "-d".to_string(), db.to_string()]
}

fn mongo_dump_args(admin_user: &str, admin_password: &str, db: &str) -> Vec<String> {
    vec![
        "mongodump".to_string(),
        "-u".to_string(),
        admin_user.to_string(),
        "-p".to_string(),
        admin_password.to_string(),
        "--authenticationDatabase".to_string(),
        "admin".to_string(),
        "--db".to_string(),
        db.to_string(),
        "--archive".to_string(),
        "--gzip".to_string(),
    ]
}

// Restoring an archive produced with `--db <name>` already scopes it to that
// database — no `--nsInclude` needed, unlike a restore from a full-instance dump.
fn mongo_restore_args(admin_user: &str, admin_password: &str) -> Vec<String> {
    vec![
        "mongorestore".to_string(),
        "-u".to_string(),
        admin_user.to_string(),
        "-p".to_string(),
        admin_password.to_string(),
        "--authenticationDatabase".to_string(),
        "admin".to_string(),
        "--drop".to_string(),
        "--archive".to_string(),
        "--gzip".to_string(),
    ]
}

fn as_str_args(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

fn mysql_backup(container: &str, admin_password: &str, db: &str, file_path: &Path) -> Result<(), String> {
    docker_exec_to_file(container, &as_str_args(&mysql_dump_args(admin_password, db)), file_path)
}

fn mysql_restore(container: &str, admin_password: &str, db: &str, file_path: &Path) -> Result<(), String> {
    docker_exec_from_file(container, &as_str_args(&mysql_restore_args(admin_password, db)), file_path)
}

fn postgres_backup(container: &str, admin_user: &str, db: &str, file_path: &Path) -> Result<(), String> {
    docker_exec_to_file(container, &as_str_args(&postgres_dump_args(admin_user, db)), file_path)
}

fn postgres_restore(container: &str, admin_user: &str, db: &str, file_path: &Path) -> Result<(), String> {
    docker_exec_from_file(container, &as_str_args(&postgres_restore_args(admin_user, db)), file_path)
}

fn mongo_backup(
    container: &str,
    admin_user: &str,
    admin_password: &str,
    db: &str,
    file_path: &Path,
) -> Result<(), String> {
    docker_exec_to_file(container, &as_str_args(&mongo_dump_args(admin_user, admin_password, db)), file_path)
}

fn mongo_restore(container: &str, admin_user: &str, admin_password: &str, file_path: &Path) -> Result<(), String> {
    docker_exec_from_file(container, &as_str_args(&mongo_restore_args(admin_user, admin_password)), file_path)
}

fn redis_backup(container: &str, admin_password: &str, file_path: &Path) -> Result<(), String> {
    let out = docker_exec(container, &["redis-cli", "-a", admin_password, "--no-auth-warning", "SAVE"])?;
    if let Some(err) = command_failed(&out) {
        return Err(err);
    }
    let out = crate::modules::tools::command("docker")
        .arg("cp")
        .arg(format!("{container}:/data/dump.rdb"))
        .arg(file_path)
        .output()
        .map_err(|e| format!("failed to run docker cp: {e}"))?;
    command_failed(&out).map_or(Ok(()), Err)
}

fn redis_restore(container: &str, file_path: &Path) -> Result<(), String> {
    let out = crate::modules::tools::command("docker")
        .arg("cp")
        .arg(file_path)
        .arg(format!("{container}:/data/dump.rdb"))
        .output()
        .map_err(|e| format!("failed to run docker cp: {e}"))?;
    if let Some(err) = command_failed(&out) {
        return Err(err);
    }
    let out = crate::modules::tools::command("docker")
        .args(["restart", container])
        .output()
        .map_err(|e| format!("failed to restart container: {e}"))?;
    command_failed(&out).map_or(Ok(()), Err)
}

#[tauri::command(async)]
pub fn databases_backup(app: tauri::AppHandle, name: String) -> Result<String, String> {
    let (kind, admin_user, admin_password) = admin_context(&app, &name)?;
    let container = container_name(&name);
    let dir = backups_dir(&app)?;
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let path = dir.join(backup_filename(&name, kind, &timestamp));

    match kind {
        DbKind::Mysql => mysql_backup(&container, &admin_password, &name, &path),
        DbKind::Postgres => postgres_backup(&container, &admin_user, &name, &path),
        DbKind::Mongo => mongo_backup(&container, &admin_user, &admin_password, &name, &path),
        DbKind::Redis => redis_backup(&container, &admin_password, &path),
    }?;

    Ok(path.to_string_lossy().to_string())
}

/// `database` (optional) targets a database other than the one named after the
/// service — used by the browser, which lets the user pick any database on the
/// server. Mongo/Redis restores ignore it since their archives carry their own
/// scope.
#[tauri::command(async)]
pub fn databases_restore(
    app: tauri::AppHandle,
    name: String,
    backup_path: String,
    database: Option<String>,
) -> Result<String, String> {
    let path = PathBuf::from(&backup_path);
    if !path.is_file() {
        return Err(format!("backup file not found: {backup_path}"));
    }
    let (kind, admin_user, admin_password) = admin_context(&app, &name)?;
    let container = container_name(&name);
    let target_db = database.filter(|d| !d.trim().is_empty()).unwrap_or_else(|| name.clone());
    if !valid_name(&target_db) {
        return Err(format!("'{target_db}' is not a valid database name"));
    }

    match kind {
        DbKind::Mysql => mysql_restore(&container, &admin_password, &target_db, &path),
        DbKind::Postgres => postgres_restore(&container, &admin_user, &target_db, &path),
        DbKind::Mongo => mongo_restore(&container, &admin_user, &admin_password, &path),
        DbKind::Redis => redis_restore(&container, &path),
    }?;

    Ok(format!("restored '{name}' from {backup_path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_filename_uses_sql_extension_for_sql_engines() {
        assert_eq!(backup_filename("mydb", DbKind::Mysql, "20260101_120000"), "mydb_20260101_120000.sql");
        assert_eq!(backup_filename("mydb", DbKind::Postgres, "20260101_120000"), "mydb_20260101_120000.sql");
    }

    #[test]
    fn backup_filename_uses_archive_gz_for_mongo() {
        assert_eq!(backup_filename("mydb", DbKind::Mongo, "20260101_120000"), "mydb_20260101_120000.archive.gz");
    }

    #[test]
    fn backup_filename_uses_rdb_for_redis() {
        assert_eq!(backup_filename("mydb", DbKind::Redis, "20260101_120000"), "mydb_20260101_120000.rdb");
    }

    #[test]
    fn mysql_dump_args_pass_password_without_a_space_before_the_flag_value() {
        let args = mysql_dump_args("s3cr3t", "mydb");
        assert_eq!(args, vec!["mysqldump", "-uroot", "-ps3cr3t", "mydb"]);
    }

    #[test]
    fn mysql_restore_args_use_the_plain_client_not_mysqldump() {
        let args = mysql_restore_args("s3cr3t", "mydb");
        assert_eq!(args, vec!["mysql", "-uroot", "-ps3cr3t", "mydb"]);
    }

    #[test]
    fn postgres_dump_args_target_the_right_database() {
        let args = postgres_dump_args("postgres", "mydb");
        assert_eq!(args, vec!["pg_dump", "-U", "postgres", "-d", "mydb"]);
    }

    #[test]
    fn postgres_restore_args_pipe_into_psql() {
        let args = postgres_restore_args("postgres", "mydb");
        assert_eq!(args, vec!["psql", "-U", "postgres", "-d", "mydb"]);
    }

    #[test]
    fn mongo_dump_args_scope_to_one_database_and_use_a_single_archive_file() {
        let args = mongo_dump_args("root", "s3cr3t", "mydb");
        assert_eq!(
            args,
            vec![
                "mongodump",
                "-u",
                "root",
                "-p",
                "s3cr3t",
                "--authenticationDatabase",
                "admin",
                "--db",
                "mydb",
                "--archive",
                "--gzip",
            ]
        );
    }

    #[test]
    fn mongo_restore_args_drop_before_restoring_and_read_the_same_archive_format() {
        let args = mongo_restore_args("root", "s3cr3t");
        assert_eq!(
            args,
            vec![
                "mongorestore",
                "-u",
                "root",
                "-p",
                "s3cr3t",
                "--authenticationDatabase",
                "admin",
                "--drop",
                "--archive",
                "--gzip",
            ]
        );
    }

    #[test]
    fn backup_extension_differs_per_engine_where_the_dump_format_differs() {
        assert_eq!(backup_extension(DbKind::Mysql), backup_extension(DbKind::Postgres));
        assert_ne!(backup_extension(DbKind::Mysql), backup_extension(DbKind::Mongo));
        assert_ne!(backup_extension(DbKind::Mysql), backup_extension(DbKind::Redis));
    }

    #[test]
    fn valid_schema_name_accepts_identifiers_and_rejects_quoting_hazards() {
        assert!(valid_schema_name("myapp"));
        assert!(valid_schema_name("my_app_2"));
        assert!(!valid_schema_name(""));
        assert!(!valid_schema_name("2fast"));
        assert!(!valid_schema_name("my-app"));
        assert!(!valid_schema_name("a`b"));
        assert!(!valid_schema_name(&"a".repeat(65)));
    }

    #[test]
    fn extract_host_port_reads_the_host_side_of_the_first_mapping() {
        let spec = db_spec("shop", DbKind::Mysql, "8", None, "pw");
        let block = render_service_block("shop", 3307, &spec);
        assert_eq!(extract_host_port(&block), Some(3307));
        assert_eq!(extract_host_port("  x:\n    image: mysql\n"), None);
    }

    #[test]
    fn declaring_a_volume_keeps_new_services_under_services() {
        let fresh = declare_volume("services:\n", "shop-data");
        assert_eq!(fresh, "volumes:\n  shop-data:\nservices:\n");

        // A second database joins the existing section rather than opening another.
        let both = declare_volume(&fresh, "blog-data");
        assert_eq!(both, "volumes:\n  blog-data:\n  shop-data:\nservices:\n");

        // Already declared: unchanged, not duplicated.
        assert_eq!(declare_volume(&both, "shop-data"), both);

        // The volume key never shadows a service that happens to end in `_data`.
        let with_service = format!("{both}  shop_data:\n    image: mysql:8\n");
        assert!(service_block_text(&with_service, "shop_data").unwrap().contains("image: mysql:8"));
    }

    #[test]
    fn undeclaring_a_volume_removes_only_that_entry() {
        let contents = "volumes:\n  blog-data:\n  shop-data:\nservices:\n  shop:\n    image: mysql:8\n";
        assert_eq!(
            undeclare_volume(contents, "shop-data"),
            "volumes:\n  blog-data:\nservices:\n  shop:\n    image: mysql:8\n"
        );
        // Not declared, or no section at all: nothing to do.
        assert_eq!(undeclare_volume(contents, "other-data"), contents);
        assert_eq!(undeclare_volume("services:\n", "shop-data"), "services:\n");
    }

    #[test]
    fn laravel_driver_names_match_config_database_php() {
        assert_eq!(laravel_driver(DbKind::Mysql), "mysql");
        assert_eq!(laravel_driver(DbKind::Postgres), "pgsql");
    }
}
