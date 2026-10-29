//! Linux: runs are found through `/proc`, ended with SIGTERM and then SIGKILL.
//!
//! A process belongs to a task when its stdout or stderr is the task's log —
//! what "Run now" gives its `sh -c` — or when it is a shell started with `-c`
//! and a command string naming the log. The second case is cron's wrapper,
//! `sh -c "cd <dir> && <cmd> >> <log> 2>&1"`: with a user command of `a && b`
//! only `b` carries the redirect, so the shell above it would otherwise go
//! unrecognised and `a` would survive. Matching on the open file, not on any
//! fd, keeps an editor or `tail -f` holding the log from being mistaken for a run.

use super::{roots, Entry};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

const SHELLS: &[&str] = &["sh", "dash", "bash"];

/// How long runs get to act on SIGTERM before SIGKILL, as for workers.
const GRACE: Duration = Duration::from_secs(2);

/// A process identified so a recycled pid is never signalled by mistake.
#[derive(Clone, Copy, PartialEq)]
struct Ident {
    pid: u32,
    start: u64,
}

struct Proc {
    ident: Ident,
    parent: u32,
    matches: bool,
}

/// `scheduled-logs/<id>.log`, compared by path components.
fn marker(id: &str) -> String {
    format!("scheduled-logs/{id}.log")
}

/// Ends every run of the task with its whole process tree: SIGTERM to all, then
/// SIGKILL to whatever is still there after the grace period. Returns how many
/// runs were found.
pub fn stop_runs(id: &str) -> usize {
    let marker = marker(id);
    let procs = snapshot(&marker);
    let entries: Vec<Entry> = procs.iter().map(|p| Entry { pid: p.ident.pid, parent: p.parent, matches: p.matches }).collect();
    let roots = roots(&entries);

    // Gathered before anything is signalled: once a parent dies its children
    // re-parent to init and the tree can no longer be walked.
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in &procs {
        children.entry(p.parent).or_default().push(p.ident.pid);
    }
    let by_pid: HashMap<u32, Ident> = procs.iter().map(|p| (p.ident.pid, p.ident)).collect();
    let mut targets: Vec<Ident> = Vec::new();
    let mut queue = roots.clone();
    while let Some(pid) = queue.pop() {
        if let Some(&ident) = by_pid.get(&pid) {
            if !targets.contains(&ident) {
                targets.push(ident);
                queue.extend(children.get(&pid).into_iter().flatten());
            }
        }
    }

    for t in &targets {
        // SAFETY: plain syscall on a pid; a stale one is guarded by the start time at snapshot.
        unsafe { libc::kill(t.pid as i32, libc::SIGTERM) };
    }
    let deadline = Instant::now() + GRACE;
    while Instant::now() < deadline && targets.iter().any(alive) {
        std::thread::sleep(Duration::from_millis(50));
    }
    for t in targets.iter().filter(|t| alive(t)) {
        // Re-checked just above; the window left is the few instructions between.
        // SAFETY: as above.
        unsafe { libc::kill(t.pid as i32, libc::SIGKILL) };
    }
    roots.len()
}

/// Still running: present, not a zombie awaiting its parent, same start time.
fn alive(t: &Ident) -> bool {
    match stat(t.pid) {
        Some((_, state, start)) => state != 'Z' && start == t.start,
        None => false,
    }
}

fn snapshot(marker: &str) -> Vec<Proc> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let (parent, _, start) = stat(pid)?;
            Some(Proc { ident: Ident { pid, start }, parent, matches: belongs(pid, marker) })
        })
        .collect()
}

/// (ppid, state, starttime) from `/proc/<pid>/stat`. The command name in field 2
/// may hold spaces and parentheses, so fields are counted from the last `)`.
fn stat(pid: u32) -> Option<(u32, char, u64)> {
    parse_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

fn parse_stat(text: &str) -> Option<(u32, char, u64)> {
    let rest = &text[text.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace();
    let state = fields.next()?.chars().next()?;
    let parent = fields.next()?.parse().ok()?;
    // Fields 3.. follow state and ppid; starttime is field 22 overall.
    let start = fields.nth(22 - 5)?.parse().ok()?;
    Some((parent, state, start))
}

fn belongs(pid: u32, marker: &str) -> bool {
    let logs_it = |fd: u32| {
        std::fs::read_link(format!("/proc/{pid}/fd/{fd}")).is_ok_and(|target| target.ends_with(Path::new(marker)))
    };
    logs_it(1) || logs_it(2) || is_shell_command_naming(pid, marker)
}

fn is_shell_command_naming(pid: u32, marker: &str) -> bool {
    let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let args: Vec<String> = raw.split(|&b| b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    let shell = args.first().and_then(|a| Path::new(a).file_name()).and_then(|n| n.to_str());
    shell.is_some_and(|s| SHELLS.contains(&s)) && args.iter().any(|a| a == "-c") && args.iter().skip(1).any(|a| a.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Child, Command, Stdio};

    /// Kills and reaps on drop, so a failed assertion leaves nothing behind.
    struct Guard(Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn temp_log() -> (String, std::path::PathBuf) {
        let id = uuid::Uuid::new_v4().to_string();
        let log = std::env::temp_dir().join("scheduled-logs").join(format!("{id}.log"));
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        (id, log)
    }

    /// `sh -c <script>` with stdout and stderr on the log, as "Run now" does.
    fn run_now(script: &str, log: &Path) -> Guard {
        let out = std::fs::File::create(log).unwrap();
        let err = out.try_clone().unwrap();
        let child = Command::new("sh").arg("-c").arg(script).stdout(out).stderr(err).spawn().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        Guard(child)
    }

    fn gone(pid: u32) -> bool {
        // Zombies count: an orphan's reaper may be slow, and it is not running.
        !alive_any(pid)
    }

    fn alive_any(pid: u32) -> bool {
        stat(pid).is_some_and(|(_, state, _)| state != 'Z')
    }

    fn pids_named(name: &str, parent: u32) -> Vec<u32> {
        let out = Command::new("pgrep").args(["-P", &parent.to_string(), name]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).split_whitespace().filter_map(|p| p.parse().ok()).collect()
    }

    #[test]
    fn parses_stat_with_parentheses_in_the_name() {
        let line = "42 (we ird) name)) S 7 42 42 0 -1 4194560 100 0 0 0 0 0 0 0 20 0 1 0 98765 1000 100 18446744073709551615";
        assert_eq!(parse_stat(line), Some((7, 'S', 98765)));
    }

    #[test]
    fn a_run_now_process_is_found_and_stopped() {
        let (id, log) = temp_log();
        let mut run = run_now("sleep 60", &log);
        assert_eq!(stop_runs(&id), 1);
        let status = run.0.wait().unwrap();
        assert!(status.signal().is_some());
        assert_eq!(stop_runs(&id), 0);
        let _ = std::fs::remove_file(log);
    }

    #[test]
    fn a_cron_shaped_run_is_found_by_its_shell_and_its_children_die() {
        let (id, log) = temp_log();
        let script = format!("cd /tmp && sleep 60 && true >> '{}' 2>&1", log.display());
        let child = Command::new("sh").arg("-c").arg(&script).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let mut run = Guard(child);
        std::thread::sleep(Duration::from_millis(300));
        let sleeps = pids_named("sleep", run.0.id());
        assert_eq!(sleeps.len(), 1);
        assert_eq!(stop_runs(&id), 1);
        assert!(run.0.wait().unwrap().signal().is_some());
        assert!(gone(sleeps[0]));
        let _ = std::fs::remove_file(log);
    }

    #[test]
    fn a_term_ignoring_run_is_killed_after_the_grace() {
        let (id, log) = temp_log();
        let mut run = run_now("trap '' TERM; sleep 60", &log);
        let started = Instant::now();
        assert_eq!(stop_runs(&id), 1);
        assert!(started.elapsed() >= GRACE);
        assert_eq!(run.0.wait().unwrap().signal(), Some(libc::SIGKILL));
        let _ = std::fs::remove_file(log);
    }

    #[test]
    fn a_reader_of_the_log_is_left_alone() {
        let (id, log) = temp_log();
        std::fs::write(&log, "").unwrap();
        let mut tail = Guard(Command::new("tail").arg("-f").arg(&log).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(stop_runs(&id), 0);
        assert!(tail.0.try_wait().unwrap().is_none());
        let _ = std::fs::remove_file(log);
    }
}
