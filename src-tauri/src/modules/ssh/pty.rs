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
use tauri::{AppHandle, Emitter, Manager, State, Window};

use super::hosts::SshHost;
use super::SshState;

pub struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    /// Label of the webview window whose terminal drives this session. When
    /// that window is destroyed the shell is killed with it; a popped-out
    /// terminal transfers ownership with `pty_adopt` so the main window can
    /// close (or its tab can) without taking the session down.
    owner: String,
}

pub type PtyMap = Mutex<HashMap<String, PtyHandle>>;

/// `"local"` for the user's shell, `{ "ssh": "<host id>" }` for a saved host,
/// `{ "exec": { program, args, cwd } }` for one program (an AI coding agent,
/// an installer) run in the user's shell environment.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum PtyCommand {
    Local(String),
    Ssh { ssh: String },
    Exec { exec: ExecSpec },
}

#[derive(Debug, Deserialize)]
pub struct ExecSpec {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory; the home directory when absent or not a directory.
    #[serde(default)]
    pub cwd: Option<String>,
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

/// The argv that runs `program args…` through the user's interactive login
/// shell, replacing it: `$SHELL -l -i -c 'exec "$0" "$@"' program args…`.
/// The shell exists only to load the profile — PATH additions from nvm, pyenv
/// and the like live in `.zshrc`/`.bashrc`, which a bare `-l` shell skips —
/// and `exec` hands the PTY straight to the program, so the exit code the
/// terminal reports is the program's own.
pub fn exec_via_shell(shell: &str, program: &str, args: &[String]) -> (String, Vec<String>) {
    let mut argv = vec![
        "-l".to_string(),
        "-i".to_string(),
        "-c".to_string(),
        r#"exec "$0" "$@""#.to_string(),
        program.to_string(),
    ];
    argv.extend(args.iter().cloned());
    (shell.to_string(), argv)
}

/// A requested working directory, or the home directory when it is missing
/// or no longer a directory.
pub fn exec_cwd(requested: Option<&str>, home: &std::path::Path) -> std::path::PathBuf {
    requested
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| home.to_path_buf())
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
            owner: String::new(),
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
    let mut cwd = home.clone();
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
        PtyCommand::Exec { exec } => {
            let program = exec.program.trim();
            if program.is_empty() {
                return Err("nothing to run".to_string());
            }
            let (shell, _) = local_shell(std::env::var("SHELL").ok().as_deref());
            let (shell, args) = exec_via_shell(&shell, program, &exec.args);
            cwd = exec_cwd(exec.cwd.as_deref(), &home);
            let mut cmd = CommandBuilder::new(shell);
            cmd.args(args);
            cmd
        }
    };
    cmd.cwd(&cwd);
    // xterm.js speaks xterm-256color; without this, programs fall back to
    // whatever TERM the app inherited (often nothing under a desktop launcher).
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    Ok(cmd)
}

#[tauri::command]
pub fn pty_open(
    app: AppHandle,
    window: Window,
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
    let (mut handle, mut reader) = spawn(cmd, cols, rows)?;
    handle.owner = window.label().to_string();
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

/// Makes the calling window the owner of an already-running session. Used by
/// a popped-out terminal window: the session keeps running, only the window
/// whose destruction would kill it changes.
#[tauri::command]
pub fn pty_adopt(window: Window, state: State<SshState>, id: String) -> Result<(), String> {
    let mut map = state.pty.lock().map_err(|e| e.to_string())?;
    let handle = map.get_mut(&id).ok_or("terminal is not open")?;
    handle.owner = window.label().to_string();
    Ok(())
}

/// The working directory of the session's shell, read from `/proc`. Only
/// meaningful for a local terminal — for an ssh session the child is the
/// local `ssh` client, whose cwd says nothing about the remote side (that
/// comes from the shell's OSC 7 reports instead). `None` when the process
/// has gone or `/proc` is unreadable.
#[tauri::command]
pub fn pty_cwd(state: State<SshState>, id: String) -> Result<Option<String>, String> {
    let map = state.pty.lock().map_err(|e| e.to_string())?;
    let handle = map.get(&id).ok_or("terminal is not open")?;
    Ok(handle.child.process_id().and_then(proc_cwd))
}

pub fn proc_cwd(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Kills every session owned by a window that has just been destroyed. React
/// cleanup does not reliably run when a webview is torn down, so this is what
/// stops a popped-out terminal's shell from outliving its window.
pub fn kill_owned_by(state: &SshState, window_label: &str) {
    if let Ok(mut map) = state.pty.lock() {
        for handle in map.values_mut().filter(|h| h.owner == window_label) {
            let _ = handle.child.kill();
        }
    }
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
    fn exec_runs_through_an_interactive_login_shell_and_execs() {
        let (shell, argv) = exec_via_shell("/bin/zsh", "claude", &["--resume".to_string()]);
        assert_eq!(shell, "/bin/zsh");
        assert_eq!(argv, ["-l", "-i", "-c", r#"exec "$0" "$@""#, "claude", "--resume"]);
    }

    #[test]
    fn exec_cwd_falls_back_to_home() {
        let home = std::env::temp_dir();
        assert_eq!(exec_cwd(None, &home), home);
        assert_eq!(exec_cwd(Some("  "), &home), home);
        assert_eq!(exec_cwd(Some("/definitely/not/a/dir"), &home), home);
        assert_eq!(exec_cwd(Some(home.to_str().unwrap()), &home), home);
    }

    #[test]
    fn exec_command_deserialises_with_optional_fields() {
        let cmd: PtyCommand = serde_json::from_str(r#"{"exec":{"program":"codex"}}"#).unwrap();
        match cmd {
            PtyCommand::Exec { exec } => {
                assert_eq!(exec.program, "codex");
                assert!(exec.args.is_empty());
                assert_eq!(exec.cwd, None);
            }
            _ => panic!("expected exec"),
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
    fn proc_cwd_reads_this_process() {
        let cwd = proc_cwd(std::process::id()).expect("own cwd via /proc");
        assert_eq!(std::path::PathBuf::from(cwd), std::env::current_dir().unwrap());
        assert_eq!(proc_cwd(u32::MAX), None);
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
        handle.writer.write_all(b"echo devspro-pty-ok; exit\n").unwrap();
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
        assert!(text.contains("devspro-pty-ok"), "unexpected pty output: {text:?}");
        assert!(handle.child.wait().unwrap().success());
    }
}
