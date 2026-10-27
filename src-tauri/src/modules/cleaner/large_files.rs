//! Large files tab: big files under the scan roots, and a folder-by-folder size
//! view. Places the other tabs already cover (caches, package stores, project
//! artifacts, the trash) are left out of the file scan.

use super::guard::{check_removable, remove_path, within};
use super::measure::{disk_usage, free_bytes, walk_dirs};
use super::{history, CleanOutcome, CleanerSettings, CleanerState, Scan};
use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use crate::modules::fs_compat::MetadataExt;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

const TAB: &str = "large-files";
const MAX_RESULTS: usize = 2000;

/// Under $HOME: owned by another tab, so not offered here.
#[cfg(not(windows))]
const HOME_SKIPPED: &[&str] = &[
    ".cache",
    ".npm",
    ".local/share/pnpm",
    ".pnpm-store",
    ".cargo",
    ".rustup",
    ".gradle",
    ".m2",
    ".local/share/Trash",
];

/// Under %USERPROFILE%: owned by another tab (Caches, System, Docker), so not
/// offered here. Docker Desktop's disk image lives under `AppData/Local/Docker`
/// and holds every image and volume; deleting it as a "large file" would lose
/// them all, which the Docker tab exists to do one item at a time.
#[cfg(windows)]
const HOME_SKIPPED: &[&str] = &[
    ".cache",
    ".npm",
    ".cargo",
    ".rustup",
    ".gradle",
    ".m2",
    ".nuget",
    "AppData/Local/Temp",
    "AppData/Local/npm-cache",
    "AppData/Local/pnpm",
    "AppData/Local/pnpm-cache",
    "AppData/Local/Yarn",
    "AppData/Local/pip",
    "AppData/Local/NuGet",
    "AppData/Local/Composer",
    "AppData/Local/go-build",
    "AppData/Local/Docker",
    "AppData/Local/CrashDumps",
    "AppData/Local/Microsoft/Windows/Explorer",
    "AppData/Local/Microsoft/Windows/INetCache",
    "AppData/Local/Microsoft/Windows/WER",
];

const VIDEO: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "wmv", "flv", "m4v", "mpg", "mpeg"];
const DISK_IMAGE: &[&str] = &["iso", "img", "qcow2", "vdi", "vmdk", "vhd"];
const ARCHIVE: &[&str] = &["zip", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "zst", "7z", "rar"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeFile {
    pub path: String,
    pub bytes: u64,
    /// Unix seconds.
    pub modified: i64,
    /// Unix seconds.
    pub accessed: i64,
    /// video, disk-image, archive, db-dump, log, core-dump or other.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderEntry {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub is_dir: bool,
    /// Entries inside, for a directory.
    pub entries: u64,
}

fn skipped_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    home.map(|h| HOME_SKIPPED.iter().map(|p| h.join(p)).collect()).unwrap_or_default()
}

/// Windows: what the guard would refuse to remove anyway — the Windows and
/// Program Files folders, the Recycle Bin, `pagefile.sys` and the like when a
/// whole drive is a scan root — is not worth listing. Linux lists everything
/// and lets the guard refuse, as it always has.
#[cfg(windows)]
fn os_owned(path: &Path) -> bool {
    super::guard::is_protected(path)
}

#[cfg(not(windows))]
fn os_owned(_path: &Path) -> bool {
    false
}

/// A project's own build output: the project-junk tab owns these.
fn is_project_artifact(dir: &Path, name: &str) -> bool {
    let beside = |file: &str| dir.parent().is_some_and(|p| p.join(file).is_file());
    match name {
        "node_modules" | ".next" => true,
        "target" => beside("Cargo.toml"),
        "vendor" => beside("composer.json"),
        _ => false,
    }
}

fn kind_of(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    if lower.ends_with(".sql.gz") || matches!(ext, "sql" | "dump" | "bak") {
        "db-dump"
    } else if ext == "log" || lower.contains(".log.") {
        "log"
    } else if DISK_IMAGE.contains(&ext) {
        "disk-image"
    } else if VIDEO.contains(&ext) {
        "video"
    } else if ARCHIVE.contains(&ext) {
        "archive"
    } else if lower == "core" || lower.starts_with("core.") {
        "core-dump"
    } else {
        "other"
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Every file under `roots` with at least `min_bytes` on disk (and, with
/// `older_than_days`, not modified since), largest first, at most 2000. Never
/// follows a symlink or leaves a root's filesystem. A file hard-linked in more
/// than one place is listed once, under the path that sorts first.
fn find_large_files(
    roots: &[PathBuf],
    exclusions: &[PathBuf],
    skipped: &[PathBuf],
    min_bytes: u64,
    older_than_days: Option<u32>,
    scan: &Scan,
) -> Vec<LargeFile> {
    let cutoff = older_than_days.map(|d| now_secs() - i64::from(d) * 86_400);
    // A root inside another root is walked by that one.
    let mut walked: Vec<&PathBuf> = roots.iter().filter(|r| !roots.iter().any(|o| o != *r && r.starts_with(o))).collect();
    walked.sort();
    walked.dedup();
    // Each file with its inode when it has other hard links.
    let mut found: Vec<(LargeFile, Option<(u64, u64)>)> = Vec::new();

    for root in walked {
        let Ok(root_meta) = fs::symlink_metadata(root) else { continue };
        if !root_meta.is_dir() {
            continue;
        }
        let dev = root_meta.dev();
        let (per_thread, _) = walk_dirs(root.clone(), scan, Vec::new, |found: &mut Vec<_>, dir, list| {
            let mut subdirs = Vec::new();
            let (mut files, mut bytes) = (0, 0);
            for entry in list {
                // From the directory listing itself, so symlinks and other
                // non-files cost no stat.
                let Ok(ft) = entry.file_type() else { continue };
                if ft.is_dir() {
                    let path = entry.path();
                    let Ok(meta) = entry.metadata() else { continue };
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let covered = meta.dev() != dev
                        || exclusions.iter().any(|e| within(&path, e))
                        || skipped.iter().any(|s| within(&path, s))
                        || is_project_artifact(&path, &name)
                        || os_owned(&path);
                    if !covered {
                        subdirs.push(path);
                    }
                    continue;
                }
                if !ft.is_file() {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                let size = meta.blocks() * 512;
                files += 1;
                bytes += size;
                if size < min_bytes || cutoff.is_some_and(|c| meta.mtime() >= c) {
                    continue;
                }
                let path = entry.path();
                if exclusions.iter().any(|e| within(&path, e)) || os_owned(&path) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let file = LargeFile {
                    path: path.to_string_lossy().into_owned(),
                    bytes: size,
                    modified: meta.mtime(),
                    accessed: meta.atime(),
                    kind: kind_of(&name).to_string(),
                };
                found.push((file, (meta.nlink() > 1).then(|| (meta.dev(), meta.ino()))));
            }
            scan.tick(files, bytes, dir);
            subdirs
        });
        found.extend(per_thread.into_iter().flatten());
    }

    // Threads finish in any order: sort by path so the same link always wins.
    found.sort_by(|a, b| a.0.path.cmp(&b.0.path));
    let mut seen_links: HashSet<(u64, u64)> = HashSet::new();
    let mut found: Vec<LargeFile> = found
        .into_iter()
        .filter(|(_, link)| link.is_none_or(|l| seen_links.insert(l)))
        .map(|(file, _)| file)
        .collect();
    found.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    found.truncate(MAX_RESULTS);
    found
}

/// The direct children of `path`, each with its disk usage, largest first.
/// `path` must be a scan root or inside one.
fn folder_entries(path: &Path, roots: &[PathBuf], scan: &Scan) -> Result<Vec<FolderEntry>, String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("{} is not an absolute path without ..", path.display()));
    }
    let root = roots
        .iter()
        .filter(|r| path.starts_with(r))
        .max_by_key(|r| r.components().count())
        .ok_or_else(|| format!("{} is not inside a scan root", path.display()))?;
    // The root is the user's choice; everything below it must be a real folder.
    let mut cursor = root.clone();
    for component in path.strip_prefix(root).unwrap_or(path).components() {
        cursor.push(component);
        let meta = fs::symlink_metadata(&cursor).map_err(|e| format!("{}: {e}", cursor.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!("{} is a symbolic link", cursor.display()));
        }
    }
    let real = fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let real_root = fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    if !real.starts_with(&real_root) {
        return Err(format!("{} is outside the scan root", path.display()));
    }
    let read = fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for entry in read.flatten() {
        if scan.is_cancelled() {
            break;
        }
        let child = entry.path();
        let Ok(meta) = fs::symlink_metadata(&child) else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        let usage = disk_usage(&child, scan);
        out.push(FolderEntry {
            path: child.to_string_lossy().into_owned(),
            name: entry.file_name().to_string_lossy().into_owned(),
            bytes: usage.bytes,
            is_dir: meta.is_dir(),
            // The directory itself is counted once in `entries`.
            entries: if meta.is_dir() { usage.entries.saturating_sub(1) } else { 0 },
        });
    }
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// Runs each path through the guard, then `remove`. A refused path is a failed
/// outcome, not an error for the rest.
fn remove_checked(
    paths: &[String],
    permanent: bool,
    settings: &CleanerSettings,
    remove: impl Fn(&Path, bool) -> Result<(), String>,
) -> Vec<CleanOutcome> {
    paths
        .iter()
        .map(|p| {
            let path = Path::new(p);
            let failed = |message: String| CleanOutcome { id: p.clone(), ok: false, message, bytes_freed: 0 };
            if let Err(e) = check_removable(path, &settings.scan_roots, &settings.exclusions) {
                return failed(e);
            }
            let probe = path.parent().unwrap_or(path);
            let before = free_bytes(probe).unwrap_or(0);
            match remove(path, permanent) {
                Ok(()) => CleanOutcome {
                    id: p.clone(),
                    ok: true,
                    message: if permanent { "Deleted".into() } else { "Moved to the Trash".into() },
                    bytes_freed: free_bytes(probe).unwrap_or(before).saturating_sub(before),
                },
                Err(e) => failed(e),
            }
        })
        .collect()
}

fn load_settings(db: &Db) -> Result<CleanerSettings, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(history::load_settings(&conn))
}

#[tauri::command(async)]
pub fn cleaner_scan_large_files(
    app: AppHandle,
    state: State<CleanerState>,
    db: State<Db>,
    scan_id: String,
    min_bytes: u64,
    older_than_days: Option<u32>,
) -> Result<Vec<LargeFile>, String> {
    let settings = load_settings(&db)?;
    let scan = Scan::start(&state, &app, &scan_id, TAB);
    let roots: Vec<PathBuf> = settings.scan_roots.iter().map(PathBuf::from).collect();
    let exclusions: Vec<PathBuf> = settings.exclusions.iter().map(PathBuf::from).collect();
    let home = super::home_dir();
    Ok(find_large_files(&roots, &exclusions, &skipped_dirs(home.as_deref()), min_bytes, older_than_days, &scan))
}

#[tauri::command(async)]
pub fn cleaner_folder_sizes(
    app: AppHandle,
    state: State<CleanerState>,
    db: State<Db>,
    scan_id: String,
    path: String,
) -> Result<Vec<FolderEntry>, String> {
    let settings = load_settings(&db)?;
    let roots: Vec<PathBuf> = settings.scan_roots.iter().map(PathBuf::from).collect();
    let scan = Scan::start(&state, &app, &scan_id, TAB);
    folder_entries(Path::new(&path), &roots, &scan)
}

#[tauri::command(async)]
pub fn cleaner_remove_files(
    db: State<Db>,
    paths: Vec<String>,
    permanent: bool,
) -> Result<Vec<CleanOutcome>, String> {
    let settings = load_settings(&db)?;
    let outcomes = remove_checked(&paths, permanent, &settings, remove_path);
    let freed: u64 = outcomes.iter().filter(|o| o.ok).map(|o| o.bytes_freed).sum();
    let done = outcomes.iter().filter(|o| o.ok).count();
    let all_ok = done == outcomes.len();
    let label = format!(
        "{} {done} large file{}",
        if permanent { "Deleted" } else { "Moved to the Trash:" },
        if done == 1 { "" } else { "s" }
    );
    let message = outcomes.iter().find(|o| !o.ok).map(|o| o.message.as_str()).unwrap_or("");
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    history::record(&conn, TAB, &label, freed, all_ok, message)?;
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("devspro-large-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn file(path: &Path, kib: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::File::create(path).unwrap().write_all(&vec![7u8; kib * 1024]).unwrap();
    }

    fn names(found: &[LargeFile], root: &Path) -> Vec<String> {
        // Windows separators read as `/`, so one expectation fits both OSes.
        found
            .iter()
            .map(|f| f.path.strip_prefix(root.to_str().unwrap()).unwrap().replace('\\', "/").trim_start_matches('/').to_string())
            .collect()
    }

    fn find(root: &Path, skipped: &[PathBuf], min: u64, days: Option<u32>) -> Vec<LargeFile> {
        find_large_files(&[root.to_path_buf()], &[], skipped, min, days, &Scan::detached())
    }

    #[test]
    fn threshold_keeps_big_files_largest_first() {
        let root = scratch("threshold");
        file(&root.join("small.bin"), 8);
        file(&root.join("mid.bin"), 128);
        file(&root.join("sub/big.bin"), 512);
        let found = find(&root, &[], 64 * 1024, None);
        assert_eq!(names(&found, &root), vec!["sub/big.bin", "mid.bin"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn age_filter_keeps_only_old_files() {
        let root = scratch("age");
        file(&root.join("old.bin"), 128);
        file(&root.join("new.bin"), 128);
        let old = fs::File::options().write(true).open(root.join("old.bin")).unwrap();
        old.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(90 * 86_400)).unwrap();
        let found = find(&root, &[], 64 * 1024, Some(30));
        assert_eq!(names(&found, &root), vec!["old.bin"]);
        assert_eq!(find(&root, &[], 64 * 1024, None).len(), 2);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn classifies_by_extension() {
        for (name, kind) in [
            ("a.MP4", "video"),
            ("a.qcow2", "disk-image"),
            ("a.iso", "disk-image"),
            ("a.tar.gz", "archive"),
            ("a.zip", "archive"),
            ("a.sql", "db-dump"),
            ("a.sql.gz", "db-dump"),
            ("a.dump", "db-dump"),
            ("a.bak", "db-dump"),
            ("a.log", "log"),
            ("core", "core-dump"),
            ("core.1234", "core-dump"),
            ("notes.txt", "other"),
            ("score", "other"),
        ] {
            assert_eq!(kind_of(name), kind, "{name}");
        }
    }

    /// A directory link: a symlink on Unix, a junction on Windows (which needs
    /// no admin rights, unlike a Windows symlink).
    fn link_dir(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap()
            .status
            .success());
    }

    #[test]
    fn symlinks_are_not_followed() {
        let root = scratch("links");
        let outside = scratch("links-outside");
        file(&outside.join("huge.bin"), 256);
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.join("huge.bin"), root.join("file-link")).unwrap();
        link_dir(&outside, &root.join("dir-link"));
        assert!(find(&root, &[], 64 * 1024, None).is_empty());
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn skips_other_tabs_folders_and_exclusions() {
        let root = scratch("skips");
        file(&root.join(".cache/a.bin"), 128);
        let other_tab = if cfg!(windows) { "AppData/Local/Temp/b.bin" } else { ".local/share/Trash/files/b.bin" };
        file(&root.join(other_tab), 128);
        file(&root.join("app/node_modules/c.bin"), 128);
        file(&root.join("app/.next/d.bin"), 128);
        file(&root.join("rust/Cargo.toml"), 1);
        file(&root.join("rust/target/e.bin"), 128);
        file(&root.join("php/composer.json"), 1);
        file(&root.join("php/vendor/f.bin"), 128);
        // Not artifacts without the manifest beside them.
        file(&root.join("plain/target/g.bin"), 128);
        file(&root.join("plain/vendor/h.bin"), 128);
        file(&root.join("keep/i.bin"), 128);
        file(&root.join("excluded/j.bin"), 128);
        let skipped = skipped_dirs(Some(&root));
        let found = find_large_files(
            &[root.clone()],
            &[root.join("excluded")],
            &skipped,
            64 * 1024,
            None,
            &Scan::detached(),
        );
        let mut got = names(&found, &root);
        got.sort();
        assert_eq!(got, vec!["keep/i.bin", "plain/target/g.bin", "plain/vendor/h.bin"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn deep_trees_are_walked_whole() {
        let root = scratch("deep");
        let mut expected = Vec::new();
        for a in 0..6 {
            for b in 0..6 {
                let rel = format!("d{a}/e{b}/f{a}{b}.bin");
                file(&root.join(&rel), 128);
                expected.push(rel);
            }
        }
        file(&root.join("d0/e0/small.bin"), 4);
        let mut got = names(&find(&root, &[], 64 * 1024, None), &root);
        got.sort();
        expected.sort();
        assert_eq!(got, expected);
        fs::remove_dir_all(&root).unwrap();
    }

    // Windows: `fs_compat` reports every file with one link, so a hard link
    // is listed under both paths there (see its module comment).
    #[cfg(unix)]
    #[test]
    fn a_hard_linked_file_is_listed_once_under_its_first_path() {
        let root = scratch("hardlinks");
        file(&root.join("z/x/big.bin"), 128);
        fs::create_dir_all(root.join("a/y")).unwrap();
        fs::hard_link(root.join("z/x/big.bin"), root.join("a/y/same.bin")).unwrap();
        for _ in 0..5 {
            assert_eq!(names(&find(&root, &[], 64 * 1024, None), &root), vec!["a/y/same.bin"]);
        }
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_root_given_twice_is_walked_once() {
        let root = scratch("twice");
        file(&root.join("sub/a.bin"), 128);
        let found = find_large_files(&[root.clone(), root.clone()], &[], &[], 64 * 1024, None, &Scan::detached());
        assert_eq!(names(&found, &root), vec!["sub/a.bin"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn results_are_capped() {
        let root = scratch("cap");
        for i in 0..(MAX_RESULTS + 5) {
            file(&root.join(format!("f{i}.bin")), 4);
        }
        assert_eq!(find(&root, &[], 1, None).len(), MAX_RESULTS);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cancelled_scan_returns_early() {
        let root = scratch("cancel");
        file(&root.join("a.bin"), 128);
        let scan = Scan::detached();
        scan.cancel();
        assert!(find_large_files(&[root.clone()], &[], &[], 1, None, &scan).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn folder_sizes_lists_children_largest_first_and_stays_in_roots() {
        let root = scratch("folders");
        file(&root.join("a/one.bin"), 64);
        file(&root.join("a/two.bin"), 64);
        file(&root.join("b.bin"), 16);
        let roots = vec![root.clone()];
        let rows = folder_entries(&root, &roots, &Scan::detached()).unwrap();
        assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["a", "b.bin"]);
        assert!(rows[0].is_dir && rows[0].entries == 2 && !rows[1].is_dir);
        assert!(rows[0].bytes > rows[1].bytes);
        assert!(folder_entries(&root.join("a"), &roots, &Scan::detached()).is_ok());
        assert!(folder_entries(Path::new("/etc"), &roots, &Scan::detached()).is_err());
        assert!(folder_entries(&root.join("a/../.."), &roots, &Scan::detached()).is_err());
        // A link inside the root that points out of it is not listed.
        let outside = scratch("folders-outside");
        file(&outside.join("secret.bin"), 4);
        link_dir(&outside, &root.join("escape"));
        let err = folder_entries(&root.join("escape"), &roots, &Scan::detached()).unwrap_err();
        assert!(err.contains("symbolic link"), "{err}");
        let top = folder_entries(&root, &roots, &Scan::detached()).unwrap();
        assert!(top.iter().all(|r| r.name != "escape"));
        fs::remove_dir_all(&outside).unwrap();
        fs::remove_dir_all(&root).unwrap();
    }

    fn settings_for(root: &Path) -> CleanerSettings {
        CleanerSettings { scan_roots: vec![root.to_string_lossy().into_owned()], ..CleanerSettings::default() }
    }

    #[test]
    fn a_path_outside_the_roots_is_rejected_and_not_removed() {
        let root = scratch("reject");
        let outside = scratch("reject-outside");
        file(&outside.join("x.bin"), 4);
        file(&root.join("in.bin"), 4);
        let calls = Mutex::new(Vec::new());
        let paths = vec![outside.join("x.bin").to_string_lossy().into_owned(), root.join("in.bin").to_string_lossy().into_owned()];
        let out = remove_checked(&paths, false, &settings_for(&root), |p, perm| {
            calls.lock().unwrap().push((p.to_path_buf(), perm));
            Ok(())
        });
        assert!(!out[0].ok && out[0].message.contains("not inside"));
        assert!(out[1].ok);
        assert_eq!(*calls.lock().unwrap(), vec![(root.join("in.bin"), false)]);
        assert!(outside.join("x.bin").exists());
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn excluded_paths_are_rejected() {
        let root = scratch("excl");
        file(&root.join("keep/x.bin"), 4);
        let mut settings = settings_for(&root);
        settings.exclusions = vec![root.join("keep").to_string_lossy().into_owned()];
        let out = remove_checked(&[root.join("keep/x.bin").to_string_lossy().into_owned()], true, &settings, remove_path);
        assert!(!out[0].ok);
        assert!(root.join("keep/x.bin").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn trash_goes_through_the_trash_and_permanent_deletes() {
        let root = scratch("modes");
        file(&root.join("a.bin"), 4);
        file(&root.join("b.bin"), 4);
        let a = root.join("a.bin").to_string_lossy().into_owned();
        let b = root.join("b.bin").to_string_lossy().into_owned();
        let settings = settings_for(&root);

        let modes = Mutex::new(Vec::new());
        let out = remove_checked(&[a.clone()], false, &settings, |_, perm| {
            modes.lock().unwrap().push(perm);
            Ok(())
        });
        assert!(out[0].ok);
        assert_eq!(*modes.lock().unwrap(), vec![false]);
        assert!(root.join("a.bin").exists(), "the stub trash does not move anything");

        let out = remove_checked(&[b], true, &settings, remove_path);
        assert!(out[0].ok, "{}", out[0].message);
        assert!(!root.join("b.bin").exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
