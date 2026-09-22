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
use std::path::Path;
use std::sync::Mutex;
use tauri::State;

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

/// Copies `reader` to `writer` in fixed chunks, returning bytes copied.
fn stream_copy(mut reader: impl Read, mut writer: impl Write) -> std::io::Result<u64> {
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        total += n as u64;
    }
    writer.flush()?;
    Ok(total)
}

#[tauri::command(async)]
pub fn sftp_download(state: State<SshState>, session: String, remote: String, local: String) -> Result<u64, String> {
    with_session(&state, &session, |s| {
        let src = s
            .sftp
            .open(Path::new(&remote))
            .map_err(|e| format!("could not open {remote}: {}", e.message()))?;
        let dst = std::fs::File::create(&local).map_err(|e| format!("could not create {local}: {e}"))?;
        stream_copy(src, std::io::BufWriter::new(dst)).map_err(|e| format!("download failed: {e}"))
    })
}

#[tauri::command(async)]
pub fn sftp_upload(state: State<SshState>, session: String, local: String, remote: String) -> Result<u64, String> {
    with_session(&state, &session, |s| {
        let src = std::fs::File::open(&local).map_err(|e| format!("could not open {local}: {e}"))?;
        let dst = s
            .sftp
            .create(Path::new(&remote))
            .map_err(|e| format!("could not create {remote}: {}", e.message()))?;
        stream_copy(std::io::BufReader::new(src), dst).map_err(|e| format!("upload failed: {e}"))
    })
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
        let n = stream_copy(&data[..], &mut out).unwrap();
        assert_eq!(n as usize, data.len());
        assert_eq!(out, data);
    }

    #[test]
    fn upload_target_uses_the_local_file_name() {
        assert_eq!(
            upload_target("/srv", Path::new("/home/me/Downloads/app.tar.gz")).as_deref(),
            Some("/srv/app.tar.gz")
        );
    }
}
