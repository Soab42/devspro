//! File Manager — the local filesystem, one directory at a time.
//!
//! The webview has no filesystem access (the app ships no `tauri-plugin-fs`),
//! so every read, list and mutation is a command here. The surface mirrors the
//! SSH module's SFTP browser on purpose: the same entry shape, the same verbs,
//! so the two front-ends share formatting and sorting code.
//!
//! Deleting goes to the trash, never straight to `unlink`. On a desktop Linux
//! box `gio trash` talks to the same trash the file manager shows, so a mistake
//! is one click from undone. If `gio` is missing we fall back to writing the
//! freedesktop trash layout ourselves rather than deleting for real.
//!
//! "Open" and "Reveal" shell out to `xdg-open` rather than widening the opener
//! plugin's capability: `opener:allow-open-path` needs a path scope, and a file
//! manager's whole point is that the path is anything the user browsed to.

use serde::Serialize;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

/// Same guard as env_diff: the editor is for config and source files, not for
/// dumping a database export into CodeMirror.
const MAX_EDITABLE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub is_hidden: bool,
    pub size: u64,
    /// Unix seconds; 0 when the filesystem won't say.
    pub modified: u64,
    /// Octal mode, e.g. "755".
    pub permissions: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Place {
    pub label: String,
    pub path: String,
    /// One of `home`, `desktop`, `documents`, `downloads`, `pictures`, `music`,
    /// `videos`, `root` — the frontend maps it to an icon.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Places {
    pub home: String,
    pub places: Vec<Place>,
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| "HOME is not set".to_string())
}

/// The XDG user directories, read from `~/.config/user-dirs.dirs` so a renamed
/// or relocated Downloads folder is found where the user actually keeps it.
/// Missing or unparseable entries fall back to the conventional names.
fn xdg_user_dir(home: &Path, key: &str, fallback: &str) -> PathBuf {
    let config = home.join(".config/user-dirs.dirs");
    if let Ok(text) = fs::read_to_string(config) {
        if let Some(path) = parse_user_dir(&text, key, home) {
            return path;
        }
    }
    home.join(fallback)
}

fn parse_user_dir(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
    let prefix = format!("XDG_{key}_DIR=");
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(&prefix) {
            let value = rest.trim().trim_matches('"');
            let expanded = value.strip_prefix("$HOME/").map(|tail| home.join(tail));
            return Some(match expanded {
                Some(p) => p,
                None if value == "$HOME" => home.to_path_buf(),
                None => PathBuf::from(value),
            });
        }
    }
    None
}

#[tauri::command]
pub fn fm_places() -> Result<Places, String> {
    let home = home_dir()?;
    let mut places = vec![Place {
        label: "Home".into(),
        path: home.to_string_lossy().into_owned(),
        kind: "home".into(),
    }];
    let known = [
        ("DESKTOP", "Desktop", "desktop"),
        ("DOCUMENTS", "Documents", "documents"),
        ("DOWNLOAD", "Downloads", "downloads"),
        ("PICTURES", "Pictures", "pictures"),
        ("MUSIC", "Music", "music"),
        ("VIDEOS", "Videos", "videos"),
    ];
    for (key, fallback, kind) in known {
        let dir = xdg_user_dir(&home, key, fallback);
        // A place that resolves back to $HOME (the XDG default when a folder is
        // disabled) would just be a second Home entry.
        if dir.is_dir() && dir != home {
            places.push(Place {
                label: dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| fallback.to_string()),
                path: dir.to_string_lossy().into_owned(),
                kind: kind.into(),
            });
        }
    }
    places.push(Place {
        label: "Computer".into(),
        path: "/".into(),
        kind: "root".into(),
    });
    Ok(Places {
        home: home.to_string_lossy().into_owned(),
        places,
    })
}

fn entry_from(path: &Path) -> Option<FileEntry> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    // symlink_metadata so a dangling link still lists (as a symlink), and so
    // a link to a directory is shown as a link rather than followed on stat.
    let lmeta = fs::symlink_metadata(path).ok()?;
    let is_symlink = lmeta.file_type().is_symlink();
    // For the kind, size and mtime follow the link — that is what the user
    // wants to know about a symlink to a folder.
    let meta = if is_symlink { fs::metadata(path).unwrap_or(lmeta.clone()) } else { lmeta.clone() };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some(FileEntry {
        is_hidden: name.starts_with('.'),
        name,
        path: path.to_string_lossy().into_owned(),
        is_dir: meta.is_dir(),
        is_symlink,
        size: if meta.is_dir() { 0 } else { meta.len() },
        modified,
        permissions: format!("{:o}", meta.permissions().mode() & 0o777),
    })
}

/// Directories first, then case-insensitive by name — the frontend re-sorts
/// by column, but this is the order a fresh listing appears in.
fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

fn list_dir(path: &Path) -> Result<Vec<FileEntry>, String> {
    let display = path.display();
    if !path.is_dir() {
        return Err(format!("{display} is not a directory"));
    }
    let read = fs::read_dir(path).map_err(|e| format!("could not open {display}: {e}"))?;
    let mut entries: Vec<FileEntry> = read
        .filter_map(|r| r.ok())
        .filter_map(|d| entry_from(&d.path()))
        .collect();
    sort_entries(&mut entries);
    Ok(entries)
}

#[tauri::command(async)]
pub fn fm_list(path: String) -> Result<Vec<FileEntry>, String> {
    list_dir(Path::new(&path))
}

/// The entry for one path, so the page can show what it is about to act on
/// (and refresh a single row) without re-listing the directory.
#[tauri::command]
pub fn fm_stat(path: String) -> Result<FileEntry, String> {
    entry_from(Path::new(&path)).ok_or_else(|| format!("{path} does not exist"))
}

fn checked_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("enter a name".into());
    }
    if name.contains('/') || name == "." || name == ".." {
        return Err("names cannot contain slashes".into());
    }
    Ok(name)
}

fn child_path(dir: &str, name: &str) -> Result<PathBuf, String> {
    let parent = Path::new(dir);
    if !parent.is_dir() {
        return Err(format!("{dir} is not a directory"));
    }
    let target = parent.join(checked_name(name)?);
    if target.exists() {
        return Err(format!("{} already exists", target.display()));
    }
    Ok(target)
}

#[tauri::command]
pub fn fm_mkdir(dir: String, name: String) -> Result<String, String> {
    let target = child_path(&dir, &name)?;
    fs::create_dir(&target).map_err(|e| format!("could not create folder: {e}"))?;
    Ok(target.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn fm_create_file(dir: String, name: String) -> Result<String, String> {
    let target = child_path(&dir, &name)?;
    fs::File::create_new(&target).map_err(|e| format!("could not create file: {e}"))?;
    Ok(target.to_string_lossy().into_owned())
}

/// Rename within the same directory only. Moving across directories is a
/// different gesture (drag, or cut and paste) with different failure modes
/// (cross-device), and this dialog only ever offers a new name.
#[tauri::command]
pub fn fm_rename(path: String, name: String) -> Result<String, String> {
    let source = Path::new(&path);
    let parent = source
        .parent()
        .ok_or_else(|| "cannot rename the root".to_string())?;
    let target = parent.join(checked_name(&name)?);
    if target == source {
        return Ok(path);
    }
    if target.exists() {
        return Err(format!("{} already exists", target.display()));
    }
    fs::rename(source, &target).map_err(|e| format!("could not rename: {e}"))?;
    Ok(target.to_string_lossy().into_owned())
}

/// freedesktop trash spec, the minimum a desktop file manager needs to list
/// and restore the item: the file under `files/`, and a `.trashinfo` beside it
/// with the original path and deletion time.
fn trash_manually(path: &Path, home: &Path) -> Result<(), String> {
    let trash = home.join(".local/share/Trash");
    let files = trash.join("files");
    let info = trash.join("info");
    fs::create_dir_all(&files).map_err(|e| format!("could not create trash: {e}"))?;
    fs::create_dir_all(&info).map_err(|e| format!("could not create trash: {e}"))?;

    let base = path
        .file_name()
        .ok_or_else(|| "cannot trash the root".to_string())?
        .to_string_lossy()
        .into_owned();
    // Avoid clobbering an earlier trashed item of the same name.
    let mut candidate = base.clone();
    let mut n = 2;
    while files.join(&candidate).exists() || info.join(format!("{candidate}.trashinfo")).exists() {
        candidate = format!("{base}.{n}");
        n += 1;
    }
    let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S");
    let record = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        path.to_string_lossy(),
        now
    );
    fs::write(info.join(format!("{candidate}.trashinfo")), record)
        .map_err(|e| format!("could not write trash record: {e}"))?;
    fs::rename(path, files.join(&candidate)).map_err(|e| {
        let _ = fs::remove_file(info.join(format!("{candidate}.trashinfo")));
        format!("could not move to trash: {e}")
    })
}

#[tauri::command(async)]
pub fn fm_trash(path: String) -> Result<String, String> {
    let target = Path::new(&path);
    if fs::symlink_metadata(target).is_err() {
        return Err(format!("{path} does not exist"));
    }
    if target.parent().is_none() {
        return Err("cannot trash the root".into());
    }
    let gio = Command::new("gio").arg("trash").arg(target).output();
    match gio {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            // gio exists but refused (e.g. a path outside the trash's mount).
            // Its message is the most useful thing we have, so keep it.
            let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if !msg.is_empty() {
                return Err(msg);
            }
            trash_manually(target, &home_dir()?)?;
        }
        Err(_) => trash_manually(target, &home_dir()?)?,
    }
    Ok(format!("Moved {} to the trash", target.display()))
}

fn regular_file(path: &str) -> Result<fs::Metadata, String> {
    let meta = fs::metadata(Path::new(path)).map_err(|e| format!("could not read {path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{path} is not a file"));
    }
    Ok(meta)
}

#[tauri::command]
pub fn fm_read_file(path: String) -> Result<String, String> {
    let meta = regular_file(&path)?;
    if meta.len() > MAX_EDITABLE_BYTES {
        return Err(format!(
            "{path} is {} KB — too large to open in the editor (limit {} KB)",
            meta.len() / 1024,
            MAX_EDITABLE_BYTES / 1024
        ));
    }
    let bytes = fs::read(&path).map_err(|e| format!("could not read {path}: {e}"))?;
    String::from_utf8(bytes).map_err(|_| format!("{path} is not a text file"))
}

/// In place, for the same reason as env_diff: permissions survive and no
/// backup file appears next to the original.
#[tauri::command]
pub fn fm_write_file(path: String, contents: String) -> Result<String, String> {
    regular_file(&path)?;
    fs::write(&path, contents).map_err(|e| format!("could not write {path}: {e}"))?;
    Ok(format!("Saved {path}"))
}

/// Opens the path with the desktop's default application. Detached: xdg-open
/// returns once it has handed off, but some handlers hold the terminal.
#[tauri::command]
pub fn fm_open(path: String) -> Result<(), String> {
    if fs::symlink_metadata(&path).is_err() {
        return Err(format!("{path} does not exist"));
    }
    Command::new("xdg-open")
        .arg(&path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open {path}: {e}"))
}

/// Splits an editor command the way a shell would for the simple cases —
/// whitespace-separated, with single or double quotes grouping a word — and
/// appends the file. "code -w", "gnome-text-editor", "\"/opt/My Editor/bin/ed\" -n".
pub fn editor_argv(command: &str, path: &Path) -> Result<Vec<String>, String> {
    let mut argv: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    for c in command.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '"') | (None, '\'') => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !word.is_empty() {
                    argv.push(std::mem::take(&mut word));
                }
            }
            (None, c) => word.push(c),
        }
    }
    if quote.is_some() {
        return Err("editor command has an unclosed quote".into());
    }
    if !word.is_empty() {
        argv.push(word);
    }
    if argv.is_empty() {
        return Err("no editor command is set — choose one in Settings".into());
    }
    argv.push(path.to_string_lossy().into_owned());
    Ok(argv)
}

/// Launches the user's editor on a path, detached. The editor is whatever
/// command they typed in Settings — `code`, `subl`, `zed` — so a missing
/// binary is reported by name rather than as a bare ENOENT.
pub fn spawn_editor(command: &str, path: &Path) -> Result<(), String> {
    let argv = editor_argv(command, path)?;
    Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start `{}`: {e}", argv[0]))
}

#[tauri::command]
pub fn fm_open_in_editor(path: String, command: String) -> Result<(), String> {
    if fs::symlink_metadata(&path).is_err() {
        return Err(format!("{path} does not exist"));
    }
    spawn_editor(&command, Path::new(&path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devspro-fm-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_directories_first_then_files_case_insensitively() {
        let dir = scratch();
        fs::write(dir.join("b.txt"), "b").unwrap();
        fs::write(dir.join("A.txt"), "a").unwrap();
        fs::create_dir(dir.join("zeta")).unwrap();
        fs::write(dir.join(".hidden"), "").unwrap();

        let names: Vec<_> = list_dir(&dir).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["zeta", ".hidden", "A.txt", "b.txt"]);

        let entries = list_dir(&dir).unwrap();
        assert!(entries[0].is_dir);
        assert!(entries[1].is_hidden);
        assert_eq!(entries[3].size, 1);
        assert!(entries[3].modified > 0);
    }

    #[test]
    fn editor_argv_splits_words_and_quotes_then_appends_the_path() {
        let p = Path::new("/tmp/a b.txt");
        assert_eq!(editor_argv("code -w", p).unwrap(), vec!["code", "-w", "/tmp/a b.txt"]);
        assert_eq!(
            editor_argv("\"/opt/My Editor/ed\" -n", p).unwrap(),
            vec!["/opt/My Editor/ed", "-n", "/tmp/a b.txt"]
        );
        assert!(editor_argv("   ", p).is_err());
        assert!(editor_argv("code \"unclosed", p).is_err());
    }

    #[test]
    fn list_rejects_a_file() {
        let dir = scratch();
        let file = dir.join("f");
        fs::write(&file, "").unwrap();
        assert!(list_dir(&file).unwrap_err().contains("not a directory"));
    }

    #[test]
    fn symlinks_are_flagged_and_follow_their_target_kind() {
        let dir = scratch();
        fs::create_dir(dir.join("real")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();
        let entries = list_dir(&dir).unwrap();
        let link = entries.iter().find(|e| e.name == "link").unwrap();
        assert!(link.is_symlink);
        assert!(link.is_dir);
    }

    #[test]
    fn mkdir_create_and_rename_refuse_slashes_and_collisions() {
        let dir = scratch();
        let d = dir.to_string_lossy().into_owned();
        fm_mkdir(d.clone(), "sub".into()).unwrap();
        assert!(dir.join("sub").is_dir());
        assert!(fm_mkdir(d.clone(), "sub".into()).unwrap_err().contains("already exists"));
        assert!(fm_mkdir(d.clone(), "a/b".into()).unwrap_err().contains("slashes"));
        assert!(fm_mkdir(d.clone(), "  ".into()).unwrap_err().contains("name"));

        let file = fm_create_file(d.clone(), "notes.md".into()).unwrap();
        assert!(Path::new(&file).is_file());

        let renamed = fm_rename(file.clone(), "todo.md".into()).unwrap();
        assert!(!Path::new(&file).exists());
        assert!(Path::new(&renamed).is_file());
        assert!(fm_rename(renamed.clone(), "sub".into()).unwrap_err().contains("already exists"));
        assert_eq!(fm_rename(renamed.clone(), "todo.md".into()).unwrap(), renamed);
    }

    #[test]
    fn read_and_write_round_trip_with_caps() {
        let dir = scratch();
        let path = dir.join("a.txt").to_string_lossy().into_owned();
        fs::write(&path, "hello").unwrap();
        assert_eq!(fm_read_file(path.clone()).unwrap(), "hello");
        fm_write_file(path.clone(), "bye".into()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "bye");

        let missing = dir.join("nope").to_string_lossy().into_owned();
        assert!(fm_write_file(missing.clone(), "x".into()).is_err());
        assert!(!Path::new(&missing).exists());

        let bin = dir.join("b.bin").to_string_lossy().into_owned();
        fs::write(&bin, [0xff, 0xfe]).unwrap();
        assert!(fm_read_file(bin).unwrap_err().contains("not a text file"));
    }

    #[test]
    fn manual_trash_writes_a_trashinfo_record() {
        // A scratch home, so the test never touches the real trash.
        let home = scratch();
        let victim = home.join("doomed.txt");
        fs::write(&victim, "x").unwrap();
        trash_manually(&victim, &home).unwrap();
        assert!(!victim.exists());
        assert!(home.join(".local/share/Trash/files/doomed.txt").exists());
        let info = fs::read_to_string(home.join(".local/share/Trash/info/doomed.txt.trashinfo")).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath="));
        assert!(info.contains("DeletionDate="));
    }

    #[test]
    fn parses_xdg_user_dirs() {
        let home = Path::new("/home/x");
        let text = "# comment\nXDG_DOWNLOAD_DIR=\"$HOME/Incoming\"\nXDG_DESKTOP_DIR=\"$HOME\"\nXDG_MUSIC_DIR=\"/mnt/music\"\n";
        assert_eq!(parse_user_dir(text, "DOWNLOAD", home), Some(PathBuf::from("/home/x/Incoming")));
        assert_eq!(parse_user_dir(text, "DESKTOP", home), Some(PathBuf::from("/home/x")));
        assert_eq!(parse_user_dir(text, "MUSIC", home), Some(PathBuf::from("/mnt/music")));
        assert_eq!(parse_user_dir(text, "VIDEOS", home), None);
    }
}
