//! macOS. `kill` is the half that was already portable and works unchanged; listing
//! ports is not implemented, and says so.
//!
//! The equivalent of `ss -tlnp` here is `lsof -iTCP -sTCP:LISTEN -P -n`, whose output is
//! a different shape entirely and which reports nothing about processes belonging to
//! other users without elevation. Writing that parser without a machine to check it
//! against would mean shipping a page that silently lists the wrong things, so this
//! keeps returning the error the module has always returned off Linux — the Processes
//! page shows it and stays honest.

use super::super::PortEntry;

pub fn list_listening_ports() -> Result<Vec<PortEntry>, String> {
    Err("listing ports is only supported on Linux and Windows right now".to_string())
}

/// `ps -o comm=` prints the full executable path on macOS, unlike on Linux. Nothing
/// calls this until the listing above exists, but it keeps the platform surface whole.
pub fn exe_path(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && path.starts_with('/')).then_some(path)
}

pub fn kill(pid: u32, force: bool) -> Result<String, String> {
    super::unix_kill(pid, force)
}
