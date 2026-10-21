//! The System tab on Windows.
//!
//! Two items belong to the user and are cleaned here, as the user, with no
//! prompt: Explorer's thumbnail cache and the user's own error reports. (The
//! Recycle Bin, %TEMP% and `CrashDumps` are offered by the Caches tab, so they
//! are not repeated here.) Four live under the Windows folder and need the
//! helper: Windows'
//! own temp folder, the Windows Update download cache, the Delivery
//! Optimization cache and machine-wide crash dumps. For those the helper
//! decides every path itself; this side only measures, and often cannot even
//! do that, since a standard user may not list `C:\Windows\Temp` or the
//! Delivery Optimization cache. Such an item is still offered, with its size
//! shown once it has been cleaned.

use super::*;
use std::fs;
use std::time::{Duration, SystemTime};

/// Temp entries this recently touched are left: something may still be
/// using them.
const MIN_TEMP_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// Suggested at this size or more.
const SUGGEST_BYTES: u64 = 200 * MIB;
/// A temp folder this deep is too big to date; it is left as if fresh.
const MAX_AGE_WALK: usize = 20_000;
const ADMIN_NOTE: &str = "a Windows administrator prompt will appear";

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute())
}

fn system_root() -> PathBuf {
    env_dir("SystemRoot").or_else(|| env_dir("windir")).unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

fn local_app_data() -> Option<PathBuf> {
    env_dir("LOCALAPPDATA").or_else(|| super::super::home_dir().map(|h| h.join(r"AppData\Local")))
}

fn thumbnail_dir() -> Option<PathBuf> {
    local_app_data().map(|l| l.join(r"Microsoft\Windows\Explorer"))
}

/// Folders whose every entry is a Windows Error Reporting report.
fn user_report_dirs() -> Vec<PathBuf> {
    let Some(local) = local_app_data() else { return Vec::new() };
    vec![
        local.join(r"Microsoft\Windows\WER\ReportArchive"),
        local.join(r"Microsoft\Windows\WER\ReportQueue"),
    ]
}

/// The newest modification time anywhere under `path`, not following links.
/// `None` when it cannot be read in full, which counts as "in use".
fn newest_mtime(path: &Path) -> Option<SystemTime> {
    let mut newest = fs::symlink_metadata(path).ok()?.modified().ok()?;
    let mut stack = vec![path.to_path_buf()];
    let mut seen = 0usize;
    while let Some(dir) = stack.pop() {
        if !fs::symlink_metadata(&dir).ok()?.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&dir).ok()?.flatten() {
            seen += 1;
            if seen > MAX_AGE_WALK {
                return None;
            }
            let meta = fs::symlink_metadata(entry.path()).ok()?;
            newest = newest.max(meta.modified().ok()?);
            if meta.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Some(newest)
}

/// The entries of `dir` nothing has touched for `min_age`.
fn stale_entries(dir: &Path, min_age: Duration) -> Vec<PathBuf> {
    let Some(cutoff) = SystemTime::now().checked_sub(min_age) else { return Vec::new() };
    let Ok(read) = fs::read_dir(dir) else { return Vec::new() };
    read.flatten()
        .map(|e| e.path())
        .filter(|p| newest_mtime(p).is_some_and(|t| t < cutoff))
        .collect()
}

fn all_entries(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

fn thumbnail_files() -> Vec<PathBuf> {
    let Some(dir) = thumbnail_dir() else { return Vec::new() };
    all_entries(&dir)
        .into_iter()
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            name.starts_with("thumbcache_") && name.ends_with(".db") && fs::symlink_metadata(p).is_ok_and(|m| m.is_file())
        })
        .collect()
}

fn usage_of(paths: &[PathBuf]) -> u64 {
    let scan = Scan::detached();
    paths.iter().map(|p| disk_usage(p, &scan).bytes).sum()
}

/// Removes each entry; a link (or junction) goes without what it points at,
/// and `remove_dir_all` never follows one inside a tree. Returns how many went
/// and how many were left, almost always because a program has them open.
/// One already gone counts as neither.
fn remove_entries(paths: &[PathBuf]) -> (usize, usize) {
    let (mut removed, mut left) = (0, 0);
    for path in paths {
        let Ok(meta) = fs::symlink_metadata(path) else { continue };
        let result = if meta.file_type().is_symlink() {
            fs::remove_file(path).or_else(|_| fs::remove_dir(path))
        } else if meta.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };
        if result.is_ok() {
            removed += 1;
        } else {
            left += 1;
        }
    }
    (removed, left)
}

/// "Removed 12 items", plus what was left. All left and none removed is a
/// failure: nothing happened.
fn removal_message((removed, left): (usize, usize), left_because: &str) -> Result<String, String> {
    let plural = |n: usize| if n == 1 { "item" } else { "items" };
    match (removed, left) {
        (0, 0) => Ok("Nothing to remove".to_string()),
        (0, l) => Err(format!("Removed nothing: {l} {} {left_because}", plural(l))),
        (r, 0) => Ok(format!("Removed {r} {}", plural(r))),
        (r, l) => Ok(format!("Removed {r} {}; {l} {} {left_because}", plural(r), plural(l))),
    }
}

fn user_finding(item: &SystemItem, id: &str, reason: &str, method: &str, paths: Vec<PathBuf>, bytes: u64, risk: Risk) -> Finding {
    Finding {
        id: id.to_string(),
        tab: TAB.to_string(),
        group: GROUP.to_string(),
        label: label_of(item),
        reason: reason.to_string(),
        method: method.to_string(),
        paths: paths.iter().map(|p| p.display().to_string()).collect(),
        bytes,
        risk,
        suggested: bytes >= SUGGEST_BYTES,
        blocked_by: None,
        needs_confirm_text: None,
    }
}

/// `bytes` is `None` when this user may not look inside; the item is still
/// offered, since the helper can.
fn admin_finding(item: &SystemItem, id: &str, reason: &str, method: &str, paths: Vec<PathBuf>, bytes: Option<u64>) -> Finding {
    let reason = match bytes {
        Some(_) => reason.to_string(),
        None => format!("{reason} Only an administrator can see inside, so its size shows after cleaning."),
    };
    let mut f = user_finding(item, id, &reason, &format!("{method}; {ADMIN_NOTE}"), paths, bytes.unwrap_or(0), Risk::Review);
    f.suggested = bytes.is_some_and(|b| b >= SUGGEST_BYTES);
    f
}

fn windows_temp() -> PathBuf {
    system_root().join("Temp")
}

fn update_downloads() -> PathBuf {
    system_root().join(r"SoftwareDistribution\Download")
}

fn delivery_cache() -> PathBuf {
    system_root().join(r"ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache")
}

/// The machine-wide crash dump files and report folders.
fn system_crash_paths() -> Vec<PathBuf> {
    let root = system_root();
    let mut paths = vec![root.join("MEMORY.DMP"), root.join("Minidump")];
    if let Some(data) = env_dir("ProgramData") {
        paths.push(data.join(r"Microsoft\Windows\WER\ReportArchive"));
        paths.push(data.join(r"Microsoft\Windows\WER\ReportQueue"));
    }
    paths
}

/// Bytes under `dir`, or `None` when this user may not list it.
fn readable_usage(dir: &Path, entries: impl Fn(&Path) -> Vec<PathBuf>) -> Option<u64> {
    fs::read_dir(dir).ok()?;
    Some(usage_of(&entries(dir)))
}

/// Delivery Optimization's own count of its cache, for when the folder is
/// closed to this user. Slow (PowerShell loads a module), so the scan runs it
/// on a thread while it measures everything else.
fn delivery_cache_bytes_reported() -> Option<u64> {
    let out = crate::modules::tools::command("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", "(Get-DeliveryOptimizationPerfSnap).CacheSizeBytes"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn delivery_cache_bytes() -> Option<u64> {
    readable_usage(&delivery_cache(), all_entries).or_else(delivery_cache_bytes_reported)
}

fn system_crash_bytes() -> Option<u64> {
    let mut total = 0;
    let mut any = false;
    for p in system_crash_paths() {
        let Ok(meta) = fs::symlink_metadata(&p) else { continue };
        if meta.is_dir() {
            if let Some(b) = readable_usage(&p, all_entries) {
                total += b;
                any = true;
            }
        } else {
            total += disk_usage(&p, &Scan::detached()).bytes;
            any = true;
        }
    }
    any.then_some(total)
}

pub fn scan_findings(scan: &Scan) -> Vec<Finding> {
    let delivery = std::thread::spawn(delivery_cache_bytes);
    let mut found = Vec::new();

    let thumbs = thumbnail_files();
    let bytes = usage_of(&thumbs);
    if bytes > 0 {
        found.push(user_finding(
            &SystemItem::Thumbnails,
            ID_THUMBNAILS,
            "Explorer's picture and video previews; it redraws them as you browse. Files Explorer has open are skipped.",
            "delete",
            thumbnail_dir().into_iter().collect(),
            bytes,
            Risk::Safe,
        ));
    }
    let report_dirs = user_report_dirs();
    let bytes: u64 = report_dirs.iter().map(|d| usage_of(&all_entries(d))).sum();
    if bytes > 0 {
        found.push(user_finding(
            &SystemItem::ErrorReports,
            ID_ERROR_REPORTS,
            "Reports Windows Error Reporting kept about apps that crashed or hung; only useful for reporting that problem.",
            "delete",
            report_dirs,
            bytes,
            Risk::Review,
        ));
    }
    if scan.is_cancelled() {
        return found;
    }

    let temp = windows_temp();
    if temp.is_dir() {
        let bytes = readable_usage(&temp, |d| stale_entries(d, MIN_TEMP_AGE));
        if bytes != Some(0) {
            found.push(admin_finding(
                &SystemItem::WindowsTemp,
                ID_WINDOWS_TEMP,
                "Temporary files installers and services left behind, untouched for a day.",
                "delete",
                vec![temp],
                bytes,
            ));
        }
    }
    let downloads = update_downloads();
    if downloads.is_dir() {
        let bytes = readable_usage(&downloads, all_entries);
        if bytes != Some(0) {
            found.push(admin_finding(
                &SystemItem::WindowsUpdate,
                ID_WINDOWS_UPDATE,
                "Update files Windows has downloaded; an update not yet installed downloads again.",
                "stop Windows Update, delete, start it again",
                vec![downloads],
                bytes,
            ));
        }
    }
    if let Some(bytes) = delivery.join().ok().flatten().filter(|b| *b > 0) {
        found.push(admin_finding(
            &SystemItem::DeliveryOptimization,
            ID_DELIVERY_OPTIMIZATION,
            "Update pieces Windows keeps to share with other PCs; nothing is lost.",
            "Delete-DeliveryOptimizationCache",
            vec![delivery_cache()],
            Some(bytes),
        ));
    }
    let crash = system_crash_bytes();
    let memory_dump = system_root().join("MEMORY.DMP").is_file();
    if crash.is_some_and(|b| b > 0) || (crash.is_none() && memory_dump) {
        found.push(admin_finding(
            &SystemItem::SystemCrashDumps,
            ID_SYSTEM_CRASH_DUMPS,
            "Memory dumps from blue screens and machine-wide error reports; only useful for diagnosing that crash.",
            "delete",
            system_crash_paths().into_iter().filter(|p| p.exists()).collect(),
            crash,
        ));
    }
    found
}

/// Size of what an item occupies now: for an in-process item, before and
/// after cleaning; for an elevated one, its share of the freed space.
pub fn item_bytes(item: &SystemItem) -> u64 {
    match item {
        SystemItem::Thumbnails => usage_of(&thumbnail_files()),
        SystemItem::ErrorReports => user_report_dirs().iter().map(|d| usage_of(&all_entries(d))).sum(),
        SystemItem::WindowsTemp => readable_usage(&windows_temp(), |d| stale_entries(d, MIN_TEMP_AGE)).unwrap_or(0),
        SystemItem::WindowsUpdate => readable_usage(&update_downloads(), all_entries).unwrap_or(0),
        SystemItem::DeliveryOptimization => readable_usage(&delivery_cache(), all_entries).unwrap_or(0),
        SystemItem::SystemCrashDumps => system_crash_bytes().unwrap_or(0),
        SystemItem::Apt | SystemItem::Journal | SystemItem::Snap { .. } => 0,
    }
}

/// Cleans one of the items the user owns.
pub fn clean_local(item: &SystemItem) -> Result<String, String> {
    match item {
        SystemItem::Thumbnails => removal_message(
            remove_entries(&thumbnail_files()),
            "open in Explorer were left; sign out and back in, then clean again",
        ),
        SystemItem::ErrorReports => {
            let entries: Vec<PathBuf> = user_report_dirs().iter().flat_map(|d| all_entries(d)).collect();
            removal_message(remove_entries(&entries), "in use were left")
        }
        other => Err(format!("{} needs the administrator helper", label_of(other))),
    }
}

/// The system drive, where everything the helper cleans lives.
pub fn free_probe() -> PathBuf {
    system_root().ancestors().last().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(r"C:\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("devspro-system-win-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn age(path: &Path, days: u64) {
        let when = SystemTime::now() - Duration::from_secs(days * 86_400);
        let f = fs::File::options().write(true).open(path).unwrap();
        f.set_modified(when).unwrap();
    }

    #[test]
    fn only_entries_untouched_for_a_day_are_stale() {
        let d = scratch("stale");
        fs::write(d.join("old.tmp"), "x").unwrap();
        age(&d.join("old.tmp"), 3);
        fs::write(d.join("new.tmp"), "x").unwrap();
        // A folder counts as fresh when anything deep inside it is.
        fs::create_dir_all(d.join("busy/deep")).unwrap();
        fs::write(d.join("busy/deep/live.log"), "x").unwrap();
        let mut stale = stale_entries(&d, MIN_TEMP_AGE);
        stale.sort();
        assert_eq!(stale, vec![d.join("old.tmp")]);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn removing_entries_reports_what_was_left() {
        let d = scratch("remove");
        fs::write(d.join("a"), "x").unwrap();
        fs::create_dir_all(d.join("b/c")).unwrap();
        // Held open without delete sharing, as a running program would.
        let held = d.join("held");
        fs::write(&held, "x").unwrap();
        let lock = {
            use std::os::windows::fs::OpenOptionsExt;
            fs::File::options().read(true).share_mode(0).open(&held).unwrap()
        };
        let counts = remove_entries(&[d.join("a"), d.join("b"), held.clone(), d.join("missing")]);
        assert_eq!(counts, (2, 1));
        assert!(!d.join("a").exists() && !d.join("b").exists() && held.exists());
        drop(lock);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn removing_a_junction_leaves_its_target() {
        let d = scratch("junction");
        let target = scratch("junction-target");
        fs::write(target.join("keep"), "x").unwrap();
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(d.join("link"))
            .arg(&target)
            .output()
            .unwrap();
        assert!(ok.status.success());
        assert_eq!(remove_entries(&[d.join("link")]), (1, 0));
        assert!(target.join("keep").exists());
        fs::remove_dir_all(&d).unwrap();
        fs::remove_dir_all(&target).unwrap();
    }

    #[test]
    fn removal_messages() {
        assert_eq!(removal_message((0, 0), "x"), Ok("Nothing to remove".to_string()));
        assert_eq!(removal_message((1, 0), "x"), Ok("Removed 1 item".to_string()));
        assert_eq!(removal_message((3, 2), "in use were left"), Ok("Removed 3 items; 2 items in use were left".to_string()));
        assert!(removal_message((0, 2), "in use were left").is_err());
    }

    #[test]
    fn admin_findings_without_a_size_say_so_and_are_not_suggested() {
        let f = admin_finding(&SystemItem::WindowsTemp, ID_WINDOWS_TEMP, "Old files.", "delete", vec![], None);
        assert!(f.reason.contains("administrator") && f.method.contains("administrator prompt"));
        assert!(!f.suggested && f.bytes == 0);
        let big = admin_finding(&SystemItem::WindowsUpdate, ID_WINDOWS_UPDATE, "x", "y", vec![], Some(SUGGEST_BYTES));
        assert!(big.suggested && big.risk == Risk::Review);
        let thumbs = user_finding(&SystemItem::Thumbnails, ID_THUMBNAILS, "x", "y", vec![], 10, Risk::Safe);
        assert!(!thumbs.suggested && !thumbs.method.contains("prompt"));
    }

    #[test]
    fn caches_tab_items_are_not_repeated_here() {
        // The Recycle Bin, %TEMP% and CrashDumps belong to the Caches tab.
        let found = scan_findings(&Scan::detached());
        assert!(found.iter().all(|f| !f.label.contains("Recycle") && !f.label.starts_with("Temporary files")));
        assert!(user_report_dirs().iter().all(|d| !d.ends_with("CrashDumps")));
    }

    #[test]
    fn free_space_is_read_on_the_system_drive() {
        let probe = free_probe();
        assert!(probe.parent().is_none() && probe.is_absolute(), "{probe:?}");
        assert!(free_bytes(&probe).unwrap() > 0);
    }
}
