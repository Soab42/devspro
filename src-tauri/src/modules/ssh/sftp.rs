//! SFTP file browser over an `ssh2` session.
//!
//! Sessions are held in managed state keyed by an id the frontend passes on
//! every call, so one open connection serves a whole browsing session rather
//! than reconnecting per directory. Transfers stream in fixed chunks: a
//! download of a multi-gigabyte log must not be buffered whole in memory just
//! to be written straight back out to disk.

use serde::Serialize;
use ssh2::{Session, Sftp};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

use super::session::{connect, trust_host, Credentials};
use super::SshState;

pub struct SftpSession {
    session: Session,
    sftp: Sftp,
}

pub type SftpMap = Mutex<HashMap<String, SftpSession>>;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SftpEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// Unix seconds; 0 when the server didn't report one.
    pub modified: u64,
    /// `rwx` bits as an octal string ("755"), as `chmod` would take it.
    pub permissions: String,
}

const CHUNK: usize = 64 * 1024;

/// How many bytes may pass between two progress events. Emitting every 64 KB
/// chunk would flood the webview on a fast link for no visible gain.
const PROGRESS_STEP: u64 = 256 * 1024;

/// One transfer's running total, emitted on `sftp:progress` so the UI can draw
/// a bar. `total` is measured up front by walking the tree, so a directory
/// copy shows one bar for the whole thing rather than resetting per file.
#[derive(Debug, Clone, Serialize)]
pub struct TransferProgress {
    pub id: String,
    pub done: u64,
    pub total: u64,
}

struct Progress<'a> {
    app: Option<(&'a AppHandle, String)>,
    done: u64,
    total: u64,
    last_emit: u64,
}

impl<'a> Progress<'a> {
    fn new(app: Option<(&'a AppHandle, String)>, total: u64) -> Self {
        let p = Progress { app, done: 0, total, last_emit: 0 };
        p.emit();
        p
    }

    fn advance(&mut self, n: u64) {
        self.done += n;
        if self.done - self.last_emit >= PROGRESS_STEP {
            self.emit();
        }
    }

    fn emit(&self) {
        if let Some((app, id)) = &self.app {
            let _ = app.emit(
                "sftp:progress",
                TransferProgress { id: id.clone(), done: self.done, total: self.total },
            );
        }
    }

    fn finish(&mut self) {
        self.last_emit = self.done;
        self.emit();
    }
}

/// `dir` + `name`, without doubling the separator at the root.
pub fn join_remote(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", dir.trim_end_matches('/'))
    }
}

pub fn entry_from_stat(dir: &str, name: &str, stat: &ssh2::FileStat) -> SftpEntry {
    SftpEntry {
        name: name.to_string(),
        path: join_remote(dir, name),
        is_dir: stat.is_dir(),
        size: stat.size.unwrap_or(0),
        modified: stat.mtime.unwrap_or(0),
        permissions: format!("{:o}", stat.perm.unwrap_or(0) & 0o777),
    }
}

fn err(e: ssh2::Error) -> String {
    e.message().to_string()
}

/// Runs a command on the SSH connection behind an SFTP session and returns
/// its stdout — for the status strip, which samples the remote host the same
/// way it samples the local one. Holds the session lock for the duration,
/// which is a few tens of milliseconds for a `/proc` read.
pub fn exec_on_session(state: &SshState, id: &str, script: &str) -> Result<String, String> {
    with_session(state, id, |s| {
        let mut channel = s.session.channel_session().map_err(err)?;
        channel.exec(script).map_err(err)?;
        let mut stdout = String::new();
        channel.read_to_string(&mut stdout).map_err(|e| e.to_string())?;
        let _ = channel.wait_close();
        Ok(stdout)
    })
}

fn with_session<T>(
    state: &SshState,
    id: &str,
    f: impl FnOnce(&SftpSession) -> Result<T, String>,
) -> Result<T, String> {
    let map = state.sftp.lock().map_err(|e| e.to_string())?;
    let sess = map.get(id).ok_or("SFTP session is not connected")?;
    f(sess)
}

/// Opens a session and returns its id. An unknown host key fails with
/// `UNKNOWN_HOST:<fingerprint>` (see `session.rs`); a password host with no
/// password fails with `PASSWORD_REQUIRED`. Both are prompts the UI turns
/// into dialogs and then retries.
#[tauri::command(async)]
pub fn sftp_connect(
    db: State<crate::db::Db>,
    state: State<SshState>,
    host_id: String,
    password: Option<String>,
    passphrase: Option<String>,
) -> Result<String, String> {
    let host = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        super::hosts::find(&conn, &host_id)?
    };
    let session = connect(&host, &Credentials { password, passphrase })?;
    let sftp = session.sftp().map_err(|e| format!("could not start SFTP: {}", e.message()))?;
    let id = uuid::Uuid::new_v4().to_string();
    state
        .sftp
        .lock()
        .map_err(|e| e.to_string())?
        .insert(id.clone(), SftpSession { session, sftp });
    Ok(id)
}

#[tauri::command(async)]
pub fn sftp_trust_host(db: State<crate::db::Db>, host_id: String) -> Result<String, String> {
    let host = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        super::hosts::find(&conn, &host_id)?
    };
    trust_host(&host)
}

#[tauri::command(async)]
pub fn sftp_disconnect(state: State<SshState>, session: String) -> Result<(), String> {
    let removed = state.sftp.lock().map_err(|e| e.to_string())?.remove(&session);
    if let Some(s) = removed {
        let _ = s.session.disconnect(None, "closed", None);
    }
    Ok(())
}

#[tauri::command(async)]
pub fn sftp_home(state: State<SshState>, session: String) -> Result<String, String> {
    with_session(&state, &session, |s| {
        let home = s.sftp.realpath(Path::new(".")).map_err(err)?;
        Ok(home.to_string_lossy().into_owned())
    })
}

#[tauri::command(async)]
pub fn sftp_list(state: State<SshState>, session: String, path: String) -> Result<Vec<SftpEntry>, String> {
    with_session(&state, &session, |s| {
        let entries = s
            .sftp
            .readdir(Path::new(&path))
            .map_err(|e| format!("could not list {path}: {}", e.message()))?;
        let mut out: Vec<SftpEntry> = entries
            .iter()
            .filter_map(|(p, stat)| {
                let name = p.file_name()?.to_str()?;
                Some(entry_from_stat(&path, name, stat))
            })
            .collect();
        out.sort_by(|a, b| (!a.is_dir, a.name.to_lowercase()).cmp(&(!b.is_dir, b.name.to_lowercase())));
        Ok(out)
    })
}

#[tauri::command(async)]
pub fn sftp_mkdir(state: State<SshState>, session: String, path: String) -> Result<(), String> {
    with_session(&state, &session, |s| {
        s.sftp
            .mkdir(Path::new(&path), 0o755)
            .map_err(|e| format!("could not create {path}: {}", e.message()))
    })
}

#[tauri::command(async)]
pub fn sftp_rename(state: State<SshState>, session: String, from: String, to: String) -> Result<(), String> {
    with_session(&state, &session, |s| {
        s.sftp
            .rename(Path::new(&from), Path::new(&to), None)
            .map_err(|e| format!("could not rename {from}: {}", e.message()))
    })
}

/// Removes a file, or a directory that is already empty. Recursive deletion is
/// deliberately not offered: over SFTP it is slow, non-atomic, and the kind of
/// thing that should be typed into the terminal tab with intent.
#[tauri::command(async)]
pub fn sftp_delete(state: State<SshState>, session: String, path: String) -> Result<(), String> {
    with_session(&state, &session, |s| {
        let target = Path::new(&path);
        let stat = s.sftp.lstat(target).map_err(|e| format!("could not stat {path}: {}", e.message()))?;
        if stat.is_dir() {
            s.sftp
                .rmdir(target)
                .map_err(|e| format!("could not remove directory {path} (is it empty?): {}", e.message()))
        } else {
            s.sftp
                .unlink(target)
                .map_err(|e| format!("could not delete {path}: {}", e.message()))
        }
    })
}

/// Same ceiling as the local file manager's editor. Generous, because logs
/// and reports are what people open beside a shell; the webview copes with
/// tens of megabytes of text, so the limit guards memory, not comfort.
const MAX_EDITABLE_BYTES: u64 = 64 * 1024 * 1024;

/// A remote text file's contents, for the in-app editor.
#[tauri::command(async)]
pub fn sftp_read_file(state: State<SshState>, session: String, path: String) -> Result<String, String> {
    with_session(&state, &session, |s| {
        let target = Path::new(&path);
        let stat = s.sftp.stat(target).map_err(|e| format!("could not stat {path}: {}", e.message()))?;
        if stat.is_dir() {
            return Err(format!("{path} is a directory"));
        }
        let size = stat.size.unwrap_or(0);
        if size > MAX_EDITABLE_BYTES {
            return Err(format!(
                "{path} is {} KB — too large to open in the editor (limit {} KB)",
                size / 1024,
                MAX_EDITABLE_BYTES / 1024
            ));
        }
        let mut file = s.sftp.open(target).map_err(|e| format!("could not open {path}: {}", e.message()))?;
        let mut bytes = Vec::with_capacity(size as usize);
        file.read_to_end(&mut bytes).map_err(|e| format!("could not read {path}: {e}"))?;
        String::from_utf8(bytes).map_err(|_| format!("{path} is not a text file"))
    })
}

/// Overwrites a remote file in place, so its mode and ownership survive.
#[tauri::command(async)]
pub fn sftp_write_file(state: State<SshState>, session: String, path: String, contents: String) -> Result<String, String> {
    with_session(&state, &session, |s| {
        let target = Path::new(&path);
        let mut file = s
            .sftp
            .open_mode(target, ssh2::OpenFlags::WRITE | ssh2::OpenFlags::TRUNCATE, 0o644, ssh2::OpenType::File)
            .map_err(|e| format!("could not open {path} for writing: {}", e.message()))?;
        file.write_all(contents.as_bytes()).map_err(|e| format!("could not write {path}: {e}"))?;
        file.flush().map_err(|e| format!("could not write {path}: {e}"))?;
        Ok(format!("Saved {path}"))
    })
}

/// Copies `reader` to `writer` in fixed chunks, calling `on_chunk` with each
/// chunk's size. Returns bytes copied.
fn stream_copy(
    mut reader: impl Read,
    mut writer: impl Write,
    mut on_chunk: impl FnMut(u64),
) -> std::io::Result<u64> {
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        total += n as u64;
        on_chunk(n as u64);
    }
    writer.flush()?;
    Ok(total)
}

/// Bytes under a remote path, so a directory copy can report one total.
fn remote_tree_size(s: &SftpSession, remote: &Path) -> Result<u64, String> {
    let stat = s
        .sftp
        .stat(remote)
        .map_err(|e| format!("could not stat {}: {}", remote.display(), e.message()))?;
    if !stat.is_dir() {
        return Ok(stat.size.unwrap_or(0));
    }
    let children = s
        .sftp
        .readdir(remote)
        .map_err(|e| format!("could not list {}: {}", remote.display(), e.message()))?;
    let mut total = 0u64;
    for (child, _) in children {
        total += remote_tree_size(s, &child)?;
    }
    Ok(total)
}

fn local_tree_size(local: &Path) -> Result<u64, String> {
    let meta = std::fs::metadata(local).map_err(|e| format!("could not read {}: {e}", local.display()))?;
    if !meta.is_dir() {
        return Ok(meta.len());
    }
    let read = std::fs::read_dir(local).map_err(|e| format!("could not list {}: {e}", local.display()))?;
    let mut total = 0u64;
    for entry in read.filter_map(|r| r.ok()) {
        total += local_tree_size(&entry.path())?;
    }
    Ok(total)
}

fn download_file(s: &SftpSession, remote: &Path, local: &Path, progress: &mut Progress) -> Result<u64, String> {
    let src = s
        .sftp
        .open(remote)
        .map_err(|e| format!("could not open {}: {}", remote.display(), e.message()))?;
    let dst = std::fs::File::create(local).map_err(|e| format!("could not create {}: {e}", local.display()))?;
    stream_copy(src, std::io::BufWriter::new(dst), |n| progress.advance(n))
        .map_err(|e| format!("download failed: {e}"))
}

/// A file, or a directory and everything under it. Symlinks are followed,
/// so a link to a folder arrives as a real copy of that folder.
fn download_tree(s: &SftpSession, remote: &Path, local: &Path, progress: &mut Progress) -> Result<u64, String> {
    let stat = s
        .sftp
        .stat(remote)
        .map_err(|e| format!("could not stat {}: {}", remote.display(), e.message()))?;
    if !stat.is_dir() {
        return download_file(s, remote, local, progress);
    }
    std::fs::create_dir_all(local).map_err(|e| format!("could not create {}: {e}", local.display()))?;
    let children = s
        .sftp
        .readdir(remote)
        .map_err(|e| format!("could not list {}: {}", remote.display(), e.message()))?;
    let mut total = 0u64;
    for (child, _) in children {
        let Some(name) = child.file_name() else { continue };
        total += download_tree(s, &child, &local.join(name), progress)?;
    }
    Ok(total)
}

fn upload_file(s: &SftpSession, local: &Path, remote: &Path, progress: &mut Progress) -> Result<u64, String> {
    let src = std::fs::File::open(local).map_err(|e| format!("could not open {}: {e}", local.display()))?;
    let dst = s
        .sftp
        .create(remote)
        .map_err(|e| format!("could not create {}: {}", remote.display(), e.message()))?;
    stream_copy(std::io::BufReader::new(src), dst, |n| progress.advance(n))
        .map_err(|e| format!("upload failed: {e}"))
}

fn upload_tree(s: &SftpSession, local: &Path, remote: &Path, progress: &mut Progress) -> Result<u64, String> {
    let meta = std::fs::metadata(local).map_err(|e| format!("could not read {}: {e}", local.display()))?;
    if !meta.is_dir() {
        return upload_file(s, local, remote, progress);
    }
    // An existing remote directory is fine — we merge into it, like `cp -r`.
    if s.sftp.stat(remote).is_err() {
        s.sftp
            .mkdir(remote, 0o755)
            .map_err(|e| format!("could not create {}: {}", remote.display(), e.message()))?;
    }
    let read = std::fs::read_dir(local).map_err(|e| format!("could not list {}: {e}", local.display()))?;
    let mut total = 0u64;
    for entry in read.filter_map(|r| r.ok()) {
        let name = entry.file_name();
        total += upload_tree(s, &entry.path(), &remote.join(name), progress)?;
    }
    Ok(total)
}

/// Copies a remote file or directory tree to `local`. Returns bytes copied.
/// With a `transfer` id, progress is emitted on `sftp:progress` as it goes.
#[tauri::command(async)]
pub fn sftp_download(
    app: AppHandle,
    state: State<SshState>,
    session: String,
    remote: String,
    local: String,
    transfer: Option<String>,
) -> Result<u64, String> {
    with_session(&state, &session, |s| {
        let (remote, local) = (Path::new(&remote), Path::new(&local));
        let total = remote_tree_size(s, remote)?;
        let mut progress = Progress::new(transfer.map(|id| (&app, id)), total);
        let n = download_tree(s, remote, local, &mut progress)?;
        progress.finish();
        Ok(n)
    })
}

/// Copies a local file or directory tree to `remote`. Returns bytes copied.
/// With a `transfer` id, progress is emitted on `sftp:progress` as it goes.
#[tauri::command(async)]
pub fn sftp_upload(
    app: AppHandle,
    state: State<SshState>,
    session: String,
    local: String,
    remote: String,
    transfer: Option<String>,
) -> Result<u64, String> {
    with_session(&state, &session, |s| {
        let (local, remote) = (Path::new(&local), Path::new(&remote));
        let total = local_tree_size(local)?;
        let mut progress = Progress::new(transfer.map(|id| (&app, id)), total);
        let n = upload_tree(s, local, remote, &mut progress)?;
        progress.finish();
        Ok(n)
    })
}

/// Where a remote file is parked for an external editor: under the user's
/// cache dir, keyed by session so two hosts' `/etc/nginx/nginx.conf` don't
/// collide. The remote path is kept whole so the editor's tab shows a
/// recognisable name and the file's real extension.
pub fn edit_cache_path(cache_root: &Path, session: &str, remote: &str) -> PathBuf {
    cache_root
        .join("devspro")
        .join("sftp-edit")
        .join(session)
        .join(remote.trim_start_matches('/'))
}

fn cache_root() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).filter(|p| p.is_absolute()) {
        return Ok(dir);
    }
    Ok(super::home_dir()?.join(".cache"))
}

/// Downloads a remote file to the edit cache and opens it in the user's
/// editor. Returns the local path, which the UI watches: every save there is
/// uploaded straight back.
#[tauri::command(async)]
pub fn sftp_open_in_editor(
    state: State<SshState>,
    session: String,
    path: String,
    command: String,
) -> Result<String, String> {
    let local = edit_cache_path(&cache_root()?, &session, &path);
    with_session(&state, &session, |s| {
        let stat = s.sftp.stat(Path::new(&path)).map_err(|e| format!("could not stat {path}: {}", e.message()))?;
        if stat.is_dir() {
            return Err(format!("{path} is a directory"));
        }
        if let Some(parent) = local.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        let mut progress = Progress::new(None, stat.size.unwrap_or(0));
        download_file(s, Path::new(&path), &local, &mut progress)
    })?;
    crate::modules::file_manager::spawn_editor(&command, &local)?;
    Ok(local.to_string_lossy().into_owned())
}

/// Where an uploaded file lands: the picked file's name inside the current
/// remote directory.
#[allow(dead_code)]
pub fn upload_target(remote_dir: &str, local: &Path) -> Option<String> {
    let name = local.file_name()?.to_str()?;
    Some(join_remote(remote_dir, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_remote_handles_root_and_trailing_slashes() {
        assert_eq!(join_remote("/", "etc"), "/etc");
        assert_eq!(join_remote("", "etc"), "/etc");
        assert_eq!(join_remote("/home/me", "x"), "/home/me/x");
        assert_eq!(join_remote("/home/me/", "x"), "/home/me/x");
    }

    #[test]
    fn entry_from_stat_maps_fields_and_octal_permissions() {
        let stat = ssh2::FileStat {
            size: Some(1234),
            uid: None,
            gid: None,
            perm: Some(0o100644),
            atime: None,
            mtime: Some(1_700_000_000),
        };
        let e = entry_from_stat("/var/log", "syslog", &stat);
        assert_eq!(
            e,
            SftpEntry {
                name: "syslog".into(),
                path: "/var/log/syslog".into(),
                is_dir: false,
                size: 1234,
                modified: 1_700_000_000,
                permissions: "644".into(),
            }
        );
        let dir = ssh2::FileStat {
            size: None,
            uid: None,
            gid: None,
            perm: Some(0o40755),
            atime: None,
            mtime: None,
        };
        let d = entry_from_stat("/", "etc", &dir);
        assert!(d.is_dir);
        assert_eq!(d.permissions, "755");
        assert_eq!(d.size, 0);
        assert_eq!(d.modified, 0);
    }

    #[test]
    fn stream_copy_moves_every_byte_in_chunks() {
        let data: Vec<u8> = (0..(CHUNK * 3 + 17)).map(|i| (i % 251) as u8).collect();
        let mut out = Vec::new();
        let mut seen = 0u64;
        let n = stream_copy(&data[..], &mut out, |k| seen += k).unwrap();
        assert_eq!(n as usize, data.len());
        assert_eq!(seen as usize, data.len());
        assert_eq!(out, data);
    }

    #[test]
    fn progress_without_an_app_counts_silently() {
        let mut p = Progress::new(None, 10);
        p.advance(4);
        p.advance(6);
        p.finish();
        assert_eq!((p.done, p.total, p.last_emit), (10, 10, 10));
    }

    #[test]
    fn local_tree_size_sums_every_file() {
        let dir = std::env::temp_dir().join(format!("devspro-sftp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("sub/b"), [0u8; 5]).unwrap();
        assert_eq!(local_tree_size(&dir).unwrap(), 15);
        assert_eq!(local_tree_size(&dir.join("a")).unwrap(), 10);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn edit_cache_path_keeps_the_remote_layout_under_the_session() {
        let p = edit_cache_path(Path::new("/home/me/.cache"), "abc", "/etc/nginx/nginx.conf");
        assert_eq!(p, Path::new("/home/me/.cache/devspro/sftp-edit/abc/etc/nginx/nginx.conf"));
    }

    #[test]
    fn upload_target_uses_the_local_file_name() {
        assert_eq!(
            upload_target("/srv", Path::new("/home/me/Downloads/app.tar.gz")).as_deref(),
            Some("/srv/app.tar.gz")
        );
    }
}
