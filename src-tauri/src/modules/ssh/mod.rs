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
pub mod key_perms;
pub mod keys;
pub mod pty;
pub mod session;
pub mod sftp;
pub mod stats;

use std::path::PathBuf;

/// Live handles for open terminals and SFTP sessions. Only meaningful while
/// the app runs — a terminal is a child process and an SFTP session a TCP
/// socket, neither of which survives a restart, so nothing here is persisted.
#[derive(Default)]
pub struct SshState {
    pub pty: pty::PtyMap,
    pub sftp: sftp::SftpMap,
}

/// `$HOME`, as a path. On Unix the environment variable is authoritative —
/// it's what `ssh` itself uses to find `~/.ssh`.
#[cfg(not(windows))]
pub fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| "HOME is not set; cannot locate ~/.ssh".to_string())
}

/// `%USERPROFILE%`, which is where Windows OpenSSH looks for `.ssh`. `HOME`
/// is deliberately ignored even when set (Git Bash sets it, sometimes to a
/// different drive): `ssh.exe` never reads it, so a key found through it
/// would not be the one ssh uses.
#[cfg(windows)]
pub fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| "USERPROFILE is not set; cannot locate ~/.ssh".to_string())
}

/// What to hand `Command::new` (or a PTY's `CommandBuilder`) to run one of
/// OpenSSH's programs, `ssh` or `ssh-keygen`. On Unix the bare name, exactly
/// as before. On Windows the copy on PATH, else the one the "OpenSSH Client"
/// optional feature puts in `System32\OpenSSH` — which is on PATH in a stock
/// install, but not always in the environment a desktop app inherits. Looked
/// up once per program: the key list runs `ssh-keygen` for every key.
pub fn openssh_program(tool: &str) -> String {
    #[cfg(windows)]
    {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        static FOUND: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
        let cache = FOUND.get_or_init(Default::default);
        if let Some(hit) = cache.lock().ok().and_then(|c| c.get(tool).cloned()) {
            return hit;
        }
        let found = crate::modules::tools::program(tool).or_else(|| {
            let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
            let bundled = root.join("System32").join("OpenSSH").join(format!("{tool}.exe"));
            bundled.is_file().then(|| bundled.to_string_lossy().into_owned())
        });
        match found {
            Some(path) => {
                if let Ok(mut c) = cache.lock() {
                    c.insert(tool.to_string(), path.clone());
                }
                path
            }
            // Not cached, so installing OpenSSH takes effect without a restart.
            None => tool.to_string(),
        }
    }
    #[cfg(not(windows))]
    {
        tool.to_string()
    }
}

/// A `Command` for one of OpenSSH's programs (see `openssh_program`). On
/// Windows it is started without a console window, which a GUI app's child
/// would otherwise flash up for every key listed.
pub fn openssh_command(tool: &str) -> std::process::Command {
    crate::modules::tools::command(openssh_program(tool))
}

/// Appended to a "failed to run ssh-keygen" error: on Windows the usual cause
/// is the OpenSSH Client optional feature being absent.
pub fn openssh_missing_hint() -> &'static str {
    if cfg!(windows) {
        " — install the \"OpenSSH Client\" optional feature (Settings › System › Optional features)"
    } else {
        ""
    }
}

pub fn ssh_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".ssh"))
}

/// Creates `~/.ssh` with the 700 mode OpenSSH insists on. A directory that is
/// group- or world-readable makes ssh refuse to use the keys inside it, so
/// creating it with default permissions would produce a confusing failure
/// later rather than an error here.
pub fn ensure_ssh_dir() -> Result<PathBuf, String> {
    let dir = ssh_dir()?;
    if !dir.is_dir() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
        // Windows has no mode bits; OpenSSH for Windows checks ACLs instead, and a
        // fresh directory under the user's profile already inherits owner-only ones.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| format!("failed to set permissions on {}: {e}", dir.display()))?;
        }
    }
    Ok(dir)
}
