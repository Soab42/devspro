//! Hosts file — a structured editor for /etc/hosts (on Windows,
//! `%SystemRoot%\System32\drivers\etc\hosts`).
//!
//! The file is parsed line by line into `Line`s that each keep their original
//! text, so writing back a file nothing was changed in is byte-identical: comments,
//! tabs, odd spacing and blank lines all survive. Only the lines this module adds
//! or edits are ever re-rendered.
//!
//! All mutation is pure (`HostsFile::add/remove/update`) and unit-tested; the
//! commands wrap it in read → mutate → hand the full new contents to the
//! elevated helper. The core never writes the hosts file itself.
//!
//! Windows hosts files use CRLF. Every line keeps its own `\r` in `raw`, and the
//! lines this module adds or rewrites get one too when the file uses CRLF, so an
//! edit never leaves a file with mixed line endings.
//!
//! Two kinds of entry count as "managed" (created by this app, safe to edit
//! freely): lines tagged with the `# devspro` marker comment, which this module
//! writes, and lines inside the nginx module's `>>> devspro` sync block. The
//! latter are regenerated on every site change, so editing them here would be
//! undone — the UI shows them as DevsPro-owned for that reason.

use crate::db::Db;
use crate::elevate::{self, Operation, OperationManifest};
use serde::Serialize;
use std::net::IpAddr;
use std::path::PathBuf;
use tauri::{AppHandle, State};

/// Where the system hosts file lives on this OS.
#[cfg(unix)]
pub fn hosts_path() -> PathBuf {
    PathBuf::from("/etc/hosts")
}

/// Where the system hosts file lives on this OS. `%SystemRoot%` is normally
/// `C:\Windows`, which is also the fallback if it is somehow unset.
#[cfg(windows)]
pub fn hosts_path() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(root).join(r"System32\drivers\etc\hosts")
}

/// Trailing comment that tags an entry as written by this module.
pub const MARKER: &str = "devspro";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub ip: String,
    pub hostnames: Vec<String>,
    /// Text after `#`, trimmed, without the `#`.
    pub comment: Option<String>,
}

/// One physical line. `raw` is the exact original text (no newline) and is what
/// gets written back unless the line is replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub raw: String,
    pub entry: Option<Entry>,
    /// Sits inside the nginx module's marker-delimited sync block.
    pub in_sync_block: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostsFile {
    pub lines: Vec<Line>,
    /// Whether the original ended with `\n`. Preserved so a round-trip of a file
    /// without one stays identical.
    pub trailing_newline: bool,
    /// Lines end in `\r\n` (the Windows convention). New and rewritten lines
    /// follow suit.
    pub crlf: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntryView {
    /// 1-based, as editors number them.
    pub line_no: usize,
    pub ip: String,
    pub hostnames: Vec<String>,
    pub comment: Option<String>,
    pub managed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostsView {
    pub entries: Vec<EntryView>,
    pub raw: String,
    /// The hosts file's location on this OS, for display.
    pub path: String,
}

// ---------------------------------------------------------------------------
// Parsing and serialising
// ---------------------------------------------------------------------------

fn parse_entry(line: &str) -> Option<Entry> {
    let (data, comment) = match line.find('#') {
        Some(i) => (&line[..i], Some(line[i + 1..].trim().to_string())),
        None => (line, None),
    };
    let mut parts = data.split_whitespace();
    let ip = parts.next()?.to_string();
    let hostnames: Vec<String> = parts.map(str::to_string).collect();
    if hostnames.is_empty() {
        // A bare token with no hostnames is not an entry; keep it verbatim as-is.
        return None;
    }
    Some(Entry { ip, hostnames, comment })
}

pub fn parse(contents: &str) -> HostsFile {
    let trailing_newline = contents.ends_with('\n');
    let crlf = contents.contains("\r\n");
    let body = contents.strip_suffix('\n').unwrap_or(contents);
    let mut in_sync_block = false;
    let lines = if contents.is_empty() {
        Vec::new()
    } else {
        body.split('\n')
            .map(|raw| {
                let trimmed = raw.trim();
                if trimmed == super::nginx_ssl::hosts::MARKER_BEGIN {
                    in_sync_block = true;
                }
                let line = Line {
                    raw: raw.to_string(),
                    entry: parse_entry(raw),
                    in_sync_block,
                };
                if trimmed == super::nginx_ssl::hosts::MARKER_END {
                    in_sync_block = false;
                }
                line
            })
            .collect()
    };
    HostsFile { lines, trailing_newline, crlf }
}

pub fn render(file: &HostsFile) -> String {
    let mut out = file
        .lines
        .iter()
        .map(|l| l.raw.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if file.trailing_newline {
        out.push('\n');
    }
    out
}

fn render_entry(entry: &Entry) -> String {
    let mut raw = format!("{}\t{}", entry.ip, entry.hostnames.join(" "));
    if let Some(c) = &entry.comment {
        raw.push_str(" # ");
        raw.push_str(c);
    }
    raw
}

fn is_managed(line: &Line) -> bool {
    line.in_sync_block
        || line
            .entry
            .as_ref()
            .and_then(|e| e.comment.as_deref())
            .map(|c| c == MARKER)
            .unwrap_or(false)
}

impl HostsFile {
    pub fn view(&self) -> HostsView {
        let entries = self
            .lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| {
                let e = line.entry.as_ref()?;
                Some(EntryView {
                    line_no: i + 1,
                    ip: e.ip.clone(),
                    hostnames: e.hostnames.clone(),
                    comment: e.comment.clone(),
                    managed: is_managed(line),
                })
            })
            .collect();
        HostsView {
            entries,
            raw: render(self),
            path: hosts_path().display().to_string(),
        }
    }

    /// Hostnames that resolve to an IPv4 loopback address through this file,
    /// lower-cased for comparison.
    pub fn loopback_hostnames(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter_map(|l| l.entry.as_ref())
            .filter(|e| matches!(e.ip.parse::<IpAddr>(), Ok(IpAddr::V4(v4)) if v4.is_loopback()))
            .flat_map(|e| e.hostnames.iter().map(|h| h.to_ascii_lowercase()))
            .collect()
    }

    fn line_mut(&mut self, line_no: usize) -> Result<&mut Line, String> {
        let line = line_no
            .checked_sub(1)
            .and_then(|i| self.lines.get_mut(i))
            .ok_or_else(|| format!("line {line_no} does not exist"))?;
        if line.entry.is_none() {
            return Err(format!("line {line_no} is not a hosts entry"));
        }
        Ok(line)
    }

    /// Appends a `# devspro`-tagged entry at the end of the file. Refuses a
    /// hostname that already resolves through any line — silently adding a second
    /// mapping would make "which one wins" depend on resolver order.
    pub fn add(&mut self, ip: &str, hostname: &str) -> Result<(), String> {
        validate_ip(ip)?;
        validate_hostname(hostname)?;
        if let Some(existing) = self.lines.iter().filter_map(|l| l.entry.as_ref()).find(|e| {
            e.hostnames.iter().any(|h| h.eq_ignore_ascii_case(hostname))
        }) {
            return Err(format!("{hostname} is already mapped to {}", existing.ip));
        }
        let entry = Entry {
            ip: ip.to_string(),
            hostnames: vec![hostname.to_string()],
            comment: Some(MARKER.to_string()),
        };
        // A CRLF file that lacked a final newline: its last line gets the `\r` it
        // would have had, since a newline now follows it.
        if self.crlf && !self.trailing_newline {
            if let Some(last) = self.lines.last_mut() {
                last.raw.push('\r');
            }
        }
        let mut raw = render_entry(&entry);
        if self.crlf {
            raw.push('\r');
        }
        self.lines.push(Line {
            raw,
            entry: Some(entry),
            in_sync_block: false,
        });
        // A file that lacked a final newline gets one now — appending a line to
        // such a file without it would glue the two last lines together.
        self.trailing_newline = true;
        Ok(())
    }

    pub fn remove(&mut self, line_no: usize) -> Result<(), String> {
        let line = self.line_mut(line_no)?;
        if mentions_localhost(line) {
            return Err("refusing to remove a line that defines localhost".to_string());
        }
        self.lines.remove(line_no - 1);
        Ok(())
    }

    /// Replaces the IP and hostnames of an existing entry, keeping its comment
    /// (and so its managed marker) and its position.
    pub fn update(&mut self, line_no: usize, ip: &str, hostnames: &[String]) -> Result<(), String> {
        validate_ip(ip)?;
        if hostnames.is_empty() {
            return Err("at least one hostname is required".to_string());
        }
        for h in hostnames {
            validate_hostname(h)?;
        }
        let line = self.line_mut(line_no)?;
        let keeps_localhost = hostnames.iter().any(|h| h.eq_ignore_ascii_case("localhost"));
        if mentions_localhost(line) && !keeps_localhost {
            return Err("refusing to drop localhost from this line".to_string());
        }
        let comment = line.entry.as_ref().and_then(|e| e.comment.clone());
        let entry = Entry {
            ip: ip.to_string(),
            hostnames: hostnames.to_vec(),
            comment,
        };
        let had_cr = line.raw.ends_with('\r');
        line.raw = render_entry(&entry);
        if had_cr {
            line.raw.push('\r');
        }
        line.entry = Some(entry);
        Ok(())
    }
}

fn mentions_localhost(line: &Line) -> bool {
    line.entry
        .as_ref()
        .map(|e| e.hostnames.iter().any(|h| h.to_ascii_lowercase().contains("localhost")))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

pub fn validate_ip(ip: &str) -> Result<(), String> {
    ip.parse::<IpAddr>()
        .map(|_| ())
        .map_err(|_| format!("{ip:?} is not a valid IPv4 or IPv6 address"))
}

/// RFC 1123 labels, with `_` also allowed: it is invalid in DNS but common in local
/// hosts files and the resolver does not object.
pub fn validate_hostname(hostname: &str) -> Result<(), String> {
    if hostname.is_empty() || hostname.len() > 253 {
        return Err("hostname must be between 1 and 253 characters".to_string());
    }
    for label in hostname.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(format!("{hostname:?}: each dot-separated label must be 1 to 63 characters"));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(format!("{hostname:?}: a label cannot start or end with '-'"));
        }
        if !label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(format!(
                "{hostname:?}: only letters, digits, '-', '_' and '.' are allowed"
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn read_file() -> Result<HostsFile, String> {
    let path = hosts_path();
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    Ok(parse(&raw))
}

/// Hands the full new contents to the helper. Nothing here retries or rolls
/// back: the helper's write is atomic (temp file + rename), so a cancelled
/// password prompt leaves the file untouched and the error is shown as-is.
fn write_file(app: &AppHandle, file: &HostsFile) -> Result<(), String> {
    let manifest = OperationManifest {
        allowed_write_prefix: String::new(),
        operations: vec![Operation::WriteHostsFile { contents: render(file) }],
    };
    let results = elevate::run(app, manifest)?;
    match results.first() {
        Some(r) if r.success => Ok(()),
        Some(r) => Err(r.message.clone()),
        None => Err("helper returned no results".to_string()),
    }
}

fn mutate(app: &AppHandle, f: impl FnOnce(&mut HostsFile) -> Result<(), String>) -> Result<HostsView, String> {
    let mut file = read_file()?;
    f(&mut file)?;
    write_file(app, &file)?;
    Ok(read_file()?.view())
}

#[tauri::command(async)]
pub fn hosts_file_read() -> Result<HostsView, String> {
    Ok(read_file()?.view())
}

#[tauri::command(async)]
pub fn hosts_file_add(app: AppHandle, ip: String, hostname: String) -> Result<HostsView, String> {
    mutate(&app, |f| f.add(ip.trim(), hostname.trim()))
}

#[tauri::command(async)]
pub fn hosts_file_remove(app: AppHandle, line_no: usize) -> Result<HostsView, String> {
    mutate(&app, |f| f.remove(line_no))
}

#[tauri::command(async)]
pub fn hosts_file_update(
    app: AppHandle,
    line_no: usize,
    ip: String,
    hostnames: Vec<String>,
) -> Result<HostsView, String> {
    let hostnames: Vec<String> = hostnames
        .iter()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .collect();
    mutate(&app, |f| f.update(line_no, ip.trim(), &hostnames))
}

/// Enabled nginx site domains with no loopback entry in the hosts file. Disabled
/// sites are skipped: nginx does not serve them, so an entry would only point
/// the browser at a connection refused.
#[tauri::command(async)]
pub fn hosts_file_missing_sites(db: State<Db>) -> Result<Vec<String>, String> {
    let sites: Vec<String> = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT hostname FROM nginx_ssl_sites WHERE enabled = 1 ORDER BY hostname")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?
    };
    let present = read_file()?.loopback_hostnames();
    Ok(sites
        .into_iter()
        .filter(|s| !present.contains(&s.to_ascii_lowercase()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# See `man hosts`\n\n127.0.0.1\tlocalhost\n::1\t\tlocalhost ip6-localhost # v6\n\n10.0.0.5   box.lan  box   # office\n192.168.1.9 nas.home # devspro\n";

    #[test]
    fn round_trips_byte_identical() {
        assert_eq!(render(&parse(SAMPLE)), SAMPLE);
        let no_newline = "127.0.0.1 localhost";
        assert_eq!(render(&parse(no_newline)), no_newline);
        assert_eq!(render(&parse("")), "");
        let crlf = "127.0.0.1 localhost\r\n\r\n# c\r\n";
        assert_eq!(render(&parse(crlf)), crlf);
    }

    /// The stock Windows hosts file: CRLF, and localhost only in comments.
    const WINDOWS_SAMPLE: &str = "# Copyright (c) 1993-2009 Microsoft Corp.\r\n#\r\n# localhost name resolution is handled within DNS itself.\r\n#\t127.0.0.1       localhost\r\n#\t::1             localhost\r\n";

    #[test]
    fn crlf_files_stay_crlf_when_edited() {
        let mut f = parse(WINDOWS_SAMPLE);
        assert!(f.crlf);
        assert!(f.view().entries.is_empty());
        f.add("127.0.0.1", "app.test").unwrap();
        assert_eq!(
            render(&f),
            format!("{WINDOWS_SAMPLE}127.0.0.1\tapp.test # devspro\r\n")
        );
        f.update(6, "127.0.0.1", &["app.test".into(), "www.app.test".into()]).unwrap();
        assert_eq!(f.lines[5].raw, "127.0.0.1\tapp.test www.app.test # devspro\r");
        assert_eq!(f.view().entries[0].comment.as_deref(), Some("devspro"));
        f.remove(6).unwrap();
        assert_eq!(render(&f), WINDOWS_SAMPLE);

        // No final newline: the old last line gets its CRLF when one is appended.
        let mut f = parse("# a\r\n10.0.0.1 box");
        f.add("10.0.0.2", "nas").unwrap();
        assert_eq!(render(&f), "# a\r\n10.0.0.1 box\r\n10.0.0.2\tnas # devspro\r\n");
    }

    #[test]
    fn parses_entries_comments_tabs_and_ipv6() {
        let view = parse(SAMPLE).view();
        let lines: Vec<usize> = view.entries.iter().map(|e| e.line_no).collect();
        assert_eq!(lines, vec![3, 4, 6, 7]);

        let v6 = &view.entries[1];
        assert_eq!(v6.ip, "::1");
        assert_eq!(v6.hostnames, vec!["localhost", "ip6-localhost"]);
        assert_eq!(v6.comment.as_deref(), Some("v6"));
        assert!(!v6.managed);

        let office = &view.entries[2];
        assert_eq!(office.hostnames, vec!["box.lan", "box"]);
        assert_eq!(office.comment.as_deref(), Some("office"));

        assert!(view.entries[3].managed);
        assert_eq!(view.entries[0].comment, None);
    }

    #[test]
    fn sync_block_lines_are_managed() {
        let text = format!(
            "127.0.0.1 localhost\n{}\n127.0.0.1 app.test\n{}\n127.0.0.1 other.test\n",
            super::super::nginx_ssl::hosts::MARKER_BEGIN,
            super::super::nginx_ssl::hosts::MARKER_END
        );
        let view = parse(&text).view();
        assert_eq!(view.entries.len(), 3);
        assert!(!view.entries[0].managed);
        assert!(view.entries[1].managed);
        assert!(!view.entries[2].managed);
    }

    #[test]
    fn add_appends_marked_entry_and_rejects_duplicates() {
        let mut f = parse("127.0.0.1 localhost");
        f.add("127.0.0.1", "app.test").unwrap();
        assert_eq!(render(&f), "127.0.0.1 localhost\n127.0.0.1\tapp.test # devspro\n");
        assert!(f.add("10.0.0.1", "APP.test").unwrap_err().contains("already mapped"));
        assert!(f.add("nope", "x.test").is_err());
        assert!(f.add("::1", "bad host").is_err());
        assert!(f.add("::1", "-bad.test").is_err());
        f.add("::1", "v6.test").unwrap();
        assert_eq!(f.view().entries.last().unwrap().ip, "::1");
    }

    #[test]
    fn remove_protects_localhost() {
        let mut f = parse(SAMPLE);
        assert!(f.remove(3).unwrap_err().contains("localhost"));
        assert!(f.remove(4).is_err());
        assert!(f.remove(1).is_err()); // a comment
        assert!(f.remove(99).is_err());
        f.remove(6).unwrap();
        assert_eq!(
            render(&f),
            "# See `man hosts`\n\n127.0.0.1\tlocalhost\n::1\t\tlocalhost ip6-localhost # v6\n\n192.168.1.9 nas.home # devspro\n"
        );
    }

    #[test]
    fn update_keeps_comment_and_position() {
        let mut f = parse(SAMPLE);
        f.update(7, "192.168.1.10", &["nas.home".into(), "nas".into()]).unwrap();
        assert_eq!(f.lines[6].raw, "192.168.1.10\tnas.home nas # devspro");
        assert!(f.view().entries[3].managed);

        f.update(6, "10.0.0.6", &["box.lan".into()]).unwrap();
        assert_eq!(f.lines[5].raw, "10.0.0.6\tbox.lan # office");

        assert!(f.update(3, "127.0.0.1", &["other".into()]).is_err());
        f.update(3, "127.0.0.1", &["localhost".into(), "me".into()]).unwrap();
        assert!(f.update(7, "1.2.3.4", &[]).is_err());
    }

    #[test]
    fn loopback_hostnames_ignore_non_loopback() {
        let f = parse("127.0.0.1 A.test\n127.0.1.1 b.test\n::1 c.test\n10.0.0.1 d.test\n");
        assert_eq!(f.loopback_hostnames(), vec!["a.test", "b.test"]);
    }
}
