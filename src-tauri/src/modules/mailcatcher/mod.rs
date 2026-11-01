//! Mail Catcher: Mailpit (https://github.com/axllent/mailpit), a single-binary
//! SMTP + web-UI mail catcher, run one of two ways — in Docker (`docker.rs`) or
//! as a built-in process for machines without it (`native.rs`).
//!
//! The rule the whole module follows is **DevsPro manages only what it started.**
//! So the question asked on every page open isn't "is Docker up" but "who is on
//! the Mailpit ports right now", answered by `resolve` in this order:
//!
//! 1. our built-in process (proved by its pid file — see `native::running`),
//! 2. our container — only looked up through `docker` when something is
//!    actually listening on its port, so a Docker-less machine never spawns it,
//! 3. somebody else's Mailpit (`probe::is_mailpit`): shown and linked, never
//!    stopped — "managed outside DevsPro",
//! 4. some other program: named, so the user can pick another port,
//! 5. nothing: off.
//!
//! Off is the default. Nothing starts until the user presses Start, unless
//! they've turned on "start with DevsPro"; and whatever DevsPro started is
//! stopped when DevsPro quits, unless they've turned on "keep running".

mod docker;
mod native;
mod probe;

pub(crate) use docker::PROJECT_NAME;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;
use tauri::Manager;

/// The child handle of a built-in Mailpit this session started, so stopping it
/// also reaps it. One started by an earlier session has no handle and is
/// stopped by pid instead.
#[derive(Default)]
pub struct MailcatcherState(Mutex<Option<Child>>);

/// Serialises every start/stop/install, so the page and the start-with-app
/// thread can't both decide the ports are free and start two Mailpits.
static OPERATION: Mutex<()> = Mutex::new(());

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Native,
    Docker,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct Settings {
    backend: Backend,
    smtp_port: u16,
    http_port: u16,
    start_with_app: bool,
    keep_running: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            backend: Backend::Native,
            smtp_port: 1025,
            http_port: 8025,
            start_with_app: false,
            keep_running: false,
        }
    }
}

fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("mailcatcher");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn load_settings(dir: &Path) -> Settings {
    fs::read_to_string(dir.join("settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_settings(dir: &Path, settings: &Settings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(dir.join("settings.json"), text).map_err(|e| format!("could not save Mail Catcher settings: {e}"))
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Off,
    /// Ours — `backend` says which.
    Running,
    /// A Mailpit DevsPro didn't start. Linked, never stopped.
    External,
    /// Something that isn't Mailpit holds one of the ports.
    Conflict,
}

#[derive(Debug, Serialize, Clone)]
pub struct MailcatcherStatus {
    pub state: RunState,
    /// Which of ours is running, when `state` is `Running`; otherwise the one
    /// the user last chose, which the page preselects.
    pub backend: Backend,
    pub smtp_port: u16,
    pub http_port: u16,
    pub web_url: Option<String>,
    /// Who holds the port, when `state` is `Conflict`.
    pub conflict: Option<String>,
    /// A Mailpit binary is available to the built-in backend: ours, or one on PATH.
    pub native_available: bool,
    /// Ours specifically — what Uninstall would delete.
    pub native_downloaded: bool,
    pub native_version: &'static str,
    /// Our container exists (running or not) — what Remove container would delete.
    pub docker_configured: bool,
    pub start_with_app: bool,
    pub keep_running: bool,
}

fn web_url(port: u16) -> String {
    format!("http://localhost:{port}")
}

/// What is on the Mailpit ports right now — see the module docs for the order.
fn resolve(dir: &Path) -> MailcatcherStatus {
    let settings = load_settings(dir);
    let docker_ports = docker::configured_ports(dir);
    let mut status = MailcatcherStatus {
        state: RunState::Off,
        backend: settings.backend,
        smtp_port: settings.smtp_port,
        http_port: settings.http_port,
        web_url: None,
        conflict: None,
        native_available: native::binary(dir).is_some(),
        native_downloaded: native::downloaded(dir),
        native_version: native::VERSION,
        docker_configured: docker_ports.is_some(),
        start_with_app: settings.start_with_app,
        keep_running: settings.keep_running,
    };

    if let Some(ours) = native::running(dir) {
        status.state = RunState::Running;
        status.backend = Backend::Native;
        status.smtp_port = ours.smtp_port;
        status.http_port = ours.http_port;
        status.web_url = Some(web_url(ours.http_port));
        return status;
    }

    if let Some((smtp, http)) = docker_ports {
        if probe::listening(http) && docker::container_running() {
            status.state = RunState::Running;
            status.backend = Backend::Docker;
            status.smtp_port = smtp;
            status.http_port = http;
            status.web_url = Some(web_url(http));
            return status;
        }
    }

    let (smtp, http) = (settings.smtp_port, settings.http_port);
    if probe::listening(http) {
        if probe::is_mailpit(http) {
            status.state = RunState::External;
            status.web_url = Some(web_url(http));
        } else {
            status.state = RunState::Conflict;
            status.conflict = Some(format!("Port {http} is in use by {}.", probe::owner_of(http)));
        }
    } else if probe::listening(smtp) {
        status.state = RunState::Conflict;
        status.conflict = Some(format!("Port {smtp} is in use by {}.", probe::owner_of(smtp)));
    }
    status
}

/// Stops whichever Mailpit of ours `current` says is running. Refuses anything
/// else: an external Mailpit is the user's to stop, not ours.
fn stop_ours(app: &tauri::AppHandle, dir: &Path, current: &MailcatcherStatus) -> Result<(), String> {
    match (current.state, current.backend) {
        (RunState::Running, Backend::Native) => {
            let Some(pid_file) = native::running(dir) else {
                return Ok(());
            };
            let child = app.state::<MailcatcherState>().0.lock().unwrap_or_else(PoisonError::into_inner).take();
            native::stop(dir, &pid_file, child)
        }
        (RunState::Running, Backend::Docker) => {
            docker::stop(dir)?;
            probe::wait_until_free(&[current.smtp_port, current.http_port], Duration::from_secs(5));
            Ok(())
        }
        (RunState::External, _) => Err(
            "This Mailpit wasn't started by DevsPro, so DevsPro leaves it running. Stop it wherever it was started."
                .to_string(),
        ),
        _ => Ok(()),
    }
}

/// Why `port` can't be used, or `None` when it's free.
fn port_problem(port: u16) -> Option<String> {
    if !probe::listening(port) {
        return None;
    }
    Some(if probe::is_mailpit(port) {
        format!("A Mailpit that DevsPro didn't start is already on port {port}. Use it as it is, or pick other ports.")
    } else {
        format!("Port {port} is in use by {}. Pick another port.", probe::owner_of(port))
    })
}

fn start_inner(app: &tauri::AppHandle, dir: &Path, backend: Backend, smtp_port: u16, http_port: u16) -> Result<(), String> {
    if smtp_port == 0 || http_port == 0 {
        return Err("ports must be between 1 and 65535".to_string());
    }
    if smtp_port == http_port {
        return Err("the SMTP and web UI ports must be different".to_string());
    }

    let current = resolve(dir);
    if current.state == RunState::Running {
        if current.backend == backend && current.smtp_port == smtp_port && current.http_port == http_port {
            return Ok(());
        }
        // Switching backends or ports: ours goes first, so the new one doesn't
        // find its own predecessor on the port.
        stop_ours(app, dir, &current)?;
    }

    if let Some(problem) = port_problem(smtp_port).or_else(|| port_problem(http_port)) {
        return Err(problem);
    }

    match backend {
        Backend::Native => {
            let (child, _) = native::start(dir, smtp_port, http_port)?;
            *app.state::<MailcatcherState>().0.lock().unwrap_or_else(PoisonError::into_inner) = Some(child);
        }
        Backend::Docker => docker::start(dir, smtp_port, http_port)?,
    }

    let mut settings = load_settings(dir);
    settings.backend = backend;
    settings.smtp_port = smtp_port;
    settings.http_port = http_port;
    save_settings(dir, &settings)
}

fn locked<T>(f: impl FnOnce() -> T) -> T {
    let _guard = OPERATION.lock().unwrap_or_else(PoisonError::into_inner);
    f()
}

#[tauri::command(async)]
pub fn mailcatcher_status(app: tauri::AppHandle) -> Result<MailcatcherStatus, String> {
    Ok(resolve(&data_dir(&app)?))
}

#[tauri::command(async)]
pub fn mailcatcher_start(
    app: tauri::AppHandle,
    backend: Backend,
    smtp_port: u16,
    http_port: u16,
) -> Result<MailcatcherStatus, String> {
    let dir = data_dir(&app)?;
    locked(|| start_inner(&app, &dir, backend, smtp_port, http_port))?;
    Ok(resolve(&dir))
}

#[tauri::command(async)]
pub fn mailcatcher_stop(app: tauri::AppHandle) -> Result<MailcatcherStatus, String> {
    let dir = data_dir(&app)?;
    locked(|| stop_ours(&app, &dir, &resolve(&dir)))?;
    Ok(resolve(&dir))
}

/// Deletes our container and its mail. Stops it first if it's running.
#[tauri::command(async)]
pub fn mailcatcher_remove_container(app: tauri::AppHandle) -> Result<MailcatcherStatus, String> {
    let dir = data_dir(&app)?;
    locked(|| docker::remove(&dir))?;
    Ok(resolve(&dir))
}

#[tauri::command]
pub async fn mailcatcher_install(app: tauri::AppHandle) -> Result<MailcatcherStatus, String> {
    let dir = data_dir(&app)?;
    native::install(&app, &dir).await?;
    Ok(resolve(&dir))
}

/// Stops the built-in Mailpit if it's ours and running, then removes everything
/// it left on disk, and turns both switches off — after this, DevsPro has no
/// Mailpit of its own on the machine.
#[tauri::command(async)]
pub fn mailcatcher_uninstall(app: tauri::AppHandle, delete_mail: bool) -> Result<MailcatcherStatus, String> {
    let dir = data_dir(&app)?;
    locked(|| {
        let current = resolve(&dir);
        if current.state == RunState::Running && current.backend == Backend::Native {
            stop_ours(&app, &dir, &current)?;
        }
        native::uninstall(&dir, delete_mail)?;
        let mut settings = load_settings(&dir);
        settings.start_with_app = false;
        settings.keep_running = false;
        save_settings(&dir, &settings)
    })?;
    Ok(resolve(&dir))
}

#[tauri::command(async)]
pub fn mailcatcher_set_options(
    app: tauri::AppHandle,
    start_with_app: bool,
    keep_running: bool,
) -> Result<MailcatcherStatus, String> {
    let dir = data_dir(&app)?;
    let mut settings = load_settings(&dir);
    settings.start_with_app = start_with_app;
    settings.keep_running = keep_running;
    save_settings(&dir, &settings)?;
    Ok(resolve(&dir))
}

/// At launch, on a background thread so a slow Docker never holds up the window.
///
/// - "Start with DevsPro" on: start the last-used backend, unless something is
///   already there (ours kept running from last time, or anyone else's).
/// - Off, but a built-in Mailpit of ours is running with "keep running" off too:
///   it outlived a DevsPro that crashed instead of quitting, so the exit hook never
///   ran. Stopped now, which is what that exit would have done.
pub fn on_startup(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Ok(dir) = data_dir(&app) else { return };
        let settings = load_settings(&dir);
        let result = locked(|| {
            let current = resolve(&dir);
            if settings.start_with_app {
                if current.state == RunState::Off {
                    return start_inner(&app, &dir, settings.backend, settings.smtp_port, settings.http_port);
                }
            } else if !settings.keep_running && current.state == RunState::Running && current.backend == Backend::Native {
                return stop_ours(&app, &dir, &current);
            }
            Ok(())
        });
        if let Err(e) = result {
            eprintln!("mail catcher: {e}");
        }
    });
}

/// From `RunEvent::ExitRequested`: unless the user asked for it to keep running,
/// whatever Mailpit DevsPro is running stops with it.
pub fn on_exit(app: &tauri::AppHandle) {
    let Ok(dir) = data_dir(app) else { return };
    if load_settings(&dir).keep_running {
        return;
    }
    let _guard = OPERATION.lock().unwrap_or_else(PoisonError::into_inner);
    let current = resolve(&dir);
    if current.state == RunState::Running {
        let _ = stop_ours(app, &dir, &current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_and_on_the_standard_ports() {
        let s = Settings::default();
        assert!(!s.start_with_app);
        assert!(!s.keep_running);
        assert_eq!((s.smtp_port, s.http_port), (1025, 8025));
    }

    #[test]
    fn settings_written_by_an_older_build_fill_in_the_rest() {
        let s: Settings = serde_json::from_str("{\"smtp_port\": 2025}").unwrap();
        assert_eq!(s.smtp_port, 2025);
        assert_eq!(s.http_port, 8025);
        assert_eq!(s.backend, Backend::Native);
    }
}
