//! Everything that changes the machine: stopping and starting apps, dropping
//! caches, the full reset.
//!
//! Every signal goes through `signal_pass`, which scans `/proc` afresh, checks
//! each target against that scan — same start time (so not a reused pid), still
//! free of every guard in `protect`, and for the reset still the user's own —
//! and re-reads the start time once more immediately before signalling. Signals
//! go out through `kill(1)`, as on the Processes page: std has no way to signal
//! an arbitrary pid and `libc` isn't a direct dependency. What remains between
//! that last read and the signal is the time to spawn `kill`; a pid would have
//! to be freed and the whole pid space wrap around inside it.
//!
//! Windows has no signals. There SIGTERM's place is taken by WM_CLOSE to the process's
//! windows and SIGKILL's by TerminateProcess (see `windows::deliver`), which checks the
//! creation time on the very handle it then acts through — so the gap above doesn't
//! exist there. The checks before it are the same.

use super::catalog::{AppDef, AppKind};
use super::procfs::{self, Proc};
use super::protect::{self, Guard};
use super::sample::{Entry, Sample, Target};
use super::{ActionResult, AppState, ResetPreview, ResetRequest, ResetSummary, ResetTarget, SUPPORT};
use crate::elevate::{self, OpResult, Operation, OperationManifest};
use std::collections::{HashMap, HashSet};
#[cfg(not(windows))]
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::AppHandle;

/// How long a stopped app gets to exit on SIGTERM before SIGKILL.
const STOP_GRACE: Duration = Duration::from_secs(5);
/// The same for the full reset, which waits for it inline.
const RESET_GRACE: Duration = Duration::from_secs(1);
/// After SIGKILL, before counting what's left.
const KILL_SETTLE: Duration = Duration::from_millis(300);
/// Reset preview command lines are cut here; Chrome's run to kilobytes.
const COMMAND_PREVIEW_CHARS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sig {
    Term,
    Kill,
}

impl Sig {
    #[cfg_attr(windows, allow(dead_code))]
    fn name(self) -> &'static str {
        match self {
            Sig::Term => "TERM",
            Sig::Kill => "KILL",
        }
    }
}

/// Why a target was not signalled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// Not running any more.
    Gone,
    /// Its pid now belongs to a different process.
    Reused,
    /// DevsPro's tree, an ancestor, the desktop session, or pid 0/1.
    Protected,
    /// Not the user's process (full reset only).
    OtherUser,
}

/// The decision behind every signal, kept pure so it can be tested without
/// signalling anything. `fresh` is the target's pid as a scan taken just now
/// sees it.
pub fn check_target(target: &Target, fresh: Option<(&Proc, Guard)>, require_uid: Option<u32>, self_pid: u32) -> Result<(), Skip> {
    let Some((p, guard)) = fresh else {
        return Err(Skip::Gone);
    };
    if p.start_time != target.start_time {
        return Err(Skip::Reused);
    }
    if !p.is_live_userspace() {
        return Err(Skip::Gone);
    }
    if p.pid <= 1 || p.pid == self_pid || guard != Guard::Free {
        return Err(Skip::Protected);
    }
    if let Some(uid) = require_uid {
        if p.uid != Some(uid) {
            return Err(Skip::OtherUser);
        }
    }
    Ok(())
}

/// A scan with its guards, indexed by pid.
struct View {
    procs: Vec<Proc>,
    guards: Vec<Guard>,
    by_pid: HashMap<u32, usize>,
}

impl View {
    fn now() -> Self {
        let procs = procfs::scan();
        let guards = protect::classify(&procs, std::process::id());
        let by_pid = procs.iter().enumerate().map(|(i, p)| (p.pid, i)).collect();
        View { procs, guards, by_pid }
    }

    fn get(&self, pid: u32) -> Option<(&Proc, Guard)> {
        self.by_pid.get(&pid).map(|&i| (&self.procs[i], self.guards[i]))
    }
}

#[derive(Debug, Default)]
struct Pass {
    sent: Vec<Target>,
    /// Passed the check, then exited before its own signal — typically a
    /// child that went down with its parent a moment earlier in this pass.
    vanished: Vec<Target>,
    skipped: Vec<(Target, Skip)>,
    failed: Vec<(Target, String)>,
}

pub(super) enum SendError {
    Gone,
    Failed(String),
}

fn signal_pass(targets: &[Target], sig: Sig, require_uid: Option<u32>) -> Pass {
    let view = View::now();
    let self_pid = std::process::id();
    let mut pass = Pass::default();
    for target in targets {
        match check_target(target, view.get(target.pid), require_uid, self_pid) {
            Err(skip) => pass.skipped.push((target.clone(), skip)),
            Ok(()) => match send(target, sig) {
                Ok(()) => pass.sent.push(target.clone()),
                Err(SendError::Gone) => pass.vanished.push(target.clone()),
                Err(SendError::Failed(message)) => pass.failed.push((target.clone(), message)),
            },
        }
    }
    pass
}

fn send(target: &Target, sig: Sig) -> Result<(), SendError> {
    // Belt and braces: `kill 0` would signal DevsPro's whole process group.
    if target.pid <= 1 || target.pid == std::process::id() {
        return Err(SendError::Failed(format!("refusing to signal pid {}", target.pid)));
    }
    if procfs::read_start_time(target.pid) != Some(target.start_time) {
        return Err(SendError::Gone);
    }
    deliver(target, sig)
}

#[cfg(windows)]
fn deliver(target: &Target, sig: Sig) -> Result<(), SendError> {
    super::windows::deliver(target.pid, target.start_time, sig == Sig::Kill)
}

#[cfg(not(windows))]
fn deliver(target: &Target, sig: Sig) -> Result<(), SendError> {
    let output = Command::new("kill")
        .args(["-s", sig.name(), &target.pid.to_string()])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| SendError::Failed(format!("failed to run kill: {e}")))?;
    if output.status.success() {
        Ok(())
    } else if procfs::read_start_time(target.pid) != Some(target.start_time) {
        Err(SendError::Gone)
    } else {
        Err(SendError::Failed(String::from_utf8_lossy(&output.stderr).trim().to_string()))
    }
}

fn is_alive(target: &Target) -> bool {
    procfs::read_proc(target.pid).is_some_and(|p| p.start_time == target.start_time && p.is_live_userspace())
}

fn label(t: &Target) -> String {
    if t.comm.is_empty() {
        format!("pid {}", t.pid)
    } else {
        format!("{} (pid {})", t.comm, t.pid)
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What a stop reports once the polite request went out.
#[cfg(not(windows))]
fn asked_message(count: usize, name: &str) -> String {
    format!(
        "Sent SIGTERM to {} of {name}; anything still running in 5 s gets SIGKILL",
        plural(count, "process", "processes")
    )
}

#[cfg(windows)]
fn asked_message(count: usize, name: &str) -> String {
    format!(
        "Asked {} of {name} to close; anything still running in 5 s is ended",
        plural(count, "process", "processes")
    )
}

/// How the reset's summary names the forced step.
#[cfg(not(windows))]
const KILL_FAILED: &str = "SIGKILL failed";
#[cfg(windows)]
const KILL_FAILED: &str = "ending it failed";
#[cfg(not(windows))]
const STILL_RUNNING: &str = "still running after SIGKILL";
#[cfg(windows)]
const STILL_RUNNING: &str = "still running after being ended";

fn result(id: &str, ok: bool, message: impl Into<String>) -> ActionResult {
    ActionResult {
        id: id.to_string(),
        ok,
        message: message.into(),
    }
}

fn run_elevated(app: &AppHandle, operations: Vec<Operation>) -> Result<Vec<OpResult>, String> {
    elevate::run(
        app,
        OperationManifest {
            allowed_write_prefix: String::new(),
            operations,
        },
    )
}

/// Stops one non-service app: `flatpak kill` for a Flatpak, otherwise SIGTERM
/// to each of its processes and, from a background thread 5 s later, SIGKILL
/// to whichever are still the same process and still unprotected. Shared by the
/// Stop button and the watchdog.
pub fn stop_entry(entry: &Entry) -> ActionResult {
    let row = &entry.row;
    if row.protected {
        return result(&row.id, false, format!("{} is part of the desktop session or DevsPro itself, so it isn't stopped from here", row.name));
    }
    if row.state == AppState::Off {
        return result(&row.id, false, format!("{} is not running", row.name));
    }
    let flatpak_id = entry.def.as_ref().and_then(|d| d.flatpak_id.as_deref());
    match (row.kind, flatpak_id) {
        (AppKind::Service, _) => result(&row.id, false, format!("{} is a service; stopping it needs the admin password", row.name)),
        (AppKind::Flatpak, Some(flatpak_id)) => flatpak_kill(&row.id, &row.name, flatpak_id),
        _ => term_then_kill(&row.id, &row.name, &entry.targets),
    }
}

#[cfg(windows)]
fn flatpak_kill(id: &str, name: &str, _flatpak_id: &str) -> ActionResult {
    result(id, false, format!("{name} is a Flatpak app; Flatpak isn't available on Windows"))
}

#[cfg(not(windows))]
fn flatpak_kill(id: &str, name: &str, flatpak_id: &str) -> ActionResult {
    match Command::new("flatpak").args(["kill", flatpak_id]).stdin(Stdio::null()).output() {
        Ok(out) if out.status.success() => result(id, true, format!("Stopped {name}")),
        Ok(out) => result(id, false, format!("flatpak kill {flatpak_id} failed: {}", String::from_utf8_lossy(&out.stderr).trim())),
        Err(e) => result(id, false, format!("failed to run flatpak: {e}")),
    }
}

fn term_then_kill(id: &str, name: &str, targets: &[Target]) -> ActionResult {
    if targets.is_empty() {
        return result(id, false, format!("{name} has no processes to stop"));
    }
    let pass = signal_pass(targets, Sig::Term, None);
    if pass.sent.is_empty() {
        return match pass.failed.first() {
            Some((_, error)) => result(id, false, format!("Could not stop {name}: {error}")),
            None if pass.skipped.iter().any(|(_, s)| *s == Skip::Protected) => {
                result(id, false, format!("{name} is protected now, so it wasn't stopped"))
            }
            None => result(id, true, format!("{name} had already exited")),
        };
    }
    let sent = pass.sent.clone();
    std::thread::spawn(move || {
        std::thread::sleep(STOP_GRACE);
        signal_pass(&sent, Sig::Kill, None);
    });
    let mut message = asked_message(pass.sent.len(), name);
    if let Some((_, error)) = pass.failed.first() {
        message.push_str(&format!(" ({} could not be signalled: {error})", plural(pass.failed.len(), "process", "processes")));
    }
    result(id, true, message)
}

/// The Stop button, for any mix of apps. Service stops are collected into one
/// elevated manifest so a "Stop all" shows a single password prompt. The
/// helper stops at the first failed operation, so a failure there marks the
/// remaining services as skipped.
pub fn stop(app: &AppHandle, sample: &Sample, ids: &[String]) -> Vec<ActionResult> {
    let mut results: Vec<ActionResult> = Vec::with_capacity(ids.len());
    let mut services: Vec<(usize, String)> = Vec::new();
    for id in ids {
        let Some(entry) = sample.entry(id) else {
            results.push(result(id, false, "That app isn't in the list any more"));
            continue;
        };
        let row = &entry.row;
        let unit = entry.def.as_ref().and_then(|d| d.service.clone());
        match (row.kind, unit) {
            (AppKind::Service, Some(unit)) if !row.protected && row.state != AppState::Off && protect::valid_unit_name(&unit) => {
                services.push((results.len(), unit));
                results.push(result(id, false, "pending"));
            }
            (AppKind::Service, _) if row.protected => {
                results.push(result(id, false, format!("{} is a system service DevsPro won't stop", row.name)))
            }
            (AppKind::Service, _) if row.state == AppState::Off => {
                results.push(result(id, false, format!("{} is not running", row.name)))
            }
            (AppKind::Service, _) => results.push(result(id, false, format!("{} has no valid unit name", row.name))),
            _ => results.push(stop_entry(entry)),
        }
    }
    if services.is_empty() {
        return results;
    }
    let operations = services
        .iter()
        .map(|(_, unit)| Operation::ServiceControl {
            action: "stop".to_string(),
            unit: unit.clone(),
        })
        .collect();
    match run_elevated(app, operations) {
        Ok(ops) => {
            for (k, (i, _)) in services.iter().enumerate() {
                let (ok, message) = match ops.get(k) {
                    Some(r) => (r.success, r.message.clone()),
                    None => (false, "the helper returned no result".to_string()),
                };
                results[*i].ok = ok;
                results[*i].message = message;
            }
        }
        Err(e) => {
            for (i, _) in &services {
                results[*i].message = e.clone();
            }
        }
    }
    results
}

/// `~` or `~/…` at the very start, as the shell would — but with the home
/// directory single-quoted, so a space in it can't split the command. Not used on
/// Windows, where `cmd` has `%USERPROFILE%` instead and a quote means something else.
#[cfg_attr(windows, allow(dead_code))]
pub fn expand_tilde(command: &str, home: Option<&str>) -> String {
    let Some(home) = home else {
        return command.to_string();
    };
    let quoted = format!("'{}'", home.replace('\'', r"'\''"));
    if command == "~" {
        quoted
    } else if let Some(rest) = command.strip_prefix("~/") {
        format!("{quoted}/{rest}")
    } else {
        command.to_string()
    }
}

/// Starts `program` in its own session with no stdio, via `setsid -f`, which
/// forks and returns at once. The app is then a child of the session's
/// subreaper rather than of DevsPro — which matters: a DevsPro descendant is
/// invisible to this module, so an app started here would otherwise be one
/// the Stop button could never reach.
#[cfg(not(windows))]
fn spawn_detached(program: &str, args: &[&str]) -> Result<(), String> {
    let home = std::env::var_os("HOME");
    let mut command = Command::new("setsid");
    command.arg("-f").arg(program).args(args);
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(home) = &home {
        command.current_dir(home);
    }
    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
        // No util-linux: start it directly in its own process group. It stays
        // DevsPro's child (and so can't be stopped from this page) until
        // DevsPro exits.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut command = Command::new(program);
            command.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            if let Some(home) = &home {
                command.current_dir(home);
            }
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.process_group(0);
            }
            let mut child = command.spawn().map_err(|e| format!("failed to start {program}: {e}"))?;
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
        Err(e) => Err(format!("failed to run setsid: {e}")),
    }
}

pub fn start(app: &AppHandle, entry: &Entry) -> ActionResult {
    let row = &entry.row;
    let Some(def) = &entry.def else {
        return result(&row.id, false, format!("{} was detected running; DevsPro doesn't know how to start it", row.name));
    };
    if row.state != AppState::Off {
        return result(&row.id, false, format!("{} is already running", row.name));
    }
    let started = match def.kind {
        AppKind::Process => match &def.start {
            Some(cmd) => start_command(cmd),
            None => Err(format!("{} has no start command", row.name)),
        },
        AppKind::Flatpak => match &def.flatpak_id {
            Some(flatpak_id) => start_flatpak(flatpak_id),
            None => Err(format!("{} has no Flatpak id", row.name)),
        },
        AppKind::Service => return start_service(app, def),
    };
    match started {
        Ok(()) => result(&row.id, true, format!("Starting {}", row.name)),
        Err(e) => result(&row.id, false, e),
    }
}

#[cfg(not(windows))]
fn start_command(cmd: &str) -> Result<(), String> {
    let home = std::env::var("HOME").ok();
    spawn_detached("sh", &["-c", &expand_tilde(cmd, home.as_deref())])
}

#[cfg(windows)]
fn start_command(cmd: &str) -> Result<(), String> {
    super::windows::spawn_shell(cmd)
}

#[cfg(not(windows))]
fn start_flatpak(flatpak_id: &str) -> Result<(), String> {
    spawn_detached("flatpak", &["run", flatpak_id])
}

#[cfg(windows)]
fn start_flatpak(_flatpak_id: &str) -> Result<(), String> {
    Err("Flatpak isn't available on Windows".to_string())
}

fn start_service(app: &AppHandle, def: &AppDef) -> ActionResult {
    let Some(unit) = def.service.clone().filter(|u| protect::valid_unit_name(u)) else {
        return result(&def.id, false, format!("{} has no valid unit name", def.name));
    };
    let operations = vec![Operation::ServiceControl {
        action: "start".to_string(),
        unit,
    }];
    match run_elevated(app, operations) {
        Ok(ops) => match ops.into_iter().next() {
            Some(r) => result(&def.id, r.success, r.message),
            None => result(&def.id, false, "the helper returned no result"),
        },
        Err(e) => result(&def.id, false, e),
    }
}

/// DropCaches, then ReclaimSwap last: the helper skips everything after a
/// failed operation, and the swap guard is the step most likely to refuse.
///
/// Where swap can't be reclaimed (Windows) the helper isn't asked to; the second
/// result says why instead, so a caller reading `[cache, swap]` still gets both.
pub fn drop_caches(app: &AppHandle, reclaim_swap: bool) -> Result<Vec<OpResult>, String> {
    let mut operations = vec![Operation::DropCaches];
    if reclaim_swap && SUPPORT.reclaim_swap {
        operations.push(Operation::ReclaimSwap);
    }
    let mut results = run_elevated(app, operations)?;
    if reclaim_swap && !SUPPORT.reclaim_swap {
        results.push(OpResult {
            success: false,
            message: SWAP_UNSUPPORTED.to_string(),
        });
    }
    Ok(results)
}

/// Why there's no Reclaim swap on Windows.
const SWAP_UNSUPPORTED: &str = "not available on Windows: it has no way to move paged-out memory back into RAM \
     on demand — each app's pages come back as the app uses them";

fn truncate_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

/// Every process of the user's that the reset would stop — anything free of
/// every guard — labelled with the app it is counted under, plus the running,
/// unprotected catalog services.
pub fn reset_preview(sample: &Sample) -> ResetPreview {
    let mut app_of: HashMap<(u32, u64), &str> = HashMap::new();
    for entry in &sample.entries {
        for t in &entry.targets {
            app_of.entry((t.pid, t.start_time)).or_insert(entry.row.name.as_str());
        }
    }
    let mut processes: Vec<ResetTarget> = sample
        .procs
        .iter()
        .zip(&sample.guards)
        .filter(|(p, g)| **g == Guard::Free && p.is_live_userspace() && sample.my_uid.is_some() && p.uid == sample.my_uid)
        .map(|(p, _)| ResetTarget {
            pid: p.pid,
            start_time: p.start_time,
            name: p.comm.clone(),
            command: truncate_chars(&p.cmdline, COMMAND_PREVIEW_CHARS),
            ram_mb: p.rss_kb / 1024,
            app: app_of.get(&(p.pid, p.start_time)).map(|s| s.to_string()),
        })
        .collect();
    processes.sort_by(|a, b| {
        a.app
            .is_none()
            .cmp(&b.app.is_none())
            .then_with(|| a.app.cmp(&b.app))
            .then_with(|| b.ram_mb.cmp(&a.ram_mb))
    });
    let services = sample
        .entries
        .iter()
        .filter(|e| e.row.kind == AppKind::Service && e.row.state != AppState::Off && !e.row.protected)
        .filter_map(|e| e.def.as_ref().and_then(|d| d.service.clone()))
        .collect();
    ResetPreview { processes, services }
}

/// The full reset. Each requested (pid, start time) is checked again against a
/// fresh scan before SIGTERM and again before SIGKILL, so a reused pid, a
/// process that became protected, or one that isn't the user's is skipped no
/// matter what the request says. Services are accepted only if they are
/// catalog services; they and the cache steps go in one elevated manifest.
pub fn reset(app: &AppHandle, catalog: &[AppDef], request: ResetRequest) -> ResetSummary {
    let mut failed: Vec<String> = Vec::new();

    let catalog_units: HashSet<&str> = catalog
        .iter()
        .filter(|a| a.kind == AppKind::Service)
        .filter_map(|a| a.service.as_deref())
        .collect();
    let mut units: Vec<String> = Vec::new();
    for unit in request.services {
        if units.contains(&unit) {
            continue;
        }
        if !catalog_units.contains(unit.as_str()) {
            failed.push(format!("{unit}: not a service from the app list, skipped"));
        } else if !protect::valid_unit_name(&unit) || protect::is_protected_unit(&unit) {
            failed.push(format!("{unit}: a system service DevsPro won't stop, skipped"));
        } else {
            units.push(unit);
        }
    }

    let mut killed = 0u32;
    match procfs::my_uid() {
        None => failed.push("could not tell which user DevsPro runs as; no processes were stopped".to_string()),
        Some(uid) => {
            let mut seen = HashSet::new();
            let targets: Vec<Target> = request
                .processes
                .iter()
                .filter(|pair| seen.insert(**pair))
                .map(|&(pid, start_time)| Target {
                    pid,
                    start_time,
                    uid: Some(uid),
                    comm: procfs::read_proc(pid).map(|p| p.comm).unwrap_or_default(),
                })
                .collect();

            let term = signal_pass(&targets, Sig::Term, Some(uid));
            for (t, skip) in &term.skipped {
                match skip {
                    Skip::Gone => {}
                    Skip::Reused => failed.push(format!("pid {}: a different process has that pid now, skipped", t.pid)),
                    Skip::Protected => failed.push(format!("{}: protected, skipped", label(t))),
                    Skip::OtherUser => failed.push(format!("{}: not your process, skipped", label(t))),
                }
            }
            for (t, error) in &term.failed {
                failed.push(format!("{}: {error}", label(t)));
            }

            if !term.sent.is_empty() {
                std::thread::sleep(RESET_GRACE);
                let kill = signal_pass(&term.sent, Sig::Kill, Some(uid));
                for (t, error) in &kill.failed {
                    failed.push(format!("{}: {KILL_FAILED}: {error}", label(t)));
                }
                std::thread::sleep(KILL_SETTLE);
            }
            for t in &term.sent {
                if is_alive(t) {
                    failed.push(format!("{}: {STILL_RUNNING}", label(t)));
                } else {
                    killed += 1;
                }
            }
            killed += term.vanished.len() as u32;
        }
    }

    let mut operations: Vec<Operation> = units
        .into_iter()
        .map(|unit| Operation::ServiceControl {
            action: "stop".to_string(),
            unit,
        })
        .collect();
    if request.drop_caches {
        operations.push(Operation::DropCaches);
        if SUPPORT.reclaim_swap {
            operations.push(Operation::ReclaimSwap);
        }
    }
    let admin = if operations.is_empty() {
        Vec::new()
    } else {
        run_elevated(app, operations).unwrap_or_else(|e| {
            vec![OpResult {
                success: false,
                message: e,
            }]
        })
    };

    ResetSummary { killed, failed, admin }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::memory::protect::tests::proc;

    fn target(p: &Proc) -> Target {
        Target {
            pid: p.pid,
            start_time: p.start_time,
            uid: p.uid,
            comm: p.comm.clone(),
        }
    }

    #[test]
    fn signals_only_the_same_free_live_process() {
        let chrome = proc(8858, 1664, "chrome", "/opt/google/chrome/chrome");
        let t = target(&chrome);
        assert_eq!(check_target(&t, Some((&chrome, Guard::Free)), None, 500), Ok(()));
        assert_eq!(check_target(&t, Some((&chrome, Guard::Free)), Some(1000), 500), Ok(()));
        assert_eq!(check_target(&t, None, None, 500), Err(Skip::Gone));
    }

    #[test]
    fn a_reused_pid_is_never_signalled() {
        let old = proc(8858, 1664, "chrome", "/opt/google/chrome/chrome");
        let mut new = old.clone();
        new.start_time += 1;
        new.comm = "gnome-shell".to_string();
        assert_eq!(check_target(&target(&old), Some((&new, Guard::Kept)), None, 500), Err(Skip::Reused));
        new.comm = "chrome".to_string();
        assert_eq!(check_target(&target(&old), Some((&new, Guard::Free)), None, 500), Err(Skip::Reused));
    }

    #[test]
    fn guarded_self_and_init_are_refused() {
        let p = proc(8858, 1664, "chrome", "/opt/google/chrome/chrome");
        let t = target(&p);
        assert_eq!(check_target(&t, Some((&p, Guard::Kept)), None, 500), Err(Skip::Protected));
        assert_eq!(check_target(&t, Some((&p, Guard::Tree)), None, 500), Err(Skip::Protected));
        assert_eq!(check_target(&t, Some((&p, Guard::Free)), None, 8858), Err(Skip::Protected));
        let init = proc(1, 0, "init", "/sbin/init");
        assert_eq!(check_target(&target(&init), Some((&init, Guard::Free)), None, 500), Err(Skip::Protected));
    }

    #[test]
    fn reset_requires_the_users_own_process() {
        let mut p = proc(700, 1, "nginx", "nginx: worker process");
        p.uid = Some(33);
        assert_eq!(check_target(&target(&p), Some((&p, Guard::Free)), Some(1000), 500), Err(Skip::OtherUser));
    }

    #[test]
    fn zombies_and_kernel_threads_count_as_gone() {
        let mut z = proc(905, 1664, "defunct", "app");
        z.state = 'Z';
        assert_eq!(check_target(&target(&z), Some((&z, Guard::Free)), None, 500), Err(Skip::Gone));
        let k = proc(2, 0, "kthreadd", "");
        assert_eq!(check_target(&target(&k), Some((&k, Guard::Free)), None, 500), Err(Skip::Gone));
    }

    #[test]
    fn tilde_expands_only_at_the_start_and_quotes_home() {
        assert_eq!(
            expand_tilde("~/.dropbox-dist/dropbox-lnx.*/dropbox start", Some("/home/soab")),
            "'/home/soab'/.dropbox-dist/dropbox-lnx.*/dropbox start"
        );
        assert_eq!(expand_tilde("~", Some("/home/a b")), "'/home/a b'");
        assert_eq!(expand_tilde("~/x", Some("/home/o'neil")), r"'/home/o'\''neil'/x");
        assert_eq!(expand_tilde("firefox ~/page.html", Some("/home/soab")), "firefox ~/page.html");
        assert_eq!(expand_tilde("~other/bin/app", Some("/home/soab")), "~other/bin/app");
        assert_eq!(expand_tilde("~/x", None), "~/x");
    }

    #[test]
    fn preview_lists_only_the_users_free_processes_grouped_by_app() {
        use super::super::sample::{SampleInput, Sampler};
        use std::time::Instant;
        let mut postgres = proc(904, 1, "postgres", "postgres: checkpointer");
        postgres.uid = Some(999);
        let procs = vec![
            proc(1664, 1, "gnome-shell", "/usr/bin/gnome-shell"),
            proc(500, 1664, "devspro", "devspro"),
            proc(501, 500, "WebKitWebProces", "/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess"),
            proc(8858, 1664, "chrome", "/opt/google/chrome/chrome"),
            proc(902, 1664, "zsh", "/usr/bin/zsh"),
            proc(903, 902, "claude", &"claude ".repeat(100)),
            postgres,
        ];
        let catalog = [AppDef {
            id: "chrome".to_string(),
            name: "Chrome".to_string(),
            category: "Browsers".to_string(),
            kind: AppKind::Process,
            matcher: "/opt/google/chrome/chrome".to_string(),
            start: None,
            flatpak_id: None,
            service: None,
        }];
        let sample = Sampler::default().build(SampleInput {
            now: Instant::now(),
            now_ms: 0,
            procs,
            meminfo: Default::default(),
            catalog: &catalog,
            active_units: &HashSet::new(),
            self_pid: 500,
            my_uid: Some(1000),
        });
        let preview = reset_preview(&sample);
        let listed: Vec<(u32, Option<&str>)> = preview.processes.iter().map(|t| (t.pid, t.app.as_deref())).collect();
        assert_eq!(listed, [(8858, Some("Chrome")), (903, Some("claude"))]);
        assert!(preview.processes[1].command.ends_with('…'));
        assert_eq!(preview.processes[1].command.chars().count(), COMMAND_PREVIEW_CHARS + 1);
        assert!(preview.services.is_empty());
    }
}
