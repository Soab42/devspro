//! Remote access, Termius-style: saved hosts, SSH keys, an in-app terminal,
//! and an SFTP file browser.
//!
//! Two different transports on purpose:
//!   - The **terminal** wraps the system `ssh` binary in a PTY (`pty.rs`). Its
//!     interactive prompts — password, host-key trust, 2FA — then just work,
//!     because the user is typing straight into ssh itself. Re-implementing
//!     that dialogue on top of a library session is a lot of surface area for
//!     a worse result.
//!   - **SFTP and key installation** use the `ssh2` crate (`session.rs`,
//!     `sftp.rs`, `keys.rs`), because a file browser needs a programmatic
//!     session with structured directory listings, not a stream of bytes.
//!     There, host-key trust and password prompts are explicit UI steps.
//!
//! Nothing here is elevated: everything runs as the user, against their own
//! `~/.ssh` and their own remote accounts. Passwords are never persisted; they
//! travel from the prompt to the session and are dropped with it.

pub mod hosts;
pub mod keys;
pub mod pty;
pub mod session;
pub mod sftp;

use std::path::PathBuf;

/// Live handles for open terminals and SFTP sessions. Only meaningful while
/// the app runs — a terminal is a child process and an SFTP session a TCP
/// socket, neither of which survives a restart, so nothing here is persisted.
#[derive(Default)]
pub struct SshState {
    pub pty: pty::PtyMap,
    pub sftp: sftp::SftpMap,
}

/// `$HOME`, as a path. Linux only for now, so the environment variable is
/// authoritative — it's what `ssh` itself uses to find `~/.ssh`.
pub fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| "HOME is not set; cannot locate ~/.ssh".to_string())
}

pub fn ssh_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".ssh"))
}

/// Creates `~/.ssh` with the 700 mode OpenSSH insists on. A directory that is
/// group- or world-readable makes ssh refuse to use the keys inside it, so
/// creating it with default permissions would produce a confusing failure
/// later rather than an error here.
pub fn ensure_ssh_dir() -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    let dir = ssh_dir()?;
    if !dir.is_dir() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("failed to set permissions on {}: {e}", dir.display()))?;
    }
    Ok(dir)
}
