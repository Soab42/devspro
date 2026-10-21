//! Linux and macOS. Both lines below are the ones this module ran before the platform
//! split, moved rather than rewritten — `sh -c` to interpret the command line the user
//! typed, and SIGTERM to ask it to stop.

use std::process::{Child, Command};

pub fn shell_command(command_line: &str) -> Command {
    let mut command = Command::new("sh");
    command.arg("-c").arg(command_line);
    command
}

/// `sh -c <one command>` execs that command in place rather than forking, so the pid held
/// here is the worker itself and signalling it needs no tree walk.
pub fn request_stop(child: &Child) -> Result<(), String> {
    Command::new("kill")
        .args(["-15", &child.id().to_string()])
        .output()
        .map_err(|e| format!("failed to send SIGTERM: {e}"))?;
    Ok(())
}
