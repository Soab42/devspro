//! GVfs: phones over MTP, Google Drive accounts, SMB and SFTP shares. The
//! desktop's gvfsd-fuse bridge puts every GVfs mount under
//! `$XDG_RUNTIME_DIR/gvfs`, so the plain `std::fs` calls the rest of the File
//! Manager makes already work there.
//!
//! One backend needs more than that. Google Drive names every file on the
//! bridge by its Drive ID (`0AF5w1…`), and the title the user gave it is only
//! a GIO attribute. These helpers ask `gio` for the titles so a Drive folder
//! reads the way it does on the web. Google Docs, Sheets and Slides are
//! shortcuts there: no bytes to read, only a web address to open.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Where gvfsd-fuse mounts its backends for this user.
pub fn gvfs_root() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        // SAFETY: getuid has no failure mode and touches no memory.
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })))
        .join("gvfs")
}

/// Whether `path` is on a GVfs backend that names files by an opaque ID.
pub fn names_by_id(path: &Path) -> bool {
    names_by_id_under(path, &gvfs_root())
}

fn names_by_id_under(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .and_then(|rest| rest.components().next())
        .is_some_and(|c| c.as_os_str().to_string_lossy().starts_with("google-drive:"))
}

/// The title GIO knows a file by, and the web page a Google Docs-style
/// shortcut points at.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GioName {
    pub display: String,
    pub link: Option<String>,
}

const DISPLAY: &str = "standard::display-name=";
const TARGET: &str = "standard::target-uri=";

/// `gio list -h -a standard::display-name,standard::target-uri DIR` prints
/// `NAME\tSIZE\t(TYPE)\tstandard::display-name=… standard::target-uri=…`, one
/// line per child. Titles may hold spaces, so each value runs to the next
/// attribute we asked for rather than to the next space.
pub fn parse_gio_list(text: &str) -> HashMap<String, GioName> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let mut cols = line.splitn(4, '\t');
        let (Some(name), Some(_size), Some(_kind), Some(attrs)) = (cols.next(), cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        let Some(display) = attr_value(attrs, DISPLAY) else { continue };
        if display.is_empty() {
            continue;
        }
        out.insert(
            name.to_string(),
            GioName {
                display: display.to_string(),
                link: attr_value(attrs, TARGET).filter(|u| u.starts_with("http")).map(str::to_string),
            },
        );
    }
    out
}

fn attr_value<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    let start = attrs.find(key)? + key.len();
    let rest = &attrs[start..];
    let end = [DISPLAY, TARGET]
        .iter()
        .filter_map(|k| rest.find(&format!(" {k}")))
        .min()
        .unwrap_or(rest.len());
    Some(rest[..end].trim_end())
}

/// `gio info -a …` prints the attributes as `  standard::display-name: X`.
pub fn parse_gio_info(text: &str) -> Option<GioName> {
    let mut name = GioName::default();
    for line in text.lines() {
        let line = line.trim_start();
        if let Some(v) = line.strip_prefix("standard::display-name: ") {
            name.display = v.to_string();
        } else if let Some(v) = line.strip_prefix("standard::target-uri: ") {
            name.link = Some(v.to_string()).filter(|u| u.starts_with("http"));
        }
    }
    (!name.display.is_empty()).then_some(name)
}

/// Titles for every child of a Drive folder, keyed by the ID the bridge uses.
/// Empty when `gio` is missing or refuses: the IDs are still usable names.
pub fn display_names(dir: &Path) -> HashMap<String, GioName> {
    let attrs = "standard::display-name,standard::target-uri";
    gio(&["list".as_ref(), "-h".as_ref(), "-a".as_ref(), attrs.as_ref(), dir.as_os_str()], GIO_READ_TIMEOUT)
        .map(|out| parse_gio_list(&out))
        .unwrap_or_default()
}

/// The title of one file on a Drive mount.
pub fn display_name(path: &Path) -> Option<GioName> {
    let attrs = "standard::display-name,standard::target-uri";
    gio(&["info".as_ref(), "-a".as_ref(), attrs.as_ref(), path.as_os_str()], GIO_READ_TIMEOUT)
        .ok()
        .and_then(|out| parse_gio_info(&out))
}

/// What a listing or a file needs from GIO is answered from the daemon's
/// cache; a mount waiting on the network gets this long.
pub const GIO_READ_TIMEOUT: Duration = Duration::from_secs(15);

/// Runs `gio args` with no shell and nothing on stdin, killed after
/// `timeout`. Stdout on success; gio's own message on failure, since that is
/// what tells the user why a phone or a share refused.
pub fn gio<S: AsRef<std::ffi::OsStr>>(args: &[S], timeout: Duration) -> Result<String, String> {
    let mut child = Command::new("gio")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run gio: {e}"))?;
    // Drained on threads so a long listing never blocks on a full pipe.
    let (tx, rx) = mpsc::channel::<(bool, String)>();
    for (is_stdout, pipe) in [
        (true, child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>)),
        (false, child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>)),
    ] {
        if let Some(mut pipe) = pipe {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                let _ = tx.send((is_stdout, String::from_utf8_lossy(&buf).into_owned()));
            });
        }
    }
    drop(tx);
    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("gio gave no answer within {}s", timeout.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let (mut out, mut err) = (String::new(), String::new());
    while let Ok((is_stdout, text)) = rx.recv_timeout(Duration::from_secs(2)) {
        if is_stdout {
            out = text;
        } else {
            err = text;
        }
    }
    if status.success() {
        Ok(out)
    } else {
        let msg = err.trim().trim_start_matches("gio: ").to_string();
        Err(if msg.is_empty() { format!("gio exited with {status}") } else { msg })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_google_drive_mounts_name_files_by_id() {
        let root = Path::new("/run/user/1000/gvfs");
        assert!(names_by_id_under(Path::new("/run/user/1000/gvfs/google-drive:host=gmail.com,user=me/0AF5"), root));
        assert!(names_by_id_under(Path::new("/run/user/1000/gvfs/google-drive:host=gmail.com,user=me"), root));
        assert!(!names_by_id_under(Path::new("/run/user/1000/gvfs/mtp:host=Phone/DCIM"), root));
        assert!(!names_by_id_under(Path::new("/home/me/google-drive:x"), root));
        assert!(!names_by_id_under(root, root));
    }

    #[test]
    fn list_titles_keep_their_spaces_and_shortcuts_keep_their_page() {
        let text = "0AF5w1\t0\t(directory)\tstandard::display-name=My Drive\n\
                    1eu_wt\t0\t(shortcut)\tstandard::display-name=ARCHITECTURE.md standard::target-uri=https://docs.google.com/document/d/1eu_wt/edit\n\
                    1I8u\t61070\t(regular)\tstandard::display-name=error report (final).xlsx\n\
                    broken line\n";
        let names = parse_gio_list(text);
        assert_eq!(names["0AF5w1"], GioName { display: "My Drive".into(), link: None });
        assert_eq!(names["1eu_wt"].display, "ARCHITECTURE.md");
        assert_eq!(names["1eu_wt"].link.as_deref(), Some("https://docs.google.com/document/d/1eu_wt/edit"));
        assert_eq!(names["1I8u"].display, "error report (final).xlsx");
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn info_reads_the_title_and_ignores_non_web_targets() {
        let text = "display name: My Drive\nname: 0AF5\nattributes:\n  standard::display-name: My Drive\n  standard::target-uri: google-drive://me@gmail.com/0AF5\n";
        assert_eq!(parse_gio_info(text), Some(GioName { display: "My Drive".into(), link: None }));
        assert_eq!(parse_gio_info("attributes:\n"), None);
    }
}
