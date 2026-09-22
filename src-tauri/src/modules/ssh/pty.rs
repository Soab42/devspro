//! Terminal sessions: a child process (the user's shell, or the system `ssh`)
//! running inside a pseudo-terminal, with its output streamed to the webview
//! as events and keystrokes written back.
//!
//! The session id is chosen by the *frontend*, not generated here. Output is
//! emitted on `pty:output:<id>` as soon as the child starts, and a shell's
//! prompt arrives within milliseconds — sooner than a round trip to register
//! the listener if the id were only known once `pty_open` returned. Letting
//! the webview pick the id means it can subscribe first and never miss the
//! first bytes.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

use super::hosts::SshHost;
use super::SshState;

pub struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

pub type PtyMap = Mutex<HashMap<String, PtyHandle>>;

/// `"local"` for the user's shell, `{ "ssh": "<host id>" }` for a saved host.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum PtyCommand {
    Local(String),
    Ssh { ssh: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct PtyExit {
    pub code: Option<u32>,
}

const READ_CHUNK: usize = 8192;

/// The argv for the system `ssh`. `-p` rather than `user@host:port` because
/// the colon form isn't an ssh syntax at all. `ServerAliveInterval` keeps NAT
/// tables from silently dropping an idle session. `-i` only when the host has
/// its own key; otherwise ssh's normal key/agent lookup applies, which is what
/// a user with a plain `~/.ssh/id_*` expects. Password prompts are ssh's own
/// and show up in the terminal — nothing here handles them.
pub fn ssh_args(host: &SshHost) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        host.port.to_string(),
        "-o".to_string(),
        "ServerAliveInterval=30".to_string(),
    ];
    if host.auth == "key" {
        if let Some(key) = host.key_path.as_deref().filter(|k| !k.trim().is_empty()) {
            args.push("-i".to_string());
            args.push(key.to_string());
        }
    }
    args.push(format!("{}@{}", host.username, host.host));
    args
}

/// The interactive shell to run for a local terminal: `$SHELL`, else a safe
/// default. Started as a login shell (`-l`) so the user's profile — PATH
/// additions, nvm, prompt — is loaded, the same as a fresh terminal window.
pub fn local_shell(shell_env: Option<&str>) -> (String, Vec<String>) {
    let shell = shell_env
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("/bin/sh")
        .to_string();
    (shell, vec!["-l".to_string()])
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Spawns `cmd` in a fresh PTY. Returns the handle and a reader for the
/// output; the caller decides where the bytes go (an event stream in the app,
/// a buffer in tests).
pub fn spawn(cmd: CommandBuilder, cols: u16, rows: u16) -> Result<(PtyHandle, Box<dyn Read + Send>), String> {
    let pty = native_pty_system();
    let pair = pty.openpty(size(cols, rows)).map_err(|e| format!("failed to open pty: {e}"))?;
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("failed to start terminal process: {e}"))?;
    // The slave end must be closed in this process, or the reader never sees
    // EOF when the child exits (we'd still be holding the other end open).
    drop(pair.slave);
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("failed to read from pty: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("failed to write to pty: {e}"))?;
    Ok((
        PtyHandle {
            master: pair.master,
            writer,
            child,
        },
        reader,
    ))
}

fn valid_session_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err("invalid terminal session id".to_string())
    }
}

fn build_command(db: &State<crate::db::Db>, command: PtyCommand) -> Result<CommandBuilder, String> {
    let home = super::home_dir()?;
    let mut cmd = match command {
        PtyCommand::Local(tag) => {
            if tag != "local" {
                return Err(format!("unknown terminal command \"{tag}\""));
            }
            let (shell, args) = local_shell(std::env::var("SHELL").ok().as_deref());
            let mut cmd = CommandBuilder::new(shell);
            cmd.args(args);
            cmd
        }
        PtyCommand::Ssh { ssh: host_id } => {
            let host = {
                let conn = db.0.lock().map_err(|e| e.to_string())?;
                super::hosts::find(&conn, &host_id)?
            };
            let mut cmd = CommandBuilder::new("ssh");
            cmd.args(ssh_args(&host));
            cmd
        }
    };
    cmd.cwd(&home);
    // xterm.js speaks xterm-256color; without this, programs fall back to
    // whatever TERM the app inherited (often nothing under a desktop launcher).
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    Ok(cmd)
}

#[tauri::command]
pub fn pty_open(
    app: AppHandle,
    db: State<crate::db::Db>,
    state: State<SshState>,
    id: String,
    cols: u16,
    rows: u16,
    command: PtyCommand,
) -> Result<String, String> {
    valid_session_id(&id)?;
    {
        let map = state.pty.lock().map_err(|e| e.to_string())?;
        if map.contains_key(&id) {
            return Err("a terminal with this id is already open".to_string());
        }
    }
    let cmd = build_command(&db, command)?;
    let (handle, mut reader) = spawn(cmd, cols, rows)?;
    state.pty.lock().map_err(|e| e.to_string())?.insert(id.clone(), handle);

    // One reader thread per session. It ends when the child exits (EOF on the
    // master) or when `pty_close` kills the child, and it reaps the process so
    // the exit code can be reported.
    let output_event = format!("pty:output:{id}");
    let exit_event = format!("pty:exit:{id}");
    let thread_id = id.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; READ_CHUNK];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = app.emit(&output_event, STANDARD.encode(&buf[..n]));
                }
            }
        }
        let code = app
            .state::<SshState>()
            .pty
            .lock()
            .ok()
            .and_then(|mut map| map.remove(&thread_id))
            .and_then(|mut handle| handle.child.wait().ok())
            .map(|status| status.exit_code());
        let _ = app.emit(&exit_event, PtyExit { code });
    });

    Ok(id)
}

#[tauri::command]
pub fn pty_write(state: State<SshState>, id: String, data: String) -> Result<(), String> {
    let mut map = state.pty.lock().map_err(|e| e.to_string())?;
    let handle = map.get_mut(&id).ok_or("terminal is not open")?;
    handle
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| format!("write to terminal failed: {e}"))?;
    let _ = handle.writer.flush();
    Ok(())
}

#[tauri::command]
pub fn pty_resize(state: State<SshState>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    let map = state.pty.lock().map_err(|e| e.to_string())?;
    let handle = map.get(&id).ok_or("terminal is not open")?;
    handle
        .master
        .resize(size(cols, rows))
        .map_err(|e| format!("resize failed: {e}"))
}

/// Kills the child; the reader thread then sees EOF, reaps it, and emits the
/// exit event. The handle is left in the map for the thread to remove, so the
/// exit code still gets reported.
#[tauri::command]
pub fn pty_close(state: State<SshState>, id: String) -> Result<(), String> {
    let mut map = state.pty.lock().map_err(|e| e.to_string())?;
    if let Some(handle) = map.get_mut(&id) {
        let _ = handle.child.kill();
    }
    Ok(())
}

/// Tears down every open terminal. Called at app exit so no shells or ssh
/// connections outlive the window.
pub fn kill_all(state: &SshState) {
    if let Ok(mut map) = state.pty.lock() {
        for (_, mut handle) in map.drain() {
            let _ = handle.child.kill();
            let _ = handle.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(auth: &str, key_path: Option<&str>) -> SshHost {
        SshHost {
            id: "h".into(),
            name: "Box".into(),
            host: "example.com".into(),
            port: 2222,
            username: "deploy".into(),
            auth: auth.into(),
            key_path: key_path.map(String::from),
            created_at: String::new(),
        }
    }

    #[test]
    fn ssh_args_include_port_keepalive_and_target() {
        let args = ssh_args(&host("agent", None));
        assert_eq!(args, ["-p", "2222", "-o", "ServerAliveInterval=30", "deploy@example.com"]);
    }

    #[test]
    fn ssh_args_add_identity_only_for_key_auth() {
        let with = ssh_args(&host("key", Some("/home/me/.ssh/k")));
        assert!(with.windows(2).any(|w| w == ["-i", "/home/me/.ssh/k"]));
        let ignored = ssh_args(&host("password", Some("/home/me/.ssh/k")));
        assert!(!ignored.contains(&"-i".to_string()));
        let blank = ssh_args(&host("key", Some("  ")));
        assert!(!blank.contains(&"-i".to_string()));
    }

    #[test]
    fn local_shell_falls_back_to_sh() {
        assert_eq!(local_shell(Some("/usr/bin/zsh")), ("/usr/bin/zsh".to_string(), vec!["-l".to_string()]));
        assert_eq!(local_shell(Some("  ")).0, "/bin/sh");
        assert_eq!(local_shell(None).0, "/bin/sh");
    }

    #[test]
    fn session_ids_are_restricted() {
        assert!(valid_session_id("6f1c2b8e-1234-4abc-9def-0123456789ab").is_ok());
        assert!(valid_session_id("").is_err());
        assert!(valid_session_id("has space").is_err());
        assert!(valid_session_id("pty:output:x").is_err());
    }

    /// Real pty round trip: spawn `sh`, write a command, read its echo back,
    /// and confirm the child is reaped — the same primitives `pty_open` wires
    /// to events, minus the `AppHandle`.
    #[test]
    fn smoke_test_spawns_writes_and_reads_a_real_pty() {
        let mut cmd = CommandBuilder::new("sh");
        cmd.env("PS1", "$ ");
        let (mut handle, mut reader) = spawn(cmd, 80, 24).unwrap();
        handle.writer.write_all(b"echo devsuite-pty-ok; exit\n").unwrap();
        handle.writer.flush().unwrap();

        let mut collected = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => collected.extend_from_slice(&buf[..n]),
            }
        }
        let text = String::from_utf8_lossy(&collected);
        assert!(text.contains("devsuite-pty-ok"), "unexpected pty output: {text:?}");
        assert!(handle.child.wait().unwrap().success());
    }
}
