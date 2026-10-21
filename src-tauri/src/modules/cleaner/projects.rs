//! Project junk tab: node_modules, target, vendor and friends in project
//! folders. An artifact counts only when its marker file sits next to it, and
//! the clean command checks that again, so a stale or forged path cannot delete
//! an arbitrary folder that happens to share a name.

use super::measure::{disk_usage, free_bytes};
use super::{guard, history, CleanOutcome, CleanerSettings, CleanerState, Scan};
use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use crate::modules::fs_compat::MetadataExt;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectJunk {
    pub project_path: String,
    pub project_name: String,
    pub artifact_path: String,
    pub artifact_kind: String,
    pub bytes: u64,
    /// Unix seconds of the project's last change.
    pub last_activity: Option<i64>,
    pub idle_days: Option<u32>,
    pub suggested: bool,
}

/// How far below a root the walk looks for projects.
const MAX_DEPTH: usize = 6;

/// Marker files, and the artifact folders each one vouches for.
const RULES: &[(&[&str], &[&str])] = &[
    (&["package.json"], &["node_modules", ".next", ".nuxt", ".turbo", ".parcel-cache", ".svelte-kit"]),
    (&["composer.json"], &["vendor"]),
    (&["Cargo.toml"], &["target"]),
    (&["pyproject.toml", "requirements.txt", "setup.py"], &[".venv", "venv", "__pycache__"]),
    (&["build.gradle", "build.gradle.kts"], &["build", ".gradle"]),
];

fn is_artifact_name(name: &str) -> bool {
    RULES.iter().any(|(_, names)| names.contains(&name))
}

/// True when `dir` holds a marker that makes `name` an artifact.
fn marker_present(dir: &Path, name: &str) -> bool {
    RULES
        .iter()
        .filter(|(_, names)| names.contains(&name))
        .any(|(markers, _)| markers.iter().any(|m| fs::symlink_metadata(dir.join(m)).is_ok_and(|md| md.is_file())))
}

/// The artifact folders of `dir`: real directories (never links) whose marker is present.
fn artifacts_in(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    for (markers, names) in RULES {
        if !markers.iter().any(|m| fs::symlink_metadata(dir.join(m)).is_ok_and(|md| md.is_file())) {
            continue;
        }
        for name in *names {
            let p = dir.join(name);
            if fs::symlink_metadata(&p).is_ok_and(|md| md.is_dir()) && !found.iter().any(|(n, _)| n == name) {
                found.push((name.to_string(), p));
            }
        }
    }
    found
}

/// Whole days between `last` and `now`, never negative.
fn idle_days(last: i64, now: i64) -> u32 {
    ((now - last).max(0) / 86_400).min(u32::MAX as i64) as u32
}

fn is_suggested(idle: Option<u32>, threshold: u32) -> bool {
    idle.is_some_and(|d| d >= threshold)
}

/// Last commit time, else the newest mtime among the top-level entries that
/// are not artifacts.
fn last_activity(project: &Path) -> Option<i64> {
    let git = super::quiet_command("git")
        .arg("-C")
        .arg(project)
        .args(["log", "-1", "--format=%ct"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<i64>().ok());
    git.or_else(|| newest_mtime(project))
}

fn newest_mtime(project: &Path) -> Option<i64> {
    fs::read_dir(project)
        .ok()?
        .flatten()
        .filter(|e| !is_artifact_name(&e.file_name().to_string_lossy()))
        .filter_map(|e| fs::symlink_metadata(e.path()).ok())
        .map(|m| m.mtime())
        .max()
}

/// Windows marks the folders Linux would name with a dot — `AppData` above
/// all, where apps like VS Code ship their own `package.json` and
/// `node_modules` — hidden or system instead. The walk leaves those alone, as
/// it leaves dot-folders alone everywhere.
#[cfg(windows)]
fn is_hidden(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    meta.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
}

#[cfg(not(windows))]
fn is_hidden(_meta: &fs::Metadata) -> bool {
    false
}

struct Found {
    project: PathBuf,
    artifacts: Vec<(String, PathBuf)>,
}

/// Finds projects with artifacts under `root`, at most `MAX_DEPTH` levels down,
/// on one filesystem, through no symlink, and not into an artifact.
fn find_projects(root: &Path, scan: &Scan) -> Vec<Found> {
    let mut out = Vec::new();
    let Ok(meta) = fs::symlink_metadata(root) else { return out };
    if !meta.is_dir() {
        return out;
    }
    let dev = meta.dev();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if scan.is_cancelled() {
            break;
        }
        scan.tick(1, 0, &dir);
        let artifacts = artifacts_in(&dir);
        if !artifacts.is_empty() {
            out.push(Found { project: dir.clone(), artifacts });
        }
        if depth >= MAX_DEPTH {
            continue;
        }
        let Ok(read) = fs::read_dir(&dir) else { continue };
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || (is_artifact_name(&name) && marker_present(&dir, &name)) {
                continue;
            }
            if let Ok(m) = fs::symlink_metadata(entry.path()) {
                if m.is_dir() && m.dev() == dev && !is_hidden(&m) {
                    stack.push((entry.path(), depth + 1));
                }
            }
        }
    }
    out
}

/// Scans `roots` into one row per artifact. Only a project's top-level
/// `__pycache__` is counted; nested ones are left alone.
fn scan_roots(roots: &[PathBuf], settings: &CleanerSettings, scan: &Scan, now: i64) -> Vec<ProjectJunk> {
    let mut rows = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for root in roots {
        for found in find_projects(root, scan) {
            if scan.is_cancelled() || !seen.insert(found.project.clone()) {
                continue;
            }
            let last = last_activity(&found.project);
            let idle = last.map(|l| idle_days(l, now));
            let name = found
                .project
                .file_name()
                .map_or_else(|| found.project.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
            for (kind, path) in &found.artifacts {
                let usage = disk_usage(path, scan);
                rows.push(ProjectJunk {
                    project_path: found.project.to_string_lossy().into_owned(),
                    project_name: name.clone(),
                    artifact_path: path.to_string_lossy().into_owned(),
                    artifact_kind: kind.clone(),
                    bytes: usage.bytes,
                    last_activity: last,
                    idle_days: idle,
                    suggested: is_suggested(idle, settings.project_idle_days),
                });
            }
        }
    }
    rows.sort_by(|a, b| a.project_path.cmp(&b.project_path).then(b.bytes.cmp(&a.bytes)));
    rows
}

/// The settings roots plus the project folders of the nginx sites DevsPro manages.
fn project_roots(conn: &rusqlite::Connection, settings: &CleanerSettings) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = settings.project_roots.iter().map(PathBuf::from).collect();
    if let Ok(mut stmt) =
        conn.prepare("SELECT project_path FROM nginx_ssl_sites WHERE project_path IS NOT NULL AND project_path != ''")
    {
        if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
            roots.extend(rows.flatten().map(PathBuf::from));
        }
    }
    let mut seen = HashSet::new();
    roots.retain(|r| r.is_absolute() && seen.insert(r.clone()));
    roots
}

/// Ok when `path` is a known artifact folder with its marker beside it,
/// inside one of `roots`, and the guard allows removing it.
fn validate(path: &Path, roots: &[PathBuf], exclusions: &[String]) -> Result<(), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !is_artifact_name(&name) {
        return Err(format!("{} is not a known build or dependency folder", path.display()));
    }
    let parent = path.parent().ok_or("no parent folder")?;
    if !marker_present(parent, &name) {
        return Err(format!("{} has no project file beside it", path.display()));
    }
    if !roots.iter().any(|r| guard::within(path, r) && !guard::same(path, r)) {
        return Err(format!("{} is outside the project folders", path.display()));
    }
    guard::check_removable(path, roots, exclusions)?;
    if !fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        return Err(format!("{} is not a folder", path.display()));
    }
    Ok(())
}

fn recreate(name: &str) -> &'static str {
    match name {
        "node_modules" => "npm install",
        ".next" | ".nuxt" | ".svelte-kit" => "the next build or dev run",
        ".turbo" | ".parcel-cache" => "the next build",
        "vendor" => "composer install",
        "target" => "cargo build",
        ".venv" | "venv" => "python -m venv, then pip install",
        "__pycache__" => "running the code again",
        "build" | ".gradle" => "gradle build",
        _ => "the project's build",
    }
}

fn clean(roots: &[PathBuf], settings: &CleanerSettings, paths: &[String]) -> Vec<CleanOutcome> {
    paths
        .iter()
        .map(|p| {
            let path = Path::new(p);
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let probe = path.parent().unwrap_or(path);
            let before = free_bytes(probe).unwrap_or(0);
            let result = validate(path, roots, &settings.exclusions).and_then(|()| guard::remove_path(path, true));
            let (ok, message, freed) = match result {
                Ok(()) => {
                    let after = free_bytes(probe).unwrap_or(0);
                    (true, format!("Deleted. Recreate it with {}.", recreate(&name)), after.saturating_sub(before))
                }
                Err(e) => (false, e, 0),
            };
            CleanOutcome { id: p.clone(), ok, message, bytes_freed: freed }
        })
        .collect()
}

#[tauri::command(async)]
pub fn cleaner_scan_projects(
    app: AppHandle,
    state: State<CleanerState>,
    db: State<Db>,
    scan_id: String,
) -> Result<Vec<ProjectJunk>, String> {
    let (settings, roots) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let settings = history::load_settings(&conn);
        let roots = project_roots(&conn, &settings);
        (settings, roots)
    };
    let scan = Scan::start(&state, &app, &scan_id, "projects");
    Ok(scan_roots(&roots, &settings, &scan, chrono::Utc::now().timestamp()))
}

#[tauri::command(async)]
pub fn cleaner_clean_projects(db: State<Db>, artifact_paths: Vec<String>) -> Result<Vec<CleanOutcome>, String> {
    // The lock is held only to read the roots and to write history, never
    // while deleting.
    let (settings, roots) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let settings = history::load_settings(&conn);
        let roots = project_roots(&conn, &settings);
        (settings, roots)
    };
    let outcomes = clean(&roots, &settings, &artifact_paths);
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    for o in &outcomes {
        let _ = history::record(&conn, "projects", &o.id, o.bytes_freed, o.ok, &o.message);
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("devspro-projects-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "x").unwrap();
    }

    fn kinds(root: &Path) -> Vec<String> {
        let mut k: Vec<String> = find_projects(root, &Scan::detached())
            .into_iter()
            .flat_map(|f| f.artifacts.into_iter().map(move |(n, _)| format!("{}:{n}", f.project.file_name().unwrap().to_string_lossy())))
            .collect();
        k.sort();
        k
    }

    #[test]
    fn marker_decides_what_counts() {
        let d = scratch("markers");
        touch(&d.join("web/package.json"));
        fs::create_dir_all(d.join("web/node_modules/x")).unwrap();
        fs::create_dir_all(d.join("web/.next")).unwrap();
        fs::create_dir_all(d.join("web/vendor")).unwrap(); // composer marker missing
        fs::create_dir_all(d.join("web/target")).unwrap(); // cargo marker missing
        touch(&d.join("rs/Cargo.toml"));
        fs::create_dir_all(d.join("rs/target")).unwrap();
        touch(&d.join("py/requirements.txt"));
        fs::create_dir_all(d.join("py/.venv")).unwrap();
        fs::create_dir_all(d.join("py/__pycache__")).unwrap();
        touch(&d.join("java/build.gradle.kts"));
        fs::create_dir_all(d.join("java/build")).unwrap();
        fs::create_dir_all(d.join("java/.gradle")).unwrap();
        touch(&d.join("php/composer.json"));
        fs::create_dir_all(d.join("php/vendor")).unwrap();
        fs::create_dir_all(d.join("loose/node_modules")).unwrap();
        assert_eq!(
            kinds(&d),
            [
                "java:.gradle", "java:build", "php:vendor", "py:.venv", "py:__pycache__", "rs:target",
                "web:.next", "web:node_modules"
            ]
        );
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn does_not_descend_into_artifacts_or_hidden_folders() {
        let d = scratch("descent");
        touch(&d.join("app/package.json"));
        touch(&d.join("app/node_modules/dep/package.json"));
        fs::create_dir_all(d.join("app/node_modules/dep/node_modules")).unwrap();
        touch(&d.join(".hidden/p/package.json"));
        fs::create_dir_all(d.join(".hidden/p/node_modules")).unwrap();
        assert_eq!(kinds(&d), ["app:node_modules"]);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn walk_is_depth_limited_and_skips_symlinks() {
        let d = scratch("depth");
        let deep = d.join("a/b/c/d/e/f/g");
        touch(&deep.join("package.json"));
        fs::create_dir_all(deep.join("node_modules")).unwrap();
        let near = d.join("a/b/c/d/e/f");
        touch(&near.join("package.json"));
        fs::create_dir_all(near.join("node_modules")).unwrap();
        let outside = scratch("depth-outside");
        touch(&outside.join("package.json"));
        fs::create_dir_all(outside.join("node_modules")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, d.join("link")).unwrap();
        // A junction, Windows' directory link that needs no admin rights.
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(d.join("link"))
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success());
        assert_eq!(kinds(&d), ["f:node_modules"]);
        fs::remove_dir_all(&d).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn idle_and_suggestion_logic() {
        assert_eq!(idle_days(0, 86_400 * 45 + 5), 45);
        assert_eq!(idle_days(100, 50), 0);
        assert!(is_suggested(Some(30), 30));
        assert!(!is_suggested(Some(29), 30));
        assert!(!is_suggested(None, 30));
    }

    #[test]
    fn falls_back_to_newest_mtime_outside_artifacts() {
        let d = scratch("mtime");
        touch(&d.join("package.json"));
        fs::create_dir_all(d.join("node_modules")).unwrap();
        let m = fs::symlink_metadata(d.join("package.json")).unwrap().mtime();
        assert_eq!(newest_mtime(&d), Some(m));
        // Not a git repository, so the mtime path is what last_activity uses.
        assert_eq!(last_activity(&d), Some(m));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn scan_marks_idle_projects_suggested() {
        let d = scratch("scan");
        touch(&d.join("old/package.json"));
        fs::create_dir_all(d.join("old/node_modules")).unwrap();
        let settings = CleanerSettings { project_idle_days: 30, ..CleanerSettings::default() };
        let now = chrono::Utc::now().timestamp();
        let fresh = scan_roots(&[d.clone()], &settings, &Scan::detached(), now);
        assert_eq!(fresh.len(), 1);
        assert!(!fresh[0].suggested);
        let later = scan_roots(&[d.clone()], &settings, &Scan::detached(), now + 40 * 86_400);
        assert!(later[0].suggested);
        assert_eq!(later[0].idle_days, Some(40));
        assert_eq!(later[0].project_name, "old");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn validate_rejects_missing_marker_wrong_name_and_outside_roots() {
        let d = scratch("validate");
        let other = scratch("validate-other");
        touch(&d.join("web/package.json"));
        fs::create_dir_all(d.join("web/node_modules")).unwrap();
        fs::create_dir_all(d.join("web/target")).unwrap();
        fs::create_dir_all(d.join("web/src")).unwrap();
        touch(&other.join("package.json"));
        fs::create_dir_all(other.join("node_modules")).unwrap();
        let roots = vec![d.clone()];

        assert!(validate(&d.join("web/node_modules"), &roots, &[]).is_ok());
        assert!(validate(&d.join("web/target"), &roots, &[]).is_err());
        assert!(validate(&d.join("web/src"), &roots, &[]).is_err());
        assert!(validate(&other.join("node_modules"), &roots, &[]).is_err());
        assert!(validate(&d.join("web/node_modules"), &roots, &[d.to_string_lossy().into_owned()]).is_err());
        fs::remove_dir_all(&d).unwrap();
        fs::remove_dir_all(&other).unwrap();
    }

    #[test]
    fn clean_deletes_valid_and_refuses_invalid() {
        let d = scratch("clean");
        touch(&d.join("web/package.json"));
        touch(&d.join("web/node_modules/x/y.js"));
        fs::create_dir_all(d.join("web/target")).unwrap();
        let paths = vec![
            d.join("web/node_modules").to_string_lossy().into_owned(),
            d.join("web/target").to_string_lossy().into_owned(),
        ];
        let out = clean(&[d.clone()], &CleanerSettings::default(), &paths);
        assert!(out[0].ok && out[0].message.contains("npm install"));
        assert!(!d.join("web/node_modules").exists());
        assert!(!out[1].ok);
        assert!(d.join("web/target").exists());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn walks_into_a_same_named_folder_without_its_marker() {
        let d = scratch("nomarker");
        touch(&d.join("mono/vendor/inner/package.json"));
        fs::create_dir_all(d.join("mono/vendor/inner/node_modules")).unwrap();
        assert_eq!(kinds(&d), ["inner:node_modules"]);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn reads_nginx_project_paths_and_tolerates_a_missing_table() {
        // An absolute path on this OS.
        let abs = |p: &str| if cfg!(windows) { format!("C:{p}") } else { p.to_string() };
        let settings = CleanerSettings { project_roots: vec![abs("/a"), abs("/site")], ..CleanerSettings::default() };
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        // No table yet: only the settings roots.
        assert_eq!(project_roots(&conn, &settings), vec![PathBuf::from(abs("/a")), PathBuf::from(abs("/site"))]);
        conn.execute_batch(&format!(
            "CREATE TABLE nginx_ssl_sites (project_path TEXT);
             INSERT INTO nginx_ssl_sites VALUES ('{}'), ('{}'), (NULL), (''), ('relative');",
            abs("/site"),
            abs("/laravel")
        ))
        .unwrap();
        assert_eq!(
            project_roots(&conn, &settings),
            vec![PathBuf::from(abs("/a")), PathBuf::from(abs("/site")), PathBuf::from(abs("/laravel"))]
        );
    }

    #[cfg(windows)]
    #[test]
    fn hidden_folders_are_not_walked_on_windows() {
        let d = scratch("hidden");
        touch(&d.join("AppData/vscode/package.json"));
        fs::create_dir_all(d.join("AppData/vscode/node_modules")).unwrap();
        touch(&d.join("code/app/package.json"));
        fs::create_dir_all(d.join("code/app/node_modules")).unwrap();
        let hide = std::process::Command::new("attrib").arg("+h").arg(d.join("AppData")).output().unwrap();
        assert!(hide.status.success());
        assert_eq!(kinds(&d), ["app:node_modules"]);
        fs::remove_dir_all(&d).unwrap();
    }
}
