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

#[tauri::command]
pub fn processes_list_ports() -> Result<Vec<PortEntry>, String> {
    platform::list_listening_ports()
}

#[tauri::command]
pub fn processes_kill(pid: u32, force: bool) -> Result<String, String> {
    platform::kill(pid, force)
}
