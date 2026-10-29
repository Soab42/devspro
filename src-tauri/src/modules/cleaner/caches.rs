//! Caches tab: package-manager and tool caches, thumbnails, trash. Scanning
//! sizes the catalog; cleaning takes ids only and re-resolves each one from the
//! catalog, so a path from the frontend is never used.

mod catalog;
mod procs;
#[cfg(windows)]
mod recycle_bin;

use super::{guard, history, measure, CleanOutcome, CleanerState, Finding, Scan};
use crate::db::Db;
use catalog::{catalog, root_for, suggested, Action, Entry, Host, Tool};
use procs::Proc;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, State};

const TAB: &str = "caches";

/// A tool's own clean command gets this long before it is killed.
const TOOL_TIMEOUT: Duration = Duration::from_secs(300);

/// `pnpm store path` only prints a line; it gets far less.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs `bin args` with no shell and nothing on stdin, in `cwd`. Killed after
/// `timeout`. Returns what it printed on stdout.
fn exec(bin: &Path, args: &[&str], cwd: &Path, timeout: Duration) -> Result<String, String> {
    let mut command = Command::new(bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // These are console programs; without this each one flashes a window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run {}: {e}", bin.display()))?;
    // Both pipes are drained while the tool runs, so a chatty one never blocks
    // on a full pipe; only the last OUTPUT_CAP bytes are kept (errors come at the end).
    let (tx, rx) = mpsc::channel::<(bool, String)>();
    if let Some(out) = child.stdout.take() {
        drain(out, true, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        drain(err, false, tx.clone());
    }
    drop(tx);
    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{} timed out after {}s", bin.display(), timeout.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };
    // A grandchild can keep a pipe open; do not wait on it for long.
    let mut out = String::new();
    while let Ok((is_stdout, text)) = rx.recv_timeout(Duration::from_secs(2)) {
        if is_stdout {
            out = text;
        }
    }
    if status.success() {
        Ok(out)
    } else {
        Err(format!("{} exited with {status}", bin.display()))
    }
}

const OUTPUT_CAP: usize = 64 * 1024;

fn drain(mut pipe: impl Read + Send + 'static, is_stdout: bool, tx: mpsc::Sender<(bool, String)>) {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buf = [0u8; 8192];
        while let Ok(n) = pipe.read(&mut buf) {
            if n == 0 {
                break;
            }
            kept.extend_from_slice(&buf[..n]);
            if kept.len() > OUTPUT_CAP {
                kept.drain(..kept.len() - OUTPUT_CAP);
            }
        }
        let _ = tx.send((is_stdout, String::from_utf8_lossy(&kept).into_owned()));
    });
}

fn run_tool(tool: &Tool, home: &Path) -> Result<(), String> {
    // From $HOME rather than the app's folder, so yarn and friends see no project.
    exec(&tool.bin, &tool.args, home, TOOL_TIMEOUT).map(|_| ())
}

/// An absolute directory from the environment.
fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// The real machine: $HOME, $PATH, pnpm's store and the managed Laravel sites.
fn real_host(db: &Db) -> Result<Host, String> {
    // Windows has no HOME; the profile folder is USERPROFILE.
    #[cfg(unix)]
    let home = env_dir("HOME").ok_or("HOME is not set")?;
    #[cfg(windows)]
    let home = env_dir("USERPROFILE").ok_or("USERPROFILE is not set")?;
    let path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    let mut host = Host { home: home.clone(), path, ..Host::default() };
    #[cfg(windows)]
    {
        host.local = env_dir("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData/Local"));
        host.roaming = env_dir("APPDATA").unwrap_or_else(|| home.join("AppData/Roaming"));
        host.temp = env_dir("TEMP");
        host.recycle_bins = recycle_bin::bins();
        if let Some(npm) = host.which("npm") {
            host.npm_cache = exec(&npm, &["config", "get", "cache"], &home, LOOKUP_TIMEOUT)
                .ok()
                .map(|s| PathBuf::from(s.trim()))
                .filter(|p| p.is_absolute());
        }
    }
    if let Some(pnpm) = host.which("pnpm") {
        host.pnpm_store = exec(&pnpm, &["store", "path"], &home, LOOKUP_TIMEOUT)
            .ok()
            .map(|s| PathBuf::from(s.trim()))
            .filter(|p| p.is_absolute());
    }
    host.rustup_env = std::env::var("RUSTUP_TOOLCHAIN").ok().filter(|t| !t.is_empty());
    if let Some(rustup) = host.which("rustup") {
        host.rustup_active = exec(&rustup, &["show", "active-toolchain"], &home, LOOKUP_TIMEOUT)
            .ok()
            .and_then(|s| s.split_whitespace().next().map(str::to_string));
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    // The sites table (nginx_ssl) keeps each Laravel site's folder in project_path.
    let mut stmt = conn
        .prepare("SELECT project_path FROM nginx_ssl_sites WHERE backend_type = 'laravel' AND project_path IS NOT NULL")
        .map_err(|e| e.to_string())?;
    host.laravel_sites = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .flatten()
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .collect();
    Ok(host)
}

fn blocked_by(entry: &Entry, procs: &[Proc]) -> Option<String> {
    entry.blockers.iter().find_map(|b| b.hit(procs))
}

fn scan_entries(host: &Host, procs: &[Proc], scan: &Scan) -> Vec<Finding> {
    let mut out = Vec::new();
    for entry in catalog(host) {
        if scan.is_cancelled() {
            break;
        }
        let usage: Vec<_> = entry.paths.iter().map(|p| measure::disk_usage(p, scan)).collect();
        let bytes: u64 = usage.iter().map(|u| u.bytes).sum();
        // A folder with nothing in it still occupies a block; that is not worth listing.
        let entries: u64 = usage.iter().map(|u| u.entries).sum();
        let only_roots = entry.action != Action::Truncate && entries <= entry.paths.len() as u64;
        if bytes == 0 || only_roots {
            continue;
        }
        out.push(Finding {
            id: entry.id.clone(),
            tab: TAB.to_string(),
            group: entry.group.to_string(),
            label: entry.label.clone(),
            reason: entry.reason.to_string(),
            method: entry.method(),
            paths: entry.paths.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
            bytes,
            risk: entry.risk,
            suggested: suggested(entry.risk, bytes),
            blocked_by: blocked_by(&entry, procs),
            needs_confirm_text: None,
        });
    }
    out
}

/// Removes what is inside `root`, keeping `root`. A symlink inside is unlinked
/// without touching its target; everything else goes through the guard.
fn empty_dir(root: &Path, exclusions: &[PathBuf]) -> Result<usize, String> {
    let (removed, errors) = empty_dir_each(root, exclusions)?;
    match errors.first() {
        None => Ok(removed),
        Some(first) => Err(format!("{} item(s) could not be removed, first: {first}", errors.len())),
    }
}

/// Removes a symlink itself. Windows keeps directory links (and junctions)
/// as directories, which `remove_file` refuses; `remove_dir` drops the link only.
fn unlink(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        if fs::symlink_metadata(path)?.file_type().is_symlink_dir() {
            return fs::remove_dir(path);
        }
    }
    fs::remove_file(path)
}

/// `empty_dir`'s work: how many children went, and why each other one did not.
fn empty_dir_each(root: &Path, exclusions: &[PathBuf]) -> Result<(usize, Vec<String>), String> {
    if !fs::symlink_metadata(root).map(|m| m.is_dir()).unwrap_or(false) {
        return Err(format!("{} is not a plain directory", root.display()));
    }
    let read = fs::read_dir(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let (mut removed, mut errors) = (0, Vec::new());
    for child in read.flatten() {
        let path = child.path();
        let is_link = fs::symlink_metadata(&path).map(|m| m.file_type().is_symlink()).unwrap_or(false);
        let result = if is_link {
            unlink(&path).map_err(|e| format!("{}: {e}", path.display()))
        } else {
            guard::check_removable(&path, &[root], exclusions).and_then(|_| guard::remove_path(&path, true))
        };
        match result {
            Ok(()) => removed += 1,
            Err(e) => errors.push(e),
        }
    }
    Ok((removed, errors))
}

/// Runs the tool's command if the entry has one, else works on its directories.
fn clean_one(entry: &Entry, exclusions: &[PathBuf], run: &dyn Fn(&Tool) -> Result<(), String>) -> Result<String, String> {
    if let Some(tool) = &entry.tool {
        run(tool)?;
        return Ok(format!("ran {}", tool.display()));
    }
    #[cfg(windows)]
    match entry.action {
        Action::RecycleBin => {
            // The shell empties every bin at once, so an exclusion inside one
            // cannot be honoured item by item; refuse instead.
            if let Some(p) = entry.paths.iter().find(|p| exclusions.iter().any(|e| e.starts_with(p) || p.starts_with(e))) {
                return Err(format!("{} is excluded in the Cleaner settings", p.display()));
            }
            recycle_bin::empty()?;
            return Ok("emptied the Recycle Bin".to_string());
        }
        Action::Sweep => {
            let (mut removed, mut skipped) = (0, 0);
            for path in &entry.paths {
                let (r, errors) = empty_dir_each(path, exclusions)?;
                removed += r;
                skipped += errors.len();
            }
            return Ok(match skipped {
                0 => format!("removed {removed} item(s)"),
                _ => format!("removed {removed} item(s), skipped {skipped} in use or excluded"),
            });
        }
        _ => {}
    }
    let mut count = 0;
    for path in &entry.paths {
        count += match entry.action {
            Action::Empty => empty_dir(path, exclusions)?,
            #[cfg(windows)]
            Action::Sweep | Action::RecycleBin => unreachable!("handled above"),
            Action::Remove => {
                guard::check_removable(path, &[root_for(entry, path)], exclusions)?;
                guard::remove_path(path, true)?;
                1
            }
            Action::Truncate => {
                guard::check_removable(path, &[root_for(entry, path)], exclusions)?;
                fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                1
            }
        };
    }
    Ok(match entry.action {
        Action::Truncate => "emptied the file".to_string(),
        _ => format!("removed {count} item(s)"),
    })
}

/// One outcome per requested id, paired with the label to record in history.
fn clean_entries(
    host: &Host,
    procs: &[Proc],
    exclusions: &[PathBuf],
    ids: &[String],
    run: &dyn Fn(&Tool) -> Result<(), String>,
) -> Vec<(String, CleanOutcome)> {
    clean_entries_with(host, procs, exclusions, ids, run, &|p| measure::free_bytes(p).ok())
}

/// `clean_entries` with the free-space reading injectable: `free` is asked
/// about a path and answers with the bytes available there.
fn clean_entries_with(
    host: &Host,
    procs: &[Proc],
    exclusions: &[PathBuf],
    ids: &[String],
    run: &dyn Fn(&Tool) -> Result<(), String>,
    free: &dyn Fn(&Path) -> Option<u64>,
) -> Vec<(String, CleanOutcome)> {
    let entries = catalog(host);
    let mut done: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for id in ids {
        if done.contains(id) {
            continue;
        }
        done.push(id.clone());
        let fail = |label: &str, message: String| {
            (label.to_string(), CleanOutcome { id: id.clone(), ok: false, message, bytes_freed: 0 })
        };
        let Some(entry) = entries.iter().find(|e| &e.id == id) else {
            out.push(fail(id, "unknown cache id".to_string()));
            continue;
        };
        if let Some(why) = blocked_by(entry, procs) {
            out.push(fail(&entry.label, format!("not cleaned: {why}")));
            continue;
        }
        let before = free(&entry.probe());
        match clean_one(entry, exclusions, run) {
            Ok(message) => {
                let after = free(&entry.probe());
                let freed = match (before, after) {
                    (Some(b), Some(a)) => a.saturating_sub(b),
                    _ => 0,
                };
                out.push((entry.label.clone(), CleanOutcome { id: id.clone(), ok: true, message, bytes_freed: freed }));
            }
            Err(e) => out.push(fail(&entry.label, e)),
        }
    }
    out
}

#[tauri::command(async)]
pub fn cleaner_scan_caches(
    app: AppHandle,
    state: State<CleanerState>,
    db: State<Db>,
    scan_id: String,
) -> Result<Vec<Finding>, String> {
    let host = real_host(&db)?;
    let scan = Scan::start(&state, &app, &scan_id, TAB);
    Ok(scan_entries(&host, &procs::snapshot(), &scan))
}

#[tauri::command(async)]
pub fn cleaner_clean_caches(
    _app: AppHandle,
    db: State<Db>,
    ids: Vec<String>,
) -> Result<Vec<CleanOutcome>, String> {
    let host = real_host(&db)?;
    let exclusions: Vec<PathBuf> = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        history::load_settings(&conn).exclusions.into_iter().map(PathBuf::from).collect()
    };
    let results = clean_entries(&host, &procs::snapshot(), &exclusions, &ids, &|t| run_tool(t, &host.home));
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    for (label, o) in &results {
        history::record(&conn, TAB, label, o.bytes_freed, o.ok, &o.message)?;
    }
    Ok(results.into_iter().map(|(_, o)| o).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::modules::cleaner::Risk;
    use procs::Blocker;
    use std::cell::RefCell;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("devspro-caches-run-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn host(home: &Path) -> Host {
        Host { home: home.to_path_buf(), ..Host::default() }
    }

    fn fill(dir: &Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), vec![7u8; 8192]).unwrap();
    }

    fn no_tool(_: &Tool) -> Result<(), String> {
        panic!("no tool should run");
    }

    fn clean(h: &Host, procs: &[Proc], excl: &[PathBuf], ids: &[&str]) -> Vec<CleanOutcome> {
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        clean_entries(h, procs, excl, &ids, &no_tool).into_iter().map(|(_, o)| o).collect()
    }

    #[cfg(unix)]
    #[test]
    fn scan_reports_size_risk_reason_method_and_no_suggestion_when_small() {
        let home = scratch("scan");
        fill(&home.join(".cache/pip"), "wheel");
        fill(&home.join(".gradle/caches"), "jar");
        fs::create_dir_all(home.join(".cache/thumbnails")).unwrap();
        let found = scan_entries(&host(&home), &[], &Scan::detached());
        assert_eq!(found.len(), 2, "an empty cache is not listed");
        let pip = found.iter().find(|f| f.id == "pip").unwrap();
        assert!(pip.bytes >= 8192);
        assert_eq!((pip.tab.as_str(), pip.group.as_str(), pip.risk), ("caches", "Package managers", Risk::Safe));
        assert!(!pip.reason.is_empty() && pip.method == "delete" && !pip.suggested);
        assert_eq!(pip.paths, vec![home.join(".cache/pip").to_string_lossy().into_owned()]);
        assert_eq!(found.iter().find(|f| f.id == "gradle").unwrap().risk, Risk::Review);
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn scan_sets_blocked_by_for_a_running_owner() {
        let home = scratch("blocked-scan");
        fill(&home.join(".cache/mozilla/firefox"), "cache2");
        let procs = [Proc { comm: "firefox".into(), args: vec![] }];
        let found = scan_entries(&host(&home), &procs, &Scan::detached());
        assert_eq!(found[0].blocked_by.as_deref(), Some("Firefox is running"));
        assert!(scan_entries(&host(&home), &[], &Scan::detached())[0].blocked_by.is_none());
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn a_cancelled_scan_returns_what_it_has() {
        let home = scratch("cancel");
        fill(&home.join(".cache/pip"), "w");
        let scan = Scan::detached();
        scan.cancel();
        assert!(scan_entries(&host(&home), &[], &scan).is_empty());
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn unknown_ids_are_rejected_and_nothing_is_touched() {
        let home = scratch("unknown");
        fill(&home.join(".cache/pip"), "w");
        let out = clean(&host(&home), &[], &[], &["../../etc", "/etc/passwd", "pip-typo"]);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|o| !o.ok && o.message == "unknown cache id" && o.bytes_freed == 0));
        assert!(home.join(".cache/pip/w").exists());
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn empty_keeps_the_root_and_removes_its_contents() {
        let home = scratch("empty");
        fill(&home.join(".cache/pip"), "a");
        fill(&home.join(".cache/pip/http/0"), "b");
        let out = clean(&host(&home), &[], &[], &["pip", "pip"]);
        assert_eq!(out.len(), 1, "a repeated id is cleaned once");
        assert!(out[0].ok, "{}", out[0].message);
        assert!(home.join(".cache/pip").is_dir());
        assert_eq!(fs::read_dir(home.join(".cache/pip")).unwrap().count(), 0);
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn empty_honours_exclusions_and_reports_the_failure() {
        let home = scratch("excl");
        fill(&home.join(".cache/pip"), "a");
        fill(&home.join(".cache/pip/keep"), "b");
        let out = clean(&host(&home), &[], &[home.join(".cache/pip/keep")], &["pip"]);
        assert!(!out[0].ok);
        assert!(home.join(".cache/pip/keep/b").exists());
        assert!(!home.join(".cache/pip/a").exists());
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_inside_a_cache_is_unlinked_not_followed() {
        let home = scratch("link");
        let target = scratch("link-target");
        fs::write(target.join("precious"), "x").unwrap();
        fill(&home.join(".cache/pip"), "a");
        std::os::unix::fs::symlink(&target, home.join(".cache/pip/ln")).unwrap();
        assert!(clean(&host(&home), &[], &[], &["pip"])[0].ok);
        assert!(target.join("precious").exists());
        assert!(fs::symlink_metadata(home.join(".cache/pip/ln")).is_err());
        fs::remove_dir_all(&home).unwrap();
        fs::remove_dir_all(&target).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_tool_runs_with_its_fixed_argv_and_the_directory_is_left_alone() {
        let home = scratch("tool");
        fill(&home.join(".npm/_cacache"), "blob");
        let bindir = scratch("tool-bin");
        let npm = bindir.join("npm");
        fs::write(&npm, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&npm, fs::Permissions::from_mode(0o755)).unwrap();
        let mut h = host(&home);
        h.path = vec![bindir.clone()];
        let seen: RefCell<Vec<(PathBuf, Vec<&str>)>> = RefCell::new(Vec::new());
        let run = |t: &Tool| {
            seen.borrow_mut().push((t.bin.clone(), t.args.clone()));
            Ok(())
        };
        let out = clean_entries(&h, &[], &[], &["npm".to_string()], &run);
        assert!(out[0].1.ok);
        assert_eq!(*seen.borrow(), vec![(npm, vec!["cache", "clean", "--force"])]);
        assert!(home.join(".npm/_cacache/blob").exists(), "the tool owns the cleaning");
        // A failing tool is a failed outcome, not a fallback to deleting.
        let bad = |_: &Tool| Err("exit 1".to_string());
        let out = clean_entries(&h, &[], &[], &["npm".to_string()], &bad);
        assert!(!out[0].1.ok && home.join(".npm/_cacache/blob").exists());
        fs::remove_dir_all(&home).unwrap();
        fs::remove_dir_all(&bindir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_blocked_entry_is_refused_at_clean_time() {
        let home = scratch("blocked-clean");
        fill(&home.join(".config/Code/Cache"), "x");
        let procs = [Proc { comm: "code".into(), args: vec![] }];
        let out = clean(&host(&home), &procs, &[], &["vscode"]);
        assert!(!out[0].ok && out[0].message.contains("VS Code is running"));
        assert!(home.join(".config/Code/Cache/x").exists());
        assert!(Blocker::Comm("VS Code", &["code"]).hit(&procs).is_some());
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn remove_deletes_the_version_directory_itself() {
        let home = scratch("remove");
        fill(&home.join(".nvm/versions/node/v18.1.0/bin"), "node");
        fill(&home.join(".nvm/versions/node/v20.0.0/bin"), "node");
        fs::create_dir_all(home.join(".nvm/alias")).unwrap();
        fs::write(home.join(".nvm/alias/default"), "20").unwrap();
        let out = clean(&host(&home), &[], &[], &["nvm-node-v18.1.0", "nvm-node-v20.0.0"]);
        assert!(out[0].ok, "{}", out[0].message);
        assert!(!out[1].ok, "the default version is not in the catalog");
        assert!(!home.join(".nvm/versions/node/v18.1.0").exists());
        assert!(home.join(".nvm/versions/node/v20.0.0/bin/node").exists());
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn laravel_logs_are_truncated_not_deleted() {
        let home = scratch("log");
        let project = home.join("sites/shop");
        let logs = project.join("storage/logs");
        fs::create_dir_all(&logs).unwrap();
        let f = fs::File::create(logs.join("laravel.log")).unwrap();
        f.set_len(catalog::LOG_MIN_BYTES + 1).unwrap();
        let mut h = host(&home);
        h.laravel_sites = vec![project];
        let id = format!("laravel-log:{}", logs.join("laravel.log").display());
        let out = clean(&h, &[], &[], &[&id]);
        assert!(out[0].ok, "{}", out[0].message);
        assert_eq!(fs::metadata(logs.join("laravel.log")).unwrap().len(), 0);
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn exec_kills_a_command_that_overruns_its_timeout() {
        let sleep = Path::new("/bin/sleep");
        assert!(sleep.exists());
        let start = Instant::now();
        let err = exec(sleep, &["5"], Path::new("/"), Duration::from_millis(150)).unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(exec(Path::new("/bin/true"), &[], Path::new("/"), TOOL_TIMEOUT).is_ok());
        assert!(exec(Path::new("/bin/false"), &[], Path::new("/"), TOOL_TIMEOUT).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn exec_keeps_the_tail_of_a_flood_on_both_pipes() {
        let out = exec(
            Path::new("/bin/sh"),
            &["-c", "yes out | head -c 3000000; yes err | head -c 3000000 >&2; echo final-line"],
            Path::new("/"),
            Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(out.len(), OUTPUT_CAP);
        assert!(out.ends_with("final-line\n"), "the last output is kept, not the first");
    }

    #[cfg(unix)]
    #[test]
    fn bytes_freed_is_the_free_space_delta_read_on_the_surviving_ancestor() {
        let home = scratch("freed");
        fill(&home.join(".nvm/versions/node/v18.1.0/bin"), "node");
        fill(&home.join(".nvm/versions/node/v20.0.0"), "x");
        fs::create_dir_all(home.join(".nvm/alias")).unwrap();
        fs::write(home.join(".nvm/alias/default"), "20").unwrap();
        let cleaned = home.join(".nvm/versions/node/v18.1.0");
        let asked: RefCell<Vec<PathBuf>> = RefCell::new(Vec::new());
        // Like statvfs: fails for a path that no longer exists.
        let free = |p: &Path| {
            asked.borrow_mut().push(p.to_path_buf());
            if !p.exists() {
                return None;
            }
            Some(if asked.borrow().len() == 1 { 1_000 } else { 5_000 })
        };
        let out = clean_entries_with(&host(&home), &[], &[], &["nvm-node-v18.1.0".to_string()], &no_tool, &free);
        assert!(out[0].1.ok, "{}", out[0].1.message);
        assert!(!cleaned.exists());
        assert_eq!(out[0].1.bytes_freed, 4_000);
        let surviving = home.join(".nvm/versions/node");
        assert_eq!(*asked.borrow(), vec![surviving.clone(), surviving]);
        fs::remove_dir_all(&home).unwrap();
    }

    /// A Windows profile: Local and Roaming under AppData, as on a real machine.
    #[cfg(windows)]
    fn win_host(home: &Path) -> Host {
        Host { local: home.join("AppData/Local"), roaming: home.join("AppData/Roaming"), ..host(home) }
    }

    #[cfg(windows)]
    #[test]
    fn windows_scan_sizes_and_clean_empties_a_cache() {
        let home = scratch("win-scan");
        let pip = home.join("AppData/Local/pip/Cache");
        fill(&pip, "wheel");
        fill(&pip.join("http/0"), "b");
        let h = win_host(&home);
        let found = scan_entries(&h, &[], &Scan::detached());
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].id.as_str(), found[0].method.as_str()), ("pip", "delete"));
        assert!(found[0].bytes >= 8192);
        let out = clean(&h, &[], &[], &["pip"]);
        assert!(out[0].ok, "{}", out[0].message);
        assert!(pip.is_dir() && fs::read_dir(&pip).unwrap().count() == 0);
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_a_junction_inside_a_cache_is_unlinked_not_followed() {
        let home = scratch("win-junction");
        let target = scratch("win-junction-target");
        fs::write(target.join("precious"), "x").unwrap();
        // Backslashes throughout: mklink reads a "/" as the start of a switch.
        let pip = home.join("AppData").join("Local").join("pip").join("Cache");
        fill(&pip, "a");
        // A junction needs no privilege, unlike a symlink.
        let made = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(pip.join("ln"))
            .arg(&target)
            .output()
            .unwrap();
        assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
        let out = clean(&win_host(&home), &[], &[], &["pip"]);
        assert!(out[0].ok, "{}", out[0].message);
        assert!(target.join("precious").exists());
        assert!(fs::symlink_metadata(pip.join("ln")).is_err());
        fs::remove_dir_all(&home).unwrap();
        fs::remove_dir_all(&target).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_temp_sweep_skips_a_file_in_use() {
        use std::os::windows::fs::OpenOptionsExt;
        let home = scratch("win-temp");
        let temp = home.join("AppData/Local/Temp");
        fill(&temp, "old");
        fill(&temp.join("dir"), "older");
        fs::write(temp.join("busy"), "x").unwrap();
        // Held open with no sharing, as a running program's temp file would be.
        let held = fs::OpenOptions::new().read(true).share_mode(0).open(temp.join("busy")).unwrap();
        let mut h = win_host(&home);
        h.temp = Some(temp.clone());
        let out = clean(&h, &[], &[], &["temp"]);
        assert!(out[0].ok, "{}", out[0].message);
        assert_eq!(out[0].message, "removed 2 item(s), skipped 1 in use or excluded");
        assert!(temp.join("busy").exists() && !temp.join("old").exists() && !temp.join("dir").exists());
        drop(held);
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_a_tool_runs_through_its_cmd_shim() {
        let home = scratch("win-tool");
        fill(&home.join("AppData/Local/npm-cache/_cacache"), "blob");
        let bindir = scratch("win-tool-bin");
        fs::write(bindir.join("npm.cmd"), "@echo off\r\n").unwrap();
        let mut h = win_host(&home);
        h.path = vec![bindir.clone()];
        let seen: RefCell<Vec<(PathBuf, Vec<&str>)>> = RefCell::new(Vec::new());
        let run = |t: &Tool| {
            seen.borrow_mut().push((t.bin.clone(), t.args.clone()));
            run_tool(t, &home)
        };
        let out = clean_entries(&h, &[], &[], &["npm".to_string()], &run);
        assert!(out[0].1.ok, "{}", out[0].1.message);
        assert_eq!(*seen.borrow(), vec![(bindir.join("npm.cmd"), vec!["cache", "clean", "--force"])]);
        fs::remove_dir_all(&home).unwrap();
        fs::remove_dir_all(&bindir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_a_running_editor_blocks_its_cache() {
        let home = scratch("win-blocked");
        fill(&home.join("AppData/Roaming/Code/Cache"), "x");
        let procs = [Proc { comm: "code".into(), args: vec![] }];
        let out = clean(&win_host(&home), &procs, &[], &["vscode"]);
        assert!(!out[0].ok && out[0].message.contains("VS Code is running"));
        assert!(home.join("AppData/Roaming/Code/Cache/x").exists());
        assert!(Blocker::Comm("VS Code", &["code"]).hit(&procs).is_some());
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_exec_kills_a_command_that_overruns_its_timeout() {
        let system = PathBuf::from(std::env::var("SystemRoot").unwrap()).join("System32");
        let start = Instant::now();
        let err = exec(&system.join("ping.exe"), &["-n", "10", "127.0.0.1"], &system, Duration::from_millis(300)).unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(5));
        let cmd = system.join("cmd.exe");
        assert_eq!(exec(&cmd, &["/C", "echo", "hi"], &system, TOOL_TIMEOUT).unwrap().trim(), "hi");
        assert!(exec(&cmd, &["/C", "exit", "1"], &system, TOOL_TIMEOUT).is_err());
    }
}
