//! Linux and macOS: `sh -c` to interpret the command line the user typed, and signals
//! to the worker's process group to stop it.

use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

/// The worker gets a process group of its own (pgid == its pid), so the signals below
/// reach everything it starts and nothing of DevsPro's. A side effect is that a Ctrl+C
/// aimed at DevsPro's own group, say in a terminal running `tauri dev`, no longer reaches
/// workers.
pub fn shell_command(command_line: &str) -> Command {
    let mut command = Command::new("sh");
    command.arg("-c").arg(command_line).process_group(0);
    command
}

/// SIGTERM to the whole group. `sh -c` only execs a lone simple command in place; with
/// `cd x && npm run dev`, a pipe or `a && b` the shell stays as the parent, so signalling
/// just its pid would end the shell and orphan the real worker while the app reported it
/// stopped.
pub fn request_stop(child: &Child) -> Result<(), String> {
    signal_group(child, libc::SIGTERM)
}

/// SIGKILL to the whole group, for a worker that ignored `request_stop`.
pub fn force_stop(child: &Child) -> Result<(), String> {
    signal_group(child, libc::SIGKILL)
}

fn signal_group(child: &Child, signal: i32) -> Result<(), String> {
    // SAFETY: plain syscall; the negated pid addresses the group `process_group(0)` made.
    let result = unsafe { libc::kill(-(child.id() as i32), signal) };
    // ESRCH: the group is already empty, which is what was wanted.
    if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
        return Err(format!("failed to signal the worker: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}
