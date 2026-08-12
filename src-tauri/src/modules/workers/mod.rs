//! Supervises long-running dev processes the user defines and starts through
//! this app — e.g. `php artisan queue:work` for a Laravel site, or any other
//! shell command someone wants kept running with a start/stop/restart and a
//! log tail. Different in kind from `processes` (which lists/kills arbitrary
//! system processes DevsPro never started): a worker's pid/handle is known
//! directly because this app spawned it, so there's no /proc scraping needed,
//! and its *definition* (name, command, working dir) is persisted so it
//! survives an app restart — the running process itself doesn't, same as any
//! terminal job wouldn't either.
//!
//! No elevation: spawning/killing a process under the user's own account needs
//! no privilege escalation, same reasoning as the Docker module.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

pub fn migrations() -> Vec<&'static str> {
    vec![
        r#"
        CREATE TABLE workers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            command TEXT NOT NULL,
            working_dir TEXT NOT NULL,
            site_id TEXT,
            created_at TEXT NOT NULL
        );
        "#,
    ]
}

/// Live handles for workers currently running, keyed by worker id. Only makes
/// sense while this app is running — deliberately not persisted, unlike the
/// worker definitions themselves.
#[derive(Default)]
pub struct WorkersState(pub Mutex<HashMap<String, Child>>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worker {
    pub id: String,
    pub name: String,
    pub command: String,
    pub working_dir: String,
    pub site_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerView {
    #[serde(flatten)]
    pub worker: Worker,
    pub running: bool,
}

const WORKER_COLUMNS: &str = "id, name, command, working_dir, site_id, created_at";

fn row_to_worker(row: &rusqlite::Row) -> rusqlite::Result<Worker> {
    Ok(Worker {
        id: row.get(0)?,
        name: row.get(1)?,
        command: row.get(2)?,
        working_dir: row.get(3)?,
        site_id: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn list(conn: &Connection) -> rusqlite::Result<Vec<Worker>> {
    let mut stmt = conn.prepare(&format!("SELECT {WORKER_COLUMNS} FROM workers ORDER BY created_at"))?;
    let rows = stmt.query_map([], row_to_worker)?;
    rows.collect()
}

fn find(conn: &Connection, id: &str) -> Result<Worker, String> {
    conn.query_row(&format!("SELECT {WORKER_COLUMNS} FROM workers WHERE id = ?1"), [id], row_to_worker)
        .map_err(|_| "worker not found".to_string())
}

/// Sites are owned by the `nginx_ssl` module; a worker only holds an optional,
/// loosely-coupled reference to one (no FK enforcement) so a site being edited
/// or removed elsewhere never blocks on this module.
fn site_exists(conn: &Connection, site_id: &str) -> bool {
    conn.query_row("SELECT COUNT(*) FROM nginx_ssl_sites WHERE id = ?1", [site_id], |r| {
        r.get::<_, i64>(0)
    })
    .unwrap_or(0)
        > 0
}

fn site_project_path(conn: &Connection, site_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT project_path FROM nginx_ssl_sites WHERE id = ?1",
        [site_id],
        |r| r.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

pub fn resolve_working_dir(explicit: Option<&str>, site_project_path: Option<&str>) -> Result<String, String> {
    if let Some(dir) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(dir.to_string());
    }
    if let Some(path) = site_project_path.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(path.to_string());
    }
    Err("a working directory is required — set one directly, or choose a site that has a project folder".to_string())
}

fn validate_definition(name: &str, command: &str) -> Result<(String, String), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    let command = command.trim().to_string();
    if command.is_empty() {
        return Err("command cannot be empty".to_string());
    }
    Ok((name, command))
}

fn logs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("logs")
        .join("workers");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn log_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(logs_dir(app)?.join(format!("{id}.log")))
}

fn tail_lines(content: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Reconciles the live-handle map against reality: any child that has already
/// exited (crashed, finished, or was reaped by a previous `try_wait`) is
/// dropped from the map here rather than left dangling.
fn running_ids(state: &WorkersState) -> Result<HashSet<String>, String> {
    let mut map = state.0.lock().map_err(|e| e.to_string())?;
    let ids: Vec<String> = map.keys().cloned().collect();
    let mut running = HashSet::new();
    for id in ids {
        let still_running = match map.get_mut(&id) {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        };
        if still_running {
            running.insert(id);
        } else {
            map.remove(&id);
        }
    }
    Ok(running)
}

fn to_view(worker: Worker, running: &HashSet<String>) -> WorkerView {
    let is_running = running.contains(&worker.id);
    WorkerView {
        running: is_running,
        worker,
    }
}

#[tauri::command]
pub fn workers_list(db: State<crate::db::Db>, state: State<WorkersState>) -> Result<Vec<WorkerView>, String> {
    let workers = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        list(&conn).map_err(|e| e.to_string())?
    };
    let running = running_ids(&state)?;
    Ok(workers.into_iter().map(|w| to_view(w, &running)).collect())
}

#[tauri::command]
pub fn workers_create(
    db: State<crate::db::Db>,
    name: String,
    command: String,
    working_dir: Option<String>,
    site_id: Option<String>,
) -> Result<Worker, String> {
    let (name, command) = validate_definition(&name, &command)?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    if let Some(sid) = &site_id {
        if !site_exists(&conn, sid) {
            return Err("selected site not found".to_string());
        }
    }
    let site_path = site_id.as_deref().and_then(|sid| site_project_path(&conn, sid));
    let working_dir = resolve_working_dir(working_dir.as_deref(), site_path.as_deref())?;

    let worker = Worker {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        command,
        working_dir,
        site_id,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    conn.execute(
        "INSERT INTO workers (id, name, command, working_dir, site_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        (
            &worker.id,
            &worker.name,
            &worker.command,
            &worker.working_dir,
            &worker.site_id,
            &worker.created_at,
        ),
    )
    .map_err(|e| e.to_string())?;
    Ok(worker)
}

#[tauri::command]
pub fn workers_update(
    db: State<crate::db::Db>,
    id: String,
    name: String,
    command: String,
    working_dir: Option<String>,
    site_id: Option<String>,
) -> Result<Worker, String> {
    let (name, command) = validate_definition(&name, &command)?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    find(&conn, &id)?;

    if let Some(sid) = &site_id {
        if !site_exists(&conn, sid) {
            return Err("selected site not found".to_string());
        }
    }
    let site_path = site_id.as_deref().and_then(|sid| site_project_path(&conn, sid));
    let working_dir = resolve_working_dir(working_dir.as_deref(), site_path.as_deref())?;

    conn.execute(
        "UPDATE workers SET name = ?1, command = ?2, working_dir = ?3, site_id = ?4 WHERE id = ?5",
        (&name, &command, &working_dir, &site_id, &id),
    )
    .map_err(|e| e.to_string())?;

    find(&conn, &id)
}

#[tauri::command]
pub fn workers_delete(
    app: AppHandle,
    db: State<crate::db::Db>,
    state: State<WorkersState>,
    id: String,
) -> Result<(), String> {
    let _ = stop_running(&state, &id);
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM workers WHERE id = ?1", [&id]).map_err(|e| e.to_string())?;
    if let Ok(path) = log_path(&app, &id) {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

/// Sends SIGTERM and waits up to ~2s for the process to exit — conceptually the
/// same grace period `docker stop` gives a container by default. If it's still
/// running after that it's left alone rather than force-killed; escalating to
/// SIGKILL after a timeout would be a reasonable follow-up but isn't built for
/// v1.
fn stop_running(state: &WorkersState, id: &str) -> Result<bool, String> {
    let mut map = state.0.lock().map_err(|e| e.to_string())?;
    let Some(child) = map.get_mut(id) else {
        return Ok(false);
    };
    if matches!(child.try_wait(), Ok(Some(_))) {
        map.remove(id);
        return Ok(false);
    }

    let pid = child.id();
    Command::new("kill")
        .args(["-15", &pid.to_string()])
        .output()
        .map_err(|e| format!("failed to send SIGTERM: {e}"))?;

    let mut exited = false;
    for _ in 0..20 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if exited {
        map.remove(id);
    }
    Ok(exited)
}

fn spawn_worker(app: &AppHandle, state: &WorkersState, worker: &Worker) -> Result<(), String> {
    let path = log_path(app, &worker.id)?;
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| format!("failed to open log file: {e}"))?;
    let stderr_file = log_file.try_clone().map_err(|e| e.to_string())?;

    let child = Command::new("sh")
        .arg("-c")
        .arg(&worker.command)
        .current_dir(&worker.working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|e| format!("failed to start: {e}"))?;

    let mut map = state.0.lock().map_err(|e| e.to_string())?;
    map.insert(worker.id.clone(), child);
    Ok(())
}

#[tauri::command]
pub fn workers_start(
    app: AppHandle,
    db: State<crate::db::Db>,
    state: State<WorkersState>,
    id: String,
) -> Result<WorkerView, String> {
    let worker = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        find(&conn, &id)?
    };
    if running_ids(&state)?.contains(&id) {
        return Err("worker is already running".to_string());
    }
    if !std::path::Path::new(&worker.working_dir).is_dir() {
        return Err(format!("working directory does not exist: {}", worker.working_dir));
    }
    spawn_worker(&app, &state, &worker)?;
    Ok(WorkerView { worker, running: true })
}

#[tauri::command]
pub fn workers_stop(db: State<crate::db::Db>, state: State<WorkersState>, id: String) -> Result<WorkerView, String> {
    let worker = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        find(&conn, &id)?
    };
    stop_running(&state, &id)?;
    let running = running_ids(&state)?.contains(&id);
    Ok(WorkerView { worker, running })
}

#[tauri::command]
pub fn workers_restart(
    app: AppHandle,
    db: State<crate::db::Db>,
    state: State<WorkersState>,
    id: String,
) -> Result<WorkerView, String> {
    let worker = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        find(&conn, &id)?
    };
    stop_running(&state, &id)?;
    if !std::path::Path::new(&worker.working_dir).is_dir() {
        return Err(format!("working directory does not exist: {}", worker.working_dir));
    }
    spawn_worker(&app, &state, &worker)?;
    Ok(WorkerView { worker, running: true })
}

/// Tails the worker's own log file rather than the child process directly — a
/// live long-running process's output has to be captured continuously as it
/// runs (redirected to disk at spawn time in `spawn_worker`), unlike
/// `docker_logs`, which can just ask the daemon for history on demand.
#[tauri::command]
pub fn workers_logs(app: AppHandle, id: String, tail: u32) -> Result<String, String> {
    let path = log_path(&app, &id)?;
    if !path.exists() {
        return Ok("(no log output)".to_string());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let text = tail_lines(&content, tail as usize);
    if text.trim().is_empty() {
        Ok("(no log output)".to_string())
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_lines_returns_last_n_lines() {
        let content = "a\nb\nc\nd\ne";
        assert_eq!(tail_lines(content, 2), "d\ne");
        assert_eq!(tail_lines(content, 100), "a\nb\nc\nd\ne");
        assert_eq!(tail_lines(content, 0), "");
        assert_eq!(tail_lines("", 5), "");
    }

    #[test]
    fn resolve_working_dir_prefers_explicit_over_site() {
        assert_eq!(
            resolve_working_dir(Some("/tmp/explicit"), Some("/tmp/site")).unwrap(),
            "/tmp/explicit"
        );
    }

    #[test]
    fn resolve_working_dir_falls_back_to_site_project_path() {
        assert_eq!(resolve_working_dir(None, Some("/tmp/site")).unwrap(), "/tmp/site");
    }

    #[test]
    fn resolve_working_dir_ignores_blank_explicit_value() {
        assert_eq!(resolve_working_dir(Some("   "), Some("/tmp/site")).unwrap(), "/tmp/site");
    }

    #[test]
    fn resolve_working_dir_requires_one_source() {
        assert!(resolve_working_dir(None, None).is_err());
        assert!(resolve_working_dir(Some("   "), None).is_err());
    }

    #[test]
    fn validate_definition_rejects_blank_fields() {
        assert!(validate_definition("", "echo hi").is_err());
        assert!(validate_definition("name", "  ").is_err());
    }

    #[test]
    fn validate_definition_trims_values() {
        let (name, command) = validate_definition("  My Worker  ", "  php artisan queue:work  ").unwrap();
        assert_eq!(name, "My Worker");
        assert_eq!(command, "php artisan queue:work");
    }

    /// Exercises the same spawn/redirect-to-file/tail/SIGTERM primitives
    /// `spawn_worker`, `workers_logs`, and `stop_running` use, minus the
    /// `AppHandle` plumbing (which needs a running Tauri app). Spawns a real
    /// `while true; do echo tick; sleep 1; done` child, confirms its output
    /// lands in the log file, then confirms a graceful stop actually reaps it.
    #[test]
    fn smoke_test_spawns_tails_and_stops_a_real_process() {
        let dir = std::env::temp_dir().join(format!("devsuite-workers-smoke-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let log_path = dir.join("worker.log");
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)
            .unwrap();
        let stderr_file = log_file.try_clone().unwrap();

        let child = Command::new("sh")
            .arg("-c")
            .arg("while true; do echo tick; sleep 1; done")
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(stderr_file))
            .spawn()
            .expect("failed to spawn smoke test process");

        let state = WorkersState::default();
        let id = "smoke-test".to_string();
        state.0.lock().unwrap().insert(id.clone(), child);

        std::thread::sleep(Duration::from_millis(1500));
        assert!(running_ids(&state).unwrap().contains(&id), "process should still be running");

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("tick"), "expected log output, got: {content:?}");
        assert!(tail_lines(&content, 2).contains("tick"));

        let exited = stop_running(&state, &id).unwrap();
        assert!(exited, "expected SIGTERM to stop the process within the grace window");
        assert!(!running_ids(&state).unwrap().contains(&id));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
