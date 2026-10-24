//! A Laravel project's own error log, read back for a failed run.
//!
//! `queue:work` prints only `... FAIL` for a failed job — never the reason.
//! Laravel reports the exception through its handler instead, which writes it
//! to `storage/logs/laravel.log` (or `laravel-YYYY-MM-DD.log` on the daily
//! channel), stamped with the same app-timezone clock the worker prints. So
//! the entries logged while the run was going are, in practice, its failure.
//!
//! Read-only, and only ever files named `laravel*.log` under the given
//! project's `storage/logs`. Only the tail is read, as for every log viewer.

use serde::Serialize;
use std::path::Path;

/// Enough for a few dozen errors with their stack traces.
const TAIL_LINES: usize = 6000;
/// Entries are cut here — a trace's first frames are the useful ones.
const MAX_ENTRY_LINES: usize = 120;
const MAX_ENTRIES: usize = 10;
const LEVELS: &[&str] = &["error", "critical", "alert", "emergency"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LaravelLogEntry {
    /// `YYYY-MM-DD HH:MM:SS`.
    pub at: String,
    pub level: String,
    /// The message without the JSON context that follows it.
    pub message: String,
    /// The whole entry as logged, stack trace included.
    pub text: String,
    /// Which file it came from, relative to the project.
    pub file: String,
}

/// `[2026-10-22 11:50:22] local.ERROR: msg` (Laravel's default) or
/// `[2026-10-22T11:50:22.123456+06:00] ...` (a custom formatter): the
/// timestamp as `YYYY-MM-DD HH:MM:SS`, the level, and the rest of the line.
fn entry_header(line: &str) -> Option<(String, String, &str)> {
    let inner = line.strip_prefix('[')?;
    let close = inner.find("] ")?;
    let stamp = inner.get(..19)?;
    let bytes = stamp.as_bytes();
    let shaped = bytes.len() == 19
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            10 => *b == b' ' || *b == b'T',
            13 | 16 => *b == b':',
            _ => b.is_ascii_digit(),
        });
    if !shaped || close < 19 {
        return None;
    }
    let rest = &inner[close + 2..];
    let (channel_level, message) = rest.split_once(": ")?;
    let level = channel_level.rsplit('.').next()?.to_ascii_lowercase();
    if level.is_empty() || !level.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    Some((stamp.replacen('T', " ", 1), level, message))
}

/// Error-level entries stamped within `[from, to]` (inclusive, both
/// `YYYY-MM-DD HH:MM:SS`, which compare correctly as strings).
pub fn errors_between(log: &str, file: &str, from: &str, to: &str) -> Vec<LaravelLogEntry> {
    let mut out: Vec<LaravelLogEntry> = Vec::new();
    let mut current: Option<(LaravelLogEntry, usize)> = None;

    let flush = |current: &mut Option<(LaravelLogEntry, usize)>, out: &mut Vec<LaravelLogEntry>| {
        if let Some((entry, _)) = current.take() {
            out.push(entry);
        }
    };

    for line in log.lines() {
        if let Some((at, level, message)) = entry_header(line) {
            flush(&mut current, &mut out);
            let wanted = LEVELS.contains(&level.as_str()) && at.as_str() >= from && at.as_str() <= to;
            if wanted {
                let message = message.split(" {\"").next().unwrap_or(message).trim().to_string();
                let entry = LaravelLogEntry { at, level, message, text: line.to_string(), file: file.to_string() };
                current = Some((entry, 1));
            }
            continue;
        }
        if let Some((entry, lines)) = current.as_mut() {
            if *lines < MAX_ENTRY_LINES {
                entry.text.push('\n');
                entry.text.push_str(line);
            } else if *lines == MAX_ENTRY_LINES {
                entry.text.push_str("\n... (trace cut, see the log file)");
            }
            *lines += 1;
        }
    }
    flush(&mut current, &mut out);
    out
}

/// The log files a run in `[from, to]` could have written to: the single
/// channel's file and the daily channel's file for each day in the window.
fn candidates(from: &str, to: &str) -> Vec<String> {
    let mut names = vec!["laravel.log".to_string()];
    for day in [from.get(..10), to.get(..10)].into_iter().flatten() {
        let name = format!("laravel-{day}.log");
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

#[tauri::command(async)]
pub fn laravel_log_errors(working_dir: String, from: String, to: String) -> Result<Vec<LaravelLogEntry>, String> {
    let logs = Path::new(&working_dir).join("storage").join("logs");
    if !logs.is_dir() {
        return Err(format!("no storage/logs folder in {working_dir} — is it a Laravel project?"));
    }
    let mut entries = Vec::new();
    for name in candidates(&from, &to) {
        let path = logs.join(&name);
        if !path.is_file() {
            continue;
        }
        let text = crate::modules::tail::tail_file(&path, TAIL_LINES).map_err(|e| format!("failed to read {name}: {e}"))?;
        entries.extend(errors_between(&text, &format!("storage/logs/{name}"), &from, &to));
    }
    entries.sort_by(|a, b| a.at.cmp(&b.at));
    let skip = entries.len().saturating_sub(MAX_ENTRIES);
    Ok(entries.into_iter().skip(skip).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = r#"[2026-10-22 17:28:44] local.INFO: LabJob started {"id":225}
[2026-10-22 17:28:45] local.ERROR: Job #225 failed on purpose ("should fail" was ticked). {"exception":"[object] (RuntimeException(code: 0): Job #225 failed on purpose at C:/app/LabJob.php:53)
[stacktrace]
#0 C:/vendor/Illuminate/Container/BoundMethod.php(36): App\\Jobs\\LabJob->handle()
#1 {main}
"}
[2026-10-22 17:28:47] production.CRITICAL: Connection could not be established with host "127.0.0.1:1099"
[2026-10-22T17:29:10.123456+06:00] local.ERROR: later
"#;

    #[test]
    fn keeps_error_entries_in_the_window_with_their_trace() {
        let got = errors_between(LOG, "storage/logs/laravel.log", "2026-10-22 17:28:44", "2026-10-22 17:28:47");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].at, "2026-10-22 17:28:45");
        assert_eq!(got[0].level, "error");
        assert_eq!(got[0].message, r#"Job #225 failed on purpose ("should fail" was ticked)."#);
        assert!(got[0].text.contains("#0 C:/vendor"));
        assert!(!got[0].text.contains("CRITICAL"));
        assert_eq!(got[1].level, "critical");
        assert_eq!(got[1].message, r#"Connection could not be established with host "127.0.0.1:1099""#);
    }

    #[test]
    fn reads_iso_stamps_and_skips_info() {
        let got = errors_between(LOG, "f", "2026-10-22 17:29:00", "2026-10-22 17:30:00");
        assert_eq!(got.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), vec!["later"]);
        assert!(errors_between(LOG, "f", "2026-10-22 17:28:44", "2026-10-22 17:28:44").is_empty());
    }

    #[test]
    fn long_traces_are_cut() {
        let mut log = String::from("[2026-10-22 10:00:00] local.ERROR: boom\n");
        for i in 0..500 {
            log.push_str(&format!("#{i} frame\n"));
        }
        let got = errors_between(&log, "f", "2026-10-22 10:00:00", "2026-10-22 10:00:00");
        assert_eq!(got[0].text.lines().count(), MAX_ENTRY_LINES + 1);
        assert!(got[0].text.ends_with("(trace cut, see the log file)"));
    }

    #[test]
    fn looks_in_the_single_and_daily_files() {
        assert_eq!(candidates("2026-10-22 23:59:59", "2026-10-23 00:00:02"), vec![
            "laravel.log",
            "laravel-2026-10-22.log",
            "laravel-2026-10-23.log"
        ]);
        assert_eq!(candidates("2026-10-22 10:00:00", "2026-10-22 10:00:05").len(), 2);
    }

    #[test]
    fn headers_need_the_bracketed_timestamp() {
        assert!(entry_header("#0 C:/x.php(1): y").is_none());
        assert!(entry_header("[stacktrace]").is_none());
        assert!(entry_header("[2026-10-22 10:00:00] local.ERROR: m").is_some());
    }
}
