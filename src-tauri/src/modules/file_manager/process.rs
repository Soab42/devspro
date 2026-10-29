//! One place that runs an external tool: no shell, a hard timeout, and both
//! pipes drained on threads so a chatty tool never blocks on a full pipe.
//! `gio`, `git` and the code formatters all go through here, so each gets the
//! same guarantee that a hung phone mount or a stuck formatter cannot hang a
//! command.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// A tool's complaint is shown to the user; a runaway one must not flood the UI.
const MAX_STDERR_BYTES: usize = 4096;

/// Whether `program` can be started: an absolute (or relative) path that
/// exists, or a bare name found on `$PATH`. Callers use this to tell "not
/// installed" apart from "ran and failed", which `run` folds into one message.
pub(crate) fn installed(program: &str) -> bool {
    installed_in(program, &std::env::var("PATH").unwrap_or_default())
}

fn installed_in(program: &str, path_var: &str) -> bool {
    if is_path(program) {
        return Path::new(program).is_file();
    }
    find_on_path(program, path_var).is_some()
}

/// A program named by its location rather than looked up on `$PATH`.
fn is_path(program: &str) -> bool {
    program.contains('/') || (cfg!(windows) && program.contains('\\'))
}

/// The file `$PATH` resolves the bare name `bin` to, without running anything.
///
/// On Windows a tool is `git.exe`, `prettier.cmd` or `code.cmd`, never the
/// bare name, so each `PATHEXT` extension is tried in turn — the same order
/// `cmd.exe` uses. A name that already carries one of them is taken as is.
pub(crate) fn find_on_path(bin: &str, path_var: &str) -> Option<PathBuf> {
    let names = candidate_names(bin);
    std::env::split_paths(path_var).find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
}

#[cfg(unix)]
fn candidate_names(bin: &str) -> Vec<String> {
    vec![bin.to_string()]
}

#[cfg(windows)]
fn candidate_names(bin: &str) -> Vec<String> {
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| e.starts_with('.'))
        .collect();
    let lower = bin.to_ascii_lowercase();
    if exts.iter().any(|e| lower.ends_with(e.as_str())) {
        return vec![bin.to_string()];
    }
    exts.iter().map(|e| format!("{bin}{e}")).collect()
}

/// `Command::new(program)`, made to work the same on every OS.
///
/// On Windows the name is resolved through `PATHEXT` first — `CreateProcess`
/// only appends `.exe` itself, so `prettier` (really `prettier.cmd`) would
/// not start — and the child gets no console window, which would otherwise
/// flash up for every `git status` the editor runs.
pub(crate) fn command(program: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let resolved = if is_path(program) {
            None
        } else {
            find_on_path(program, &std::env::var("PATH").unwrap_or_default())
        };
        let mut cmd = match resolved {
            Some(path) => Command::new(path),
            None => Command::new(program),
        };
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }
    #[cfg(not(windows))]
    {
        Command::new(program)
    }
}

/// Runs `program args` with no shell, killed after `timeout`. `stdin` bytes
/// are fed on a thread and the pipe closed; without them stdin is null.
/// Stdout on success; the tool's own (trimmed, capped) stderr on failure,
/// since that is what explains a syntax error or a refused mount.
pub(crate) fn run<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    cwd: Option<&Path>,
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> Result<String, String> {
    run_bytes(program, args, cwd, stdin, timeout).map(|out| String::from_utf8_lossy(&out).into_owned())
}

/// `run` with stdout as raw bytes, so a caller can tell a binary file from
/// text instead of receiving replacement characters.
pub(crate) fn run_bytes<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    cwd: Option<&Path>,
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let mut cmd = command(program);
    cmd.args(args)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(|e| format!("could not run {program}: {e}"))?;
    // Written on a thread: a tool that answers before reading all of a large
    // buffer would otherwise deadlock against our own write.
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let bytes = bytes.to_vec();
        std::thread::spawn(move || {
            let _ = pipe.write_all(&bytes);
        });
    }
    let (tx, rx) = mpsc::channel::<(bool, Vec<u8>)>();
    for (is_stdout, pipe) in [
        (true, child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>)),
        (false, child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>)),
    ] {
        if let Some(mut pipe) = pipe {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                let _ = tx.send((is_stdout, buf));
            });
        }
    }
    drop(tx);
    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} gave no answer within {}s", timeout.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    while let Ok((is_stdout, bytes)) = rx.recv_timeout(Duration::from_secs(2)) {
        if is_stdout {
            out = bytes;
        } else {
            err = bytes;
        }
    }
    if status.success() {
        Ok(out)
    } else {
        let err = &err[..err.len().min(MAX_STDERR_BYTES)];
        let msg = String::from_utf8_lossy(err).trim().to_string();
        Err(if msg.is_empty() { format!("{program} exited with {status}") } else { msg })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn windows_finds_tools_by_their_pathext_extension() {
        let dir = std::env::temp_dir().join(format!("devspro-proc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("prettier.cmd"), "").unwrap();
        std::fs::write(dir.join("git.exe"), "").unwrap();
        // An extensionless shim (Git Bash's) is not something Windows can run.
        std::fs::write(dir.join("shim"), "").unwrap();
        let path_var = dir.to_string_lossy().into_owned();
        assert_eq!(find_on_path("prettier", &path_var), Some(dir.join("prettier.cmd")));
        assert_eq!(find_on_path("git", &path_var), Some(dir.join("git.exe")));
        assert_eq!(find_on_path("git.exe", &path_var), Some(dir.join("git.exe")));
        assert_eq!(find_on_path("shim", &path_var), None);
        assert!(installed_in(&dir.join("git.exe").to_string_lossy(), ""));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_runs_through_cmd_with_stdin_and_stderr() {
        let out = run("cmd", &["/C", "more"], None, Some(b"hello\r\n"), Duration::from_secs(10)).unwrap();
        assert_eq!(out.trim_end(), "hello");
        let err = run("cmd", &["/C", "echo boom 1>&2 & exit 3"], None, None, Duration::from_secs(10)).unwrap_err();
        assert_eq!(err, "boom");
        let err = run("ping", &["-n", "5", "127.0.0.1"], None, None, Duration::from_millis(200)).unwrap_err();
        assert!(err.contains("no answer"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn finds_programs_on_a_given_path_and_by_absolute_path() {
        assert!(installed_in("sh", "/bin:/usr/bin"));
        assert!(!installed_in("definitely-not-a-tool", "/bin:/usr/bin"));
        assert!(installed_in("/bin/sh", ""));
        assert!(!installed_in("/nonexistent/tool", "/bin"));
    }

    #[cfg(unix)]
    #[test]
    fn stdin_reaches_the_tool_and_stdout_comes_back() {
        let out = run("cat", &[] as &[&str], None, Some(b"hello"), Duration::from_secs(5)).unwrap();
        assert_eq!(out, "hello");
    }

    #[cfg(unix)]
    #[test]
    fn failure_returns_stderr_or_the_status() {
        let err = run("sh", &["-c", "echo boom >&2; exit 3"], None, None, Duration::from_secs(5)).unwrap_err();
        assert_eq!(err, "boom");
        let err = run("sh", &["-c", "exit 3"], None, None, Duration::from_secs(5)).unwrap_err();
        assert!(err.starts_with("sh exited with"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_hung_tool_is_killed_at_the_timeout() {
        let err = run("sleep", &["5"], None, None, Duration::from_millis(100)).unwrap_err();
        assert!(err.contains("no answer"), "{err}");
    }

    #[test]
    fn missing_program_is_a_spawn_error() {
        let err = run("definitely-not-a-tool", &[] as &[&str], None, None, Duration::from_secs(1)).unwrap_err();
        assert!(err.starts_with("could not run definitely-not-a-tool"), "{err}");
    }
}
