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
use std::os::unix::fs::{MetadataExt, PermissionsExt};
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

#[tauri::command(async)]
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
#[tauri::command(async)]
pub fn fm_stat(path: String) -> Result<FileEntry, String> {
    entry_from(Path::new(&path)).ok_or_else(|| format!("{path} does not exist"))
}

/// Everything the column view's preview card shows for one path: the plain
/// entry plus what a listing does not carry (owner, creation time, MIME type,
/// the `rw-r--r--` spelling of the mode).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FileDetail {
    pub entry: FileEntry,
    /// Unix seconds; 0 when the filesystem does not record it.
    pub created: u64,
    pub owner: String,
    pub group: String,
    /// `rw-r--r--` — the mode as `ls -l` prints it.
    pub mode_symbolic: String,
    pub mime: String,
}

/// `ls -l` style permission triads for the low nine mode bits.
fn symbolic_mode(mode: u32) -> String {
    let mut out = String::with_capacity(9);
    for shift in [6u32, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        out.push(if bits & 0o4 != 0 { 'r' } else { '-' });
        out.push(if bits & 0o2 != 0 { 'w' } else { '-' });
        out.push(if bits & 0o1 != 0 { 'x' } else { '-' });
    }
    out
}

/// Resolves a uid or gid through `/etc/passwd` or `/etc/group`. Falls back to
/// the number itself: LDAP-backed accounts do not appear there and the id is
/// still more useful than nothing.
fn name_for_id(table: &str, id: u32) -> String {
    let wanted = id.to_string();
    fs::read_to_string(table)
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let mut cols = line.split(':');
                let name = cols.next()?;
                let _password = cols.next()?;
                (cols.next()? == wanted).then(|| name.to_string())
            })
        })
        .unwrap_or(wanted)
}

/// A MIME type from the extension alone — no sniffing, since it is only shown
/// as a label. Anything unknown is `application/octet-stream`, the honest
/// answer for a file we have not looked inside.
fn mime_for(name: &str, is_dir: bool) -> String {
    if is_dir {
        return "inode/directory".to_string();
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let mime = match ext.as_str() {
        "txt" | "log" | "env" | "lock" | "conf" | "ini" | "" => "text/plain",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" | "scss" => "text/css",
        "csv" => "text/csv",
        "js" | "mjs" | "cjs" | "jsx" => "text/javascript",
        "ts" | "tsx" => "text/typescript",
        "json" => "application/json",
        "yml" | "yaml" => "application/yaml",
        "toml" => "application/toml",
        "xml" => "application/xml",
        "sql" => "application/sql",
        "sh" | "bash" | "zsh" => "application/x-sh",
        "py" => "text/x-python",
        "rs" => "text/x-rust",
        "go" => "text/x-go",
        "rb" => "text/x-ruby",
        "php" => "application/x-httpd-php",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "tar" => "application/x-tar",
        "xz" => "application/x-xz",
        "7z" => "application/x-7z-compressed",
        _ => "application/octet-stream",
    };
    mime.to_string()
}

/// The preview card's data for one path. Follows symlinks the same way
/// `entry_from` does, so the owner and dates describe the target.
#[tauri::command(async)]
pub fn fm_stat_detail(path: String) -> Result<FileDetail, String> {
    let entry = entry_from(Path::new(&path)).ok_or_else(|| format!("{path} does not exist"))?;
    let meta = fs::metadata(&path)
        .or_else(|_| fs::symlink_metadata(&path))
        .map_err(|e| format!("could not stat {path}: {e}"))?;
    let created = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(FileDetail {
        mime: mime_for(&entry.name, entry.is_dir),
        mode_symbolic: symbolic_mode(meta.mode()),
        owner: name_for_id("/etc/passwd", meta.uid()),
        group: name_for_id("/etc/group", meta.gid()),
        created,
        entry,
    })
}

/// What the Properties dialog adds for a folder: how much is underneath it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DirStats {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    /// True when the walk stopped at the cap, so the numbers are a floor.
    pub truncated: bool,
}

const DIR_STATS_CAP: u64 = 250_000;

fn walk_stats(dir: &Path, stats: &mut DirStats) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.filter_map(|r| r.ok()) {
        if stats.files + stats.dirs >= DIR_STATS_CAP {
            stats.truncated = true;
            return;
        }
        // symlink_metadata: a link to a directory is counted as one file, never
        // followed — a link back up the tree would otherwise loop forever.
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            stats.dirs += 1;
            walk_stats(&entry.path(), stats);
            if stats.truncated {
                return;
            }
        } else {
            stats.files += 1;
            stats.bytes += meta.len();
        }
    }
}

/// Size and count of everything under a folder, capped so Properties on `/`
/// answers in a moment rather than walking the whole disk.
#[tauri::command(async)]
pub fn fm_dir_stats(path: String) -> Result<DirStats, String> {
    let dir = Path::new(&path);
    if !dir.is_dir() {
        return Err(format!("{path} is not a directory"));
    }
    let mut stats = DirStats { files: 0, dirs: 0, bytes: 0, truncated: false };
    walk_stats(dir, &mut stats);
    Ok(stats)
}

/// A code editor the "Open in" menu can offer: its menu label and the command
/// that takes a path as its last argument.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EditorApp {
    pub id: String,
    pub label: String,
    pub command: String,
}

/// The editors worth looking for, in menu order. The command is the binary
/// each one installs on PATH; Flatpak or snap installs that expose no
/// binary do not show up, and the configured editor from Settings covers those.
const KNOWN_EDITORS: &[(&str, &str, &str)] = &[
    ("code", "VS Code", "code"),
    ("code-insiders", "VS Code Insiders", "code-insiders"),
    ("codium", "VSCodium", "codium"),
    ("cursor", "Cursor", "cursor"),
    ("windsurf", "Windsurf", "windsurf"),
    ("zed", "Zed", "zed"),
    ("subl", "Sublime Text", "subl"),
    ("idea", "IntelliJ IDEA", "idea"),
    ("phpstorm", "PhpStorm", "phpstorm"),
    ("webstorm", "WebStorm", "webstorm"),
    ("pycharm", "PyCharm", "pycharm"),
    ("nvim-qt", "Neovim", "nvim-qt"),
    ("gedit", "Text Editor", "gedit"),
    ("gnome-text-editor", "Text Editor", "gnome-text-editor"),
];

fn on_path(bin: &str, path_var: &str) -> bool {
    std::env::split_paths(path_var).any(|dir| {
        let candidate = dir.join(bin);
        fs::metadata(&candidate).map(|m| m.is_file()).unwrap_or(false)
    })
}

fn detect_editors(path_var: &str) -> Vec<EditorApp> {
    KNOWN_EDITORS
        .iter()
        .filter(|(_, _, bin)| on_path(bin, path_var))
        .map(|(id, label, command)| EditorApp {
            id: id.to_string(),
            label: label.to_string(),
            command: command.to_string(),
        })
        .collect()
}

/// The code editors installed on this machine, for the "Open in" menu.
#[tauri::command(async)]
pub fn fm_editors() -> Vec<EditorApp> {
    detect_editors(&std::env::var("PATH").unwrap_or_default())
}

/// The system terminals worth trying, in order, with the flag each takes for
/// its starting directory. `$TERMINAL` (when set) and the Debian alternative
/// `x-terminal-emulator` go first; the latter takes no directory flag that is
/// portable, so it is launched from the directory instead.
const KNOWN_TERMINALS: &[(&str, &[&str])] = &[
    ("gnome-terminal", &["--working-directory"]),
    ("konsole", &["--workdir"]),
    ("xfce4-terminal", &["--working-directory"]),
    ("tilix", &["--working-directory"]),
    ("kitty", &["--directory"]),
    ("alacritty", &["--working-directory"]),
    ("wezterm", &["start", "--cwd"]),
    ("ptyxis", &["--working-directory"]),
    ("mate-terminal", &["--working-directory"]),
    ("terminator", &["--working-directory"]),
    ("lxterminal", &["--working-directory"]),
    ("foot", &["--working-directory"]),
    ("x-terminal-emulator", &[]),
    ("xterm", &[]),
];

/// The argv that opens a system terminal in `dir`: the first installed
/// candidate, with its directory flag when it has one. Pure, for the test;
/// `fm_open_terminal` runs it from `dir` so flagless terminals still land there.
fn terminal_argv(dir: &str, path_var: &str, terminal_env: Option<&str>) -> Option<Vec<String>> {
    let preferred = terminal_env.map(str::trim).filter(|t| !t.is_empty());
    let mut candidates: Vec<(&str, &[&str])> = Vec::new();
    if let Some(t) = preferred {
        let flags = KNOWN_TERMINALS.iter().find(|(n, _)| *n == t).map(|(_, f)| *f).unwrap_or(&[]);
        candidates.push((t, flags));
    }
    candidates.extend(KNOWN_TERMINALS.iter().copied());
    let (name, flags) = candidates.into_iter().find(|(n, _)| on_path(n, path_var))?;
    let mut argv = vec![name.to_string()];
    if !flags.is_empty() {
        argv.extend(flags.iter().map(|f| f.to_string()));
        argv.push(dir.to_string());
    }
    Some(argv)
}

/// Opens the desktop's terminal emulator in a folder.
#[tauri::command(async)]
pub fn fm_open_terminal(dir: String) -> Result<(), String> {
    if !Path::new(&dir).is_dir() {
        return Err(format!("{dir} is not a directory"));
    }
    let path_var = std::env::var("PATH").unwrap_or_default();
    let argv = terminal_argv(&dir, &path_var, std::env::var("TERMINAL").ok().as_deref())
        .ok_or_else(|| "no terminal emulator found on PATH — set $TERMINAL to the one you use".to_string())?;
    Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start {}: {e}", argv[0]))
}

/// A thumbnail for the grid and preview: a data URL the webview can show
/// without filesystem access. `None` when there is nothing to draw — no
/// thumbnailer for the type, or an image nobody could decode.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Thumbnail {
    pub data_url: String,
}

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "avif", "tiff", "tif"];
const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v", "ogv", "wmv"];
/// Images small enough to hand over as they are when no thumbnailer exists.
const RAW_IMAGE_CAP: u64 = 6 * 1024 * 1024;

fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Where rendered thumbnails live between runs: keyed by path, size and
/// mtime, so an edited image gets a fresh one and an untouched one is free.
fn thumb_cache_path(path: &Path, size: u32, meta: &fs::Metadata) -> Result<PathBuf, String> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    size.hash(&mut h);
    meta.len().hash(&mut h);
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).hash(&mut h);
    let dir = home_dir()?.join(".cache/devspro/thumbs");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create thumbnail cache: {e}"))?;
    Ok(dir.join(format!("{:016x}.png", h.finish())))
}

/// Runs the first installed thumbnailer whose argv template fits, writing to
/// `out`. Templates use `{in}`, `{out}` and `{size}`.
fn render_with(candidates: &[&[&str]], input: &Path, out: &Path, size: u32, path_var: &str) -> bool {
    for argv in candidates {
        if !on_path(argv[0], path_var) {
            continue;
        }
        let args: Vec<String> = argv[1..]
            .iter()
            .map(|a| {
                a.replace("{in}", &input.to_string_lossy())
                    .replace("{out}", &out.to_string_lossy())
                    .replace("{size}", &size.to_string())
            })
            .collect();
        let ok = Command::new(argv[0])
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|st| st.success())
            .unwrap_or(false);
        if ok && out.is_file() {
            return true;
        }
    }
    false
}

const IMAGE_THUMBNAILERS: &[&[&str]] = &[
    &["gdk-pixbuf-thumbnailer", "-s", "{size}", "{in}", "{out}"],
    &["convert", "{in}[0]", "-thumbnail", "{size}x{size}", "{out}"],
    &["magick", "{in}[0]", "-thumbnail", "{size}x{size}", "{out}"],
];

const VIDEO_THUMBNAILERS: &[&[&str]] = &[
    &["ffmpegthumbnailer", "-i", "{in}", "-o", "{out}", "-s", "{size}"],
    &["totem-video-thumbnailer", "-s", "{size}", "{in}", "{out}"],
    &["ffmpeg", "-y", "-loglevel", "quiet", "-ss", "1", "-i", "{in}", "-frames:v", "1", "-vf", "scale={size}:-1", "{out}"],
];

/// Renders (or fetches from the cache) a thumbnail at most `size` px wide.
#[tauri::command(async)]
pub fn fm_thumbnail(path: String, size: u32) -> Result<Option<Thumbnail>, String> {
    use base64::Engine;
    let file = Path::new(&path);
    let meta = fs::metadata(file).map_err(|e| format!("could not read {path}: {e}"))?;
    if !meta.is_file() {
        return Ok(None);
    }
    let size = size.clamp(32, 512);
    let ext = ext_of(file);
    let is_image = IMAGE_EXTS.contains(&ext.as_str());
    let is_video = VIDEO_EXTS.contains(&ext.as_str());
    if !is_image && !is_video {
        return Ok(None);
    }
    // SVG is already a tiny vector the webview draws itself.
    if ext == "svg" && meta.len() <= RAW_IMAGE_CAP {
        let bytes = fs::read(file).map_err(|e| format!("could not read {path}: {e}"))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        return Ok(Some(Thumbnail { data_url: format!("data:image/svg+xml;base64,{b64}") }));
    }
    let out = thumb_cache_path(file, size, &meta)?;
    let path_var = std::env::var("PATH").unwrap_or_default();
    let rendered = out.is_file()
        || render_with(if is_video { VIDEO_THUMBNAILERS } else { IMAGE_THUMBNAILERS }, file, &out, size, &path_var);
    if rendered {
        let bytes = fs::read(&out).map_err(|e| format!("could not read thumbnail: {e}"))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        return Ok(Some(Thumbnail { data_url: format!("data:image/png;base64,{b64}") }));
    }
    // No thumbnailer: a small image can be shown as it is; a video cannot.
    if is_image && meta.len() <= RAW_IMAGE_CAP {
        let bytes = fs::read(file).map_err(|e| format!("could not read {path}: {e}"))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let mime = mime_for(&file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), false);
        return Ok(Some(Thumbnail { data_url: format!("data:{mime};base64,{b64}") }));
    }
    Ok(None)
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

#[tauri::command(async)]
pub fn fm_mkdir(dir: String, name: String) -> Result<String, String> {
    let target = child_path(&dir, &name)?;
    fs::create_dir(&target).map_err(|e| format!("could not create folder: {e}"))?;
    Ok(target.to_string_lossy().into_owned())
}

#[tauri::command(async)]
pub fn fm_create_file(dir: String, name: String) -> Result<String, String> {
    let target = child_path(&dir, &name)?;
    fs::File::create_new(&target).map_err(|e| format!("could not create file: {e}"))?;
    Ok(target.to_string_lossy().into_owned())
}

/// Rename within the same directory only. Moving across directories is a
/// different gesture (drag, or cut and paste) with different failure modes
/// (cross-device), and this dialog only ever offers a new name.
#[tauri::command(async)]
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

/// Move a file or folder into another directory, keeping its name. This is
/// the drag-and-drop gesture `fm_rename` leaves alone. A plain rename first;
/// when that fails because `dir` is on another filesystem, fall back to a
/// copy and delete for files and folders alike.
#[tauri::command(async)]
pub fn fm_move(path: String, dir: String) -> Result<String, String> {
    let source = Path::new(&path);
    let name = source
        .file_name()
        .ok_or_else(|| "cannot move the root".to_string())?;
    let parent = Path::new(&dir);
    if !parent.is_dir() {
        return Err(format!("{dir} is not a directory"));
    }
    let target = parent.join(name);
    if target == source {
        return Ok(path);
    }
    if source.is_dir() && target.starts_with(source) {
        return Err("cannot move a folder into itself".into());
    }
    if target.exists() {
        return Err(format!("{} already exists", target.display()));
    }
    match fs::rename(source, &target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            copy_tree(source, &target)?;
            let removed = if source.is_dir() {
                fs::remove_dir_all(source)
            } else {
                fs::remove_file(source)
            };
            removed.map_err(|e| format!("copied, but could not remove the original: {e}"))?;
        }
        Err(e) => return Err(format!("could not move: {e}")),
    }
    Ok(target.to_string_lossy().into_owned())
}

/// Copy a file or folder into another directory, keeping its name — the
/// paste after a copy. Never overwrites: a clash is an error, the same as
/// rename and move.
#[tauri::command(async)]
pub fn fm_copy(path: String, dir: String) -> Result<String, String> {
    let source = Path::new(&path);
    let name = source
        .file_name()
        .ok_or_else(|| "cannot copy the root".to_string())?;
    let parent = Path::new(&dir);
    if !parent.is_dir() {
        return Err(format!("{dir} is not a directory"));
    }
    if source.is_dir() && parent.starts_with(source) {
        return Err("cannot copy a folder into itself".into());
    }
    let mut target = parent.join(name);
    // Pasting beside the original gets a "copy" suffix rather than a refusal.
    if target == source {
        let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = source.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let mut n = 1;
        loop {
            let candidate = if n == 1 { format!("{stem} copy{ext}") } else { format!("{stem} copy {n}{ext}") };
            target = parent.join(candidate);
            if !target.exists() {
                break;
            }
            n += 1;
        }
    }
    if target.exists() {
        return Err(format!("{} already exists", target.display()));
    }
    copy_tree(source, &target)?;
    Ok(target.to_string_lossy().into_owned())
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(source).map_err(|e| format!("could not read {}: {e}", source.display()))?;
    if meta.is_dir() {
        fs::create_dir(target).map_err(|e| format!("could not create {}: {e}", target.display()))?;
        for entry in fs::read_dir(source).map_err(|e| format!("could not list {}: {e}", source.display()))? {
            let entry = entry.map_err(|e| format!("could not list {}: {e}", source.display()))?;
            copy_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else if meta.file_type().is_symlink() {
        let link = fs::read_link(source).map_err(|e| format!("could not read link {}: {e}", source.display()))?;
        std::os::unix::fs::symlink(link, target).map_err(|e| format!("could not create {}: {e}", target.display()))?;
    } else {
        fs::copy(source, target).map_err(|e| format!("could not copy {}: {e}", source.display()))?;
    }
    Ok(())
}

/// freedesktop trash spec, the minimum a desktop file manager needs to list
/// and restore the item: the file under `files/`, and a `.trashinfo` beside it
/// with the original path and deletion time.
pub(crate) fn trash_manually(path: &Path, home: &Path) -> Result<(), String> {
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

/// Moves one path to the trash: `gio trash`, else the freedesktop layout by
/// hand. Shared with the Cleaner, which must never delete for real by default.
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    let gio = Command::new("gio").arg("trash").arg(path).output();
    match gio {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            // gio exists but refused (e.g. a path outside the trash's mount).
            // Its message is the most useful thing we have, so keep it.
            let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if !msg.is_empty() {
                return Err(msg);
            }
            trash_manually(path, &home_dir()?)
        }
        Err(_) => trash_manually(path, &home_dir()?),
    }
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
    move_to_trash(target)?;
    Ok(format!("Moved {} to the trash", target.display()))
}

fn regular_file(path: &str) -> Result<fs::Metadata, String> {
    let meta = fs::metadata(Path::new(path)).map_err(|e| format!("could not read {path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{path} is not a file"));
    }
    Ok(meta)
}

#[tauri::command(async)]
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
#[tauri::command(async)]
pub fn fm_write_file(path: String, contents: String) -> Result<String, String> {
    regular_file(&path)?;
    fs::write(&path, contents).map_err(|e| format!("could not write {path}: {e}"))?;
    Ok(format!("Saved {path}"))
}

/// Opens the path with the desktop's default application. Detached: xdg-open
/// returns once it has handed off, but some handlers hold the terminal.
#[tauri::command(async)]
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

#[tauri::command(async)]
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
    fn move_keeps_the_name_and_refuses_a_folder_into_itself() {
        let dir = scratch();
        let file = dir.join("a.txt");
        fs::write(&file, "a").unwrap();
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();

        let moved = fm_move(file.to_string_lossy().into_owned(), dest.to_string_lossy().into_owned()).unwrap();
        assert_eq!(Path::new(&moved), dest.join("a.txt"));
        assert!(!file.exists());
        assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "a");

        // Already there: nothing to do, no error.
        let same = fm_move(moved.clone(), dest.to_string_lossy().into_owned()).unwrap();
        assert_eq!(same, moved);

        let err = fm_move(dest.to_string_lossy().into_owned(), dest.join("inner").to_string_lossy().into_owned()).unwrap_err();
        assert!(err.contains("not a directory"), "{err}");
        let err = fm_move(dest.to_string_lossy().into_owned(), dest.to_string_lossy().into_owned()).unwrap_err();
        assert!(err.contains("into itself"), "{err}");
        let err = fm_move(dir.join("missing").to_string_lossy().into_owned(), dest.to_string_lossy().into_owned()).unwrap_err();
        assert!(err.contains("could not move"), "{err}");
    }

    #[test]
    fn copy_tree_copies_folders_files_and_links() {
        let dir = scratch();
        let src = dir.join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub/f.txt"), "f").unwrap();
        std::os::unix::fs::symlink("sub/f.txt", src.join("link")).unwrap();

        copy_tree(&src, &dir.join("out")).unwrap();
        assert_eq!(fs::read_to_string(dir.join("out/sub/f.txt")).unwrap(), "f");
        assert_eq!(fs::read_link(dir.join("out/link")).unwrap(), Path::new("sub/f.txt"));
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

    #[test]
    fn symbolic_mode_spells_the_triads() {
        assert_eq!(symbolic_mode(0o644), "rw-r--r--");
        assert_eq!(symbolic_mode(0o755), "rwxr-xr-x");
        assert_eq!(symbolic_mode(0o100600), "rw-------");
    }

    #[test]
    fn mime_is_by_extension_with_an_honest_fallback() {
        assert_eq!(mime_for("a.PNG", false), "image/png");
        assert_eq!(mime_for("Makefile", false), "text/plain");
        assert_eq!(mime_for("blob.bin", false), "application/octet-stream");
        assert_eq!(mime_for("src", true), "inode/directory");
    }

    #[test]
    fn unknown_ids_fall_back_to_the_number() {
        assert_eq!(name_for_id("/nonexistent/passwd", 4242), "4242");
    }

    #[test]
    fn stat_detail_describes_a_file() {
        let dir = scratch();
        let file = dir.join("note.md");
        fs::write(&file, "hi").unwrap();
        let detail = fm_stat_detail(file.to_string_lossy().into_owned()).unwrap();
        assert_eq!(detail.entry.name, "note.md");
        assert_eq!(detail.mime, "text/markdown");
        assert_eq!(detail.mode_symbolic.len(), 9);
        assert!(!detail.owner.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dir_stats_counts_the_tree() {
        let dir = scratch();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("a.txt"), "12345").unwrap();
        fs::write(dir.join("sub/b.txt"), "123").unwrap();
        let stats = fm_dir_stats(dir.to_string_lossy().into_owned()).unwrap();
        assert_eq!((stats.files, stats.dirs, stats.bytes, stats.truncated), (2, 1, 8, false));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn editors_and_terminals_are_found_on_a_fake_path() {
        let dir = scratch();
        for bin in ["code", "zed", "konsole"] {
            fs::write(dir.join(bin), "").unwrap();
        }
        let path_var = dir.to_string_lossy().into_owned();
        let editors = detect_editors(&path_var);
        assert_eq!(editors.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["code", "zed"]);
        assert_eq!(
            terminal_argv("/work", &path_var, None).unwrap(),
            vec!["konsole", "--workdir", "/work"]
        );
        // $TERMINAL wins when it is installed, even one we know no flag for.
        fs::write(dir.join("myterm"), "").unwrap();
        assert_eq!(terminal_argv("/work", &path_var, Some("myterm")).unwrap(), vec!["myterm"]);
        // ...and is ignored when it is not.
        assert_eq!(terminal_argv("/work", &path_var, Some("ghost")).unwrap()[0], "konsole");
        assert!(terminal_argv("/work", "/nonexistent", None).is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn thumbnail_hands_back_small_images_and_skips_other_files() {
        let dir = scratch();
        // A 1x1 PNG.
        let png: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0,
            0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0, 1, 0, 0, 5, 0, 1, 0x0D, 0x0A, 0x2D, 0xB4,
            0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        fs::write(dir.join("dot.png"), &png).unwrap();
        fs::write(dir.join("notes.txt"), "hi").unwrap();
        let thumb = fm_thumbnail(dir.join("dot.png").to_string_lossy().into_owned(), 128).unwrap().unwrap();
        assert!(thumb.data_url.starts_with("data:image/"));
        assert!(fm_thumbnail(dir.join("notes.txt").to_string_lossy().into_owned(), 128).unwrap().is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copy_pastes_beside_the_original_with_a_suffix_and_into_folders_by_name() {
        let dir = scratch();
        fs::write(dir.join("a.txt"), "x").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        let d = dir.to_string_lossy().into_owned();
        let beside = fm_copy(format!("{d}/a.txt"), d.clone()).unwrap();
        assert!(beside.ends_with("/a copy.txt"));
        let again = fm_copy(format!("{d}/a.txt"), d.clone()).unwrap();
        assert!(again.ends_with("/a copy 2.txt"));
        let into = fm_copy(format!("{d}/a.txt"), format!("{d}/sub")).unwrap();
        assert!(into.ends_with("/sub/a.txt"));
        assert!(fm_copy(format!("{d}/a.txt"), format!("{d}/sub")).is_err());
        assert!(fm_copy(format!("{d}/sub"), format!("{d}/sub")).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
