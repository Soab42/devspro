//! Starting the Docker engine when it's installed but not running — the "Start
//! Docker" button on the Docker, Databases and Data browser pages.
//!
//! Only asks it to start, and returns. The engine takes anywhere from a few
//! seconds to a minute to answer, so waiting for it is the page's job: it keeps
//! running `detect()` until the daemon replies.
//!
//! The one exception to this module's "no elevation" rule is the Linux engine:
//! `docker.service` is a system unit, so starting it goes through the helper's
//! `ServiceControl`, which only takes start/stop and a validated unit name.
//! Docker Desktop, on any OS, is a program the user starts as themselves.

#[cfg(windows)]
use std::path::PathBuf;

/// Docker Desktop's own executable. Found next to the `docker.exe` on PATH
/// first (`…\Docker\Docker\resources\bin\docker.exe` → `…\Docker\Docker\`), so a
/// non-default install location still works, then in the default location.
#[cfg(windows)]
fn docker_desktop_exe() -> Option<PathBuf> {
    let beside_cli = crate::modules::tools::find("docker").and_then(|cli| {
        let root = PathBuf::from(cli).parent()?.parent()?.parent()?.to_path_buf();
        Some(root.join("Docker Desktop.exe"))
    });
    let default = std::env::var("ProgramFiles")
        .ok()
        .map(|p| PathBuf::from(p).join("Docker").join("Docker").join("Docker Desktop.exe"));
    [beside_cli, default].into_iter().flatten().find(|p| p.is_file())
}

#[cfg(windows)]
pub fn start(_app: &tauri::AppHandle) -> Result<String, String> {
    let exe = docker_desktop_exe().ok_or(
        "Docker Desktop isn't installed where DevsPro can find it. Start it from the Start menu, or reinstall it from Apps.",
    )?;
    // Spawned and left alone: it's a GUI app that outlives DevsPro, and it
    // starts the engine itself once it's up.
    crate::modules::tools::command(&exe)
        .spawn()
        .map_err(|e| format!("could not start Docker Desktop: {e}"))?;
    Ok("Starting Docker Desktop…".to_string())
}

#[cfg(target_os = "macos")]
pub fn start(_app: &tauri::AppHandle) -> Result<String, String> {
    let output = std::process::Command::new("open")
        .args(["-a", "Docker"])
        .output()
        .map_err(|e| format!("could not start Docker Desktop: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "could not start Docker Desktop: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok("Starting Docker Desktop…".to_string())
}

/// Docker Desktop for Linux runs as the user's own systemd service; the plain
/// engine is the system's `docker.service`. Desktop wins when both exist,
/// because that is the context the `docker` CLI is pointed at.
#[cfg(target_os = "linux")]
pub fn start(app: &tauri::AppHandle) -> Result<String, String> {
    use crate::elevate::{self, Operation, OperationManifest};

    let desktop = std::process::Command::new("systemctl")
        .args(["--user", "cat", "docker-desktop"])
        .output()
        .is_ok_and(|o| o.status.success());
    if desktop {
        let output = std::process::Command::new("systemctl")
            .args(["--user", "start", "docker-desktop"])
            .output()
            .map_err(|e| format!("could not run systemctl: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "could not start Docker Desktop: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        return Ok("Starting Docker Desktop…".to_string());
    }

    let results = elevate::run(
        app,
        OperationManifest {
            allowed_write_prefix: String::new(),
            operations: vec![Operation::ServiceControl {
                action: "start".to_string(),
                unit: "docker".to_string(),
            }],
        },
    )?;
    match results.into_iter().next() {
        Some(r) if r.success => Ok("Started the Docker service.".to_string()),
        Some(r) => Err(r.message),
        None => Err("the helper returned no result".to_string()),
    }
}
