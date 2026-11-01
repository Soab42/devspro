//! Per-OS process inspection, split the same way `elevate::platform`,
//! `system::platform` and `workers::platform` are:
//!
//! | | |
//! | --- | --- |
//! | `list_listening_ports()` | Every listening TCP socket and whatever process owns it. `Err` when this OS has no supported way to ask. |
//! | `kill(pid, force)` | Stop a process the user picked, returning the message the UI shows. |
//! | `exe_path(pid)` | The full path of the executable a process runs, or `None` where it can't be read. |
//! | `process_name(pid)` | The name of the process running as `pid`, or `None` when nothing is. |
//!
//! Unlike the other three splits, this one keeps Linux and macOS apart. Linux reads `ss`
//! (iproute2) and `/proc`, and macOS has neither — so rather than pretend, its listing
//! returns the same "not supported here" error the module has always returned off Linux,
//! while `kill` works, because `kill` is the one part that was already portable.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

/// Shared by the Unix backends. A normal `kill` only reaches processes owned by the same
/// user, which is the whole intended scope — a dev server *you* started. Killing
/// somebody else's or root's process is deliberately out of reach; this isn't a task
/// manager, and a sudo path here would be an easy way to break your own system.
#[cfg(unix)]
fn unix_kill(pid: u32, force: bool) -> Result<String, String> {
    let signal = if force { "-9" } else { "-15" };
    let output = std::process::Command::new("kill")
        .args([signal, &pid.to_string()])
        .output()
        .map_err(|e| format!("failed to run kill: {e}"))?;
    if output.status.success() {
        Ok(format!(
            "sent {} to pid {pid}",
            if force { "SIGKILL" } else { "SIGTERM" }
        ))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}
