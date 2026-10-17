//! One place that runs an external tool: no shell, a hard timeout, and both
//! pipes drained on threads so a chatty tool never blocks on a full pipe.
//! `gio`, `git` and the code formatters all go through here, so each gets the
//! same guarantee that a hung phone mount or a stuck formatter cannot hang a
//! command.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::Path;
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
    if program.contains('/') {
        return Path::new(program).is_file();
    }
    std::env::split_paths(path_var).any(|dir| dir.join(program).is_file())
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
    let mut cmd = Command::new(program);
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

    #[test]
    fn finds_programs_on_a_given_path_and_by_absolute_path() {
        assert!(installed_in("sh", "/bin:/usr/bin"));
        assert!(!installed_in("definitely-not-a-tool", "/bin:/usr/bin"));
        assert!(installed_in("/bin/sh", ""));
        assert!(!installed_in("/nonexistent/tool", "/bin"));
    }

    #[test]
    fn stdin_reaches_the_tool_and_stdout_comes_back() {
        let out = run("cat", &[] as &[&str], None, Some(b"hello"), Duration::from_secs(5)).unwrap();
        assert_eq!(out, "hello");
    }

    #[test]
    fn failure_returns_stderr_or_the_status() {
        let err = run("sh", &["-c", "echo boom >&2; exit 3"], None, None, Duration::from_secs(5)).unwrap_err();
        assert_eq!(err, "boom");
        let err = run("sh", &["-c", "exit 3"], None, None, Duration::from_secs(5)).unwrap_err();
        assert!(err.starts_with("sh exited with"), "{err}");
    }

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
