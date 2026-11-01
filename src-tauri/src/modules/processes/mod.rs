//! Lists every listening TCP port and whatever process owns it (the node/next/php/
//! whatever-dev-server processes people forget running in a background terminal),
//! with a way to kill them. The reading itself is per-OS and lives in `platform/`:
//! Linux on `ss` (iproute2) and `/proc/<pid>/cmdline`, Windows on `GetExtendedTcpTable`
//! and a ToolHelp snapshot. macOS lists nothing yet and says so rather than guessing.
//!
//! No elevation: a normal kill only works on processes owned by the same user
//! anyway, which covers the actual use case (a dev server *you* started). Killing
//! someone else's/root's process is out of scope on purpose — this isn't a task
//! manager, and adding a sudo path here would be an easy way to shoot yourself in
//! the foot.

mod platform;

use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct PortEntry {
    pub port: u16,
    pub address: String,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    /// The full command line, where the OS will say. Linux reads it from `/proc`;
    /// Windows keeps it somewhere this app deliberately doesn't reach into, so it stays
    /// `None` there rather than being approximated.
    pub command: Option<String>,
}

#[tauri::command(async)]
pub fn processes_list_ports() -> Result<Vec<PortEntry>, String> {
    platform::list_listening_ports()
}

/// The same listing for other modules — `nginx_ssl::port_conflict` names whatever is
/// holding a port a web server failed to bind.
pub(crate) fn listening_ports() -> Result<Vec<PortEntry>, String> {
    platform::list_listening_ports()
}

/// The full path of the executable a process was started from, where this OS will say.
/// `nginx_ssl` uses it to find the exact web server that is running — XAMPP's, WAMP's,
/// Laragon's — instead of guessing from a list of install folders.
pub(crate) fn exe_path(pid: u32) -> Option<String> {
    platform::exe_path(pid)
}

/// The name of the process running as `pid` right now (`httpd.exe`, `nginx`), or `None`
/// when there is none. What makes a leftover pid file tell the truth: Laragon leaves
/// `httpd.pid` behind when it stops Apache, so the pid in it has to be checked against
/// what is actually running.
pub(crate) fn process_name(pid: u32) -> Option<String> {
    platform::process_name(pid)
}

#[tauri::command(async)]
pub fn processes_kill(pid: u32, force: bool) -> Result<String, String> {
    platform::kill(pid, force)
}
