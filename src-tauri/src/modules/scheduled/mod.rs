//! Scheduled tasks — recurring commands the user defines here and hands to
//! cron. The canonical case is `php artisan schedule:run` every minute for a
//! Laravel site, which Laravel's own docs tell you to put in your crontab by
//! hand; this module owns that entry so it can be edited, paused, and its
//! output read back from inside the app.
//!
//! Division of labour: the *definitions* live in SQLite (like workers), the
//! *scheduling* is delegated to the user's own crontab, and this module only
//! ever rewrites one clearly delimited block inside it (`BLOCK_START` ..
//! `BLOCK_END`). Everything outside the block is written back byte-for-byte —
//! people keep hand-written entries in their crontab and losing one would be
//! unforgivable. Cron, not an in-app timer, so tasks keep running when DevsPro
//! isn't open, which is the whole point of a schedule.
//!
//! Nothing here touches the crontab on its own initiative: the sync only runs
//! from the save / delete / toggle commands, i.e. from an explicit click.
//!
//! No elevation: the user's crontab is the user's own file, `crontab -` edits
//! it without root.

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeDelta, Timelike};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tauri::{AppHandle, Manager, State};

/// Appended to `db::later_migrations()`; never reordered.
pub const MIGRATION_SCHEDULED_TASKS: &str = r#"
    CREATE TABLE scheduled_tasks (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        schedule TEXT NOT NULL,
        command TEXT NOT NULL,
        working_dir TEXT NOT NULL,
        site_id TEXT,
        enabled INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL
    );
"#;

pub const BLOCK_START: &str = "# >>> devspro managed — edit in DevsPro >>>";
pub const BLOCK_END: &str = "# <<< devspro managed <<<";

/// How many trailing log lines ride along with the list response. Enough to
/// see "what happened last time" at a glance; the logs modal has the rest.
const TAIL_PREVIEW_LINES: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub command: String,
    pub working_dir: String,
    pub site_id: Option<String>,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScheduledTaskView {
    #[serde(flatten)]
    pub task: ScheduledTask,
    /// Human-readable version of `schedule`, e.g. "Every 5 minutes".
    pub description: String,
    /// RFC 3339 local time of the next cron fire, or null if the expression
    /// never matches within a year (e.g. 31 February).
    pub next_run: Option<String>,
    /// Derived from the log file's mtime — cron itself keeps no per-job history.
    pub last_run_at: Option<String>,
    pub last_output_tail: String,
}

// ---------------------------------------------------------------------------
// Cron expressions
// ---------------------------------------------------------------------------

/// One parsed 5-field expression. Each field is the set of values it matches,
/// plus whether it was written as `*` — that distinction matters for the
/// day-of-month / day-of-week rule below and for the description.
#[derive(Debug, Clone, PartialEq)]
pub struct CronSpec {
    pub minute: CronField,
    pub hour: CronField,
    pub dom: CronField,
    pub month: CronField,
    pub dow: CronField,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CronField {
    pub values: Vec<u8>,
    pub any: bool,
}

impl CronField {
    fn matches(&self, v: u8) -> bool {
        self.any || self.values.contains(&v)
    }
}

const MONTH_NAMES: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const DAY_NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
const MONTH_LONG: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];
const DAY_LONG: [&str; 7] = [
    "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday",
];

fn parse_value(raw: &str, names: Option<&[&str]>, label: &str) -> Result<u8, String> {
    if let Ok(n) = raw.parse::<u8>() {
        return Ok(n);
    }
    if let Some(names) = names {
        let lower = raw.to_ascii_lowercase();
        if let Some(i) = names.iter().position(|n| *n == lower) {
            // Names are 1-based for months, 0-based for weekdays — mirroring
            // the numeric ranges cron itself uses.
            return Ok(if names.len() == 12 { i as u8 + 1 } else { i as u8 });
        }
    }
    Err(format!("{label}: '{raw}' is not a number"))
}

fn parse_field(raw: &str, min: u8, max: u8, names: Option<&[&str]>, label: &str) -> Result<CronField, String> {
    if raw.is_empty() {
        return Err(format!("{label}: field is empty"));
    }
    let mut values = Vec::new();
    let mut any = false;
    for part in raw.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => {
                let step = s
                    .parse::<u8>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("{label}: step '/{s}' must be a positive number"))?;
                (r, step)
            }
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (parse_value(a, names, label)?, parse_value(b, names, label)?)
        } else {
            let v = parse_value(range, names, label)?;
            // `5/10` means "starting at 5, every 10" — an extension most crons
            // accept; a bare value with no step is just itself.
            if step > 1 { (v, max) } else { (v, v) }
        };
        if lo > hi {
            return Err(format!("{label}: range {lo}-{hi} is backwards"));
        }
        if lo < min || hi > max {
            return Err(format!("{label}: values must be between {min} and {max}"));
        }
        if range == "*" && step == 1 {
            any = true;
        }
        let mut v = lo;
        while v <= hi {
            values.push(v);
            v = match v.checked_add(step) {
                Some(n) => n,
                None => break,
            };
        }
    }
    values.sort_unstable();
    values.dedup();
    Ok(CronField { values, any })
}

pub fn parse_cron(expr: &str) -> Result<CronSpec, String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!(
            "a schedule needs 5 fields (minute hour day month weekday), got {}",
            fields.len()
        ));
    }
    let minute = parse_field(fields[0], 0, 59, None, "minute")?;
    let hour = parse_field(fields[1], 0, 23, None, "hour")?;
    let dom = parse_field(fields[2], 1, 31, None, "day of month")?;
    let month = parse_field(fields[3], 1, 12, Some(&MONTH_NAMES), "month")?;
    let mut dow = parse_field(fields[4], 0, 7, Some(&DAY_NAMES), "weekday")?;
    // Both 0 and 7 are Sunday in cron; normalise so matching is one lookup.
    if dow.values.contains(&7) {
        dow.values.retain(|d| *d != 7);
        if !dow.values.contains(&0) {
            dow.values.insert(0, 0);
        }
    }
    Ok(CronSpec { minute, hour, dom, month, dow })
}

/// Vixie-cron day rule: when both day-of-month and day-of-week are restricted,
/// a day matches if *either* does; when only one is, that one decides.
fn day_matches(spec: &CronSpec, date: NaiveDate) -> bool {
    let dom_ok = spec.dom.matches(date.day() as u8);
    let dow_ok = spec.dow.matches(date.weekday().num_days_from_sunday() as u8);
    match (spec.dom.any, spec.dow.any) {
        (true, true) => true,
        (false, true) => dom_ok,
        (true, false) => dow_ok,
        (false, false) => dom_ok || dow_ok,
    }
}

/// First minute strictly after `after` that the spec fires on, searching at
/// most 366 days ahead. Walks minute by minute but skips whole hours, days and
/// months that can't match so a yearly job still resolves instantly.
pub fn next_run(spec: &CronSpec, after: NaiveDateTime) -> Option<NaiveDateTime> {
    let minute = TimeDelta::minutes(1);
    let start = after.with_second(0)?.with_nanosecond(0)? + minute;
    let limit = start + TimeDelta::days(366);
    let mut t = start;
    while t < limit {
        if !spec.month.matches(t.month() as u8) {
            // Jump to the first minute of the next month.
            let (y, m) = if t.month() == 12 { (t.year() + 1, 1) } else { (t.year(), t.month() + 1) };
            t = NaiveDate::from_ymd_opt(y, m, 1)?.and_hms_opt(0, 0, 0)?;
            continue;
        }
        if !day_matches(spec, t.date()) {
            t = (t.date() + TimeDelta::days(1)).and_hms_opt(0, 0, 0)?;
            continue;
        }
        if !spec.hour.matches(t.hour() as u8) {
            t = t.with_minute(0)? + TimeDelta::hours(1);
            continue;
        }
        if spec.minute.matches(t.minute() as u8) {
            return Some(t);
        }
        t += minute;
    }
    None
}

fn join_names(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        n => format!("{} and {}", items[..n - 1].join(", "), items[n - 1]),
    }
}

/// Detects an evenly-stepped field (`*/5`, `0-59/15`) starting at the field's
/// minimum; returns the step. `*` itself is step 1 and handled by `any`.
fn even_step(field: &CronField, min: u8, max: u8) -> Option<u8> {
    if field.any || field.values.len() < 2 || field.values[0] != min {
        return None;
    }
    let step = field.values[1] - field.values[0];
    let expected: Vec<u8> = (min..=max).step_by(step as usize).collect();
    (expected == field.values).then_some(step)
}

fn hhmm(hour: u8, minute: u8) -> String {
    format!("{hour:02}:{minute:02}")
}

fn day_suffix(spec: &CronSpec) -> String {
    let dows: Vec<String> = spec.dow.values.iter().map(|d| DAY_LONG[*d as usize].to_string()).collect();
    let doms: Vec<String> = spec.dom.values.iter().map(|d| d.to_string()).collect();
    let months: Vec<String> = spec.month.values.iter().map(|m| MONTH_LONG[*m as usize - 1].to_string()).collect();
    let dow_part = if spec.dow.any {
        None
    } else if spec.dow.values == [1, 2, 3, 4, 5] {
        Some("on weekdays".to_string())
    } else if spec.dow.values == [0, 6] {
        Some("on weekends".to_string())
    } else {
        Some(format!("on {}", join_names(&dows)))
    };
    let dom_part = (!spec.dom.any).then(|| format!("on day {} of the month", join_names(&doms)));
    let month_part = (!spec.month.any).then(|| format!("in {}", join_names(&months)));

    let mut parts = Vec::new();
    match (dom_part, dow_part) {
        (None, None) => parts.push("every day".to_string()),
        (Some(d), None) => parts.push(d),
        (None, Some(w)) => parts.push(w),
        (Some(d), Some(w)) => parts.push(format!("{d} or {w}")),
    }
    if let Some(m) = month_part {
        parts.push(m);
    }
    parts.join(" ")
}

/// English rendering of a spec. Optimised for the shapes the presets produce
/// ("Every 5 minutes", "At 03:00 every day") with an honest generic fallback
/// for anything more baroque.
pub fn describe(spec: &CronSpec) -> String {
    let days_are_any = spec.dom.any && spec.dow.any && spec.month.any;
    let single_minute = (!spec.minute.any && spec.minute.values.len() == 1).then(|| spec.minute.values[0]);
    let single_hour = (!spec.hour.any && spec.hour.values.len() == 1).then(|| spec.hour.values[0]);

    if spec.hour.any {
        if spec.minute.any {
            return if days_are_any {
                "Every minute".to_string()
            } else {
                format!("Every minute {}", day_suffix(spec))
            };
        }
        if let Some(step) = even_step(&spec.minute, 0, 59) {
            let base = format!("Every {step} minutes");
            return if days_are_any { base } else { format!("{base} {}", day_suffix(spec)) };
        }
        if let Some(m) = single_minute {
            let base = if m == 0 { "Every hour".to_string() } else { format!("Every hour at :{m:02}") };
            return if days_are_any { base } else { format!("{base} {}", day_suffix(spec)) };
        }
    }

    if let (Some(m), Some(step)) = (single_minute, even_step(&spec.hour, 0, 23)) {
        let base = if m == 0 {
            format!("Every {step} hours")
        } else {
            format!("Every {step} hours at :{m:02}")
        };
        return if days_are_any { base } else { format!("{base} {}", day_suffix(spec)) };
    }

    if let (Some(m), Some(h)) = (single_minute, single_hour) {
        return format!("At {} {}", hhmm(h, m), day_suffix(spec));
    }

    if let Some(m) = single_minute {
        if !spec.hour.any {
            let times: Vec<String> = spec.hour.values.iter().map(|h| hhmm(*h, m)).collect();
            return format!("At {} {}", join_names(&times), day_suffix(spec));
        }
    }

    // Generic fallback: spell every restricted field out.
    let minutes: Vec<String> = spec.minute.values.iter().map(|v| v.to_string()).collect();
    let hours: Vec<String> = spec.hour.values.iter().map(|v| v.to_string()).collect();
    let minute_part = if spec.minute.any {
        "Every minute".to_string()
    } else {
        format!("At minute {}", join_names(&minutes))
    };
    let hour_part = if spec.hour.any {
        String::new()
    } else {
        format!(" past hour {}", join_names(&hours))
    };
    format!("{minute_part}{hour_part} {}", day_suffix(spec))
}

// ---------------------------------------------------------------------------
// Crontab text
// ---------------------------------------------------------------------------

/// Single-quotes a path for `sh`, so spaces and `$` in a project folder
/// survive. A literal `'` becomes `'\''` — the only escape sh needs.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// One crontab line for a task. `%` is special in crontab command text (it
/// means newline, and everything after it becomes stdin), so it is escaped —
/// otherwise `date +%F` in a command would silently truncate the job.
pub fn cron_line(task: &ScheduledTask, log_path: &str) -> String {
    let command = format!(
        "cd {} && {} >> {} 2>&1",
        shell_quote(&task.working_dir),
        task.command,
        shell_quote(log_path)
    );
    format!("{} {}", task.schedule.split_whitespace().collect::<Vec<_>>().join(" "), command.replace('%', r"\%"))
}

/// The full managed block, markers included, or `None` when nothing is
/// enabled — in which case the block is removed from the crontab entirely
/// rather than left as an empty pair of markers.
pub fn render_block(tasks: &[ScheduledTask], log_dir: &str) -> Option<String> {
    let enabled: Vec<&ScheduledTask> = tasks.iter().filter(|t| t.enabled).collect();
    if enabled.is_empty() {
        return None;
    }
    let mut out = String::new();
    out.push_str(BLOCK_START);
    out.push('\n');
    for task in enabled {
        // Name as a comment above each line so the raw crontab stays readable
        // for someone who opens it with `crontab -l` — one line per task, and
        // a name with a newline in it can't sneak a second entry in.
        let name: String = task.name.chars().filter(|c| !c.is_control()).collect();
        out.push_str(&format!("# {name}\n"));
        out.push_str(&cron_line(task, &format!("{log_dir}/{}.log", task.id)));
        out.push('\n');
    }
    out.push_str(BLOCK_END);
    out.push('\n');
    Some(out)
}

/// Splices `block` into an existing crontab, replacing a previous managed
/// block in place (so it keeps its position) or appending one. Everything
/// outside the markers is preserved exactly, including a missing final
/// newline on the user's own last line — the only byte added is the newline
/// needed to keep the block on its own lines.
pub fn replace_managed_block(existing: &str, block: Option<&str>) -> String {
    let start_idx = existing.find(BLOCK_START);
    let end_idx = existing.find(BLOCK_END);
    let (before, after) = match (start_idx, end_idx) {
        (Some(s), Some(e)) if e >= s => {
            let after_end = e + BLOCK_END.len();
            // Swallow the newline that terminated the old END marker so the
            // block doesn't leave a blank line behind when removed.
            let after_end = if existing[after_end..].starts_with('\n') { after_end + 1 } else { after_end };
            (&existing[..s], &existing[after_end..])
        }
        // Markers missing or out of order: treat the file as unmanaged and
        // append. A stray lone marker is left in place rather than guessed at.
        _ => (existing, ""),
    };
    let mut out = String::from(before);
    if let Some(block) = block {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(block);
    }
    out.push_str(after);
    out
}

fn crontab_missing(e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "crontab is not installed — install the `cron` (or `cronie`) package and make sure the cron service is running".to_string()
    } else {
        format!("failed to run crontab: {e}")
    }
}

/// `crontab -l` exits 1 with "no crontab for <user>" when none exists yet;
/// that's an empty file, not an error.
fn read_crontab() -> Result<String, String> {
    let out = Command::new("crontab").arg("-l").output().map_err(|e| crontab_missing(&e))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("no crontab for") {
        return Ok(String::new());
    }
    Err(format!("crontab -l failed: {}", stderr.trim()))
}

fn write_crontab(content: &str) -> Result<(), String> {
    let mut child = Command::new("crontab")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| crontab_missing(&e))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(content.as_bytes()).map_err(|e| format!("failed to write crontab: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("failed to write crontab: {e}"))?;
    if !out.status.success() {
        return Err(format!("crontab rejected the new schedule: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

/// Regenerates the managed block from the current enabled tasks. Called only
/// from the commands that change definitions.
fn sync_crontab(app: &AppHandle, conn: &Connection) -> Result<(), String> {
    let tasks = list(conn).map_err(|e| e.to_string())?;
    let log_dir = logs_dir(app)?;
    let block = render_block(&tasks, &log_dir.to_string_lossy());
    let existing = read_crontab()?;
    let updated = replace_managed_block(&existing, block.as_deref());
    if updated == existing {
        return Ok(());
    }
    write_crontab(&updated)
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

const COLUMNS: &str = "id, name, schedule, command, working_dir, site_id, enabled, created_at";

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<ScheduledTask> {
    Ok(ScheduledTask {
        id: row.get(0)?,
        name: row.get(1)?,
        schedule: row.get(2)?,
        command: row.get(3)?,
        working_dir: row.get(4)?,
        site_id: row.get(5)?,
        enabled: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
    })
}

fn list(conn: &Connection) -> rusqlite::Result<Vec<ScheduledTask>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM scheduled_tasks ORDER BY created_at"))?;
    let rows = stmt.query_map([], row_to_task)?;
    rows.collect()
}

fn find(conn: &Connection, id: &str) -> Result<ScheduledTask, String> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM scheduled_tasks WHERE id = ?1"), [id], row_to_task)
        .map_err(|_| "scheduled task not found".to_string())
}

/// Same loose site reference as workers: no FK, so a site removed elsewhere
/// never blocks this module.
fn site_exists(conn: &Connection, site_id: &str) -> bool {
    conn.query_row("SELECT COUNT(*) FROM nginx_ssl_sites WHERE id = ?1", [site_id], |r| r.get::<_, i64>(0))
        .unwrap_or(0)
        > 0
}

fn site_project_path(conn: &Connection, site_id: &str) -> Option<String> {
    conn.query_row("SELECT project_path FROM nginx_ssl_sites WHERE id = ?1", [site_id], |r| {
        r.get::<_, Option<String>>(0)
    })
    .ok()
    .flatten()
}

fn resolve_working_dir(explicit: Option<&str>, site_project_path: Option<&str>) -> Result<String, String> {
    if let Some(dir) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(dir.to_string());
    }
    if let Some(path) = site_project_path.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(path.to_string());
    }
    Err("a working directory is required — set one directly, or choose a site that has a project folder".to_string())
}

/// Trims and validates the free-text fields. The schedule is normalised to
/// single spaces so `0  3 * * *` and `0 3 * * *` compare equal.
fn validate_definition(name: &str, schedule: &str, command: &str) -> Result<(String, String, String), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    let schedule = schedule.split_whitespace().collect::<Vec<_>>().join(" ");
    parse_cron(&schedule).map_err(|e| format!("invalid schedule: {e}"))?;
    let command = command.trim().to_string();
    if command.is_empty() {
        return Err("command cannot be empty".to_string());
    }
    if command.contains('\n') {
        return Err("command must be a single line".to_string());
    }
    Ok((name, schedule, command))
}

fn logs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("scheduled-logs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn log_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(logs_dir(app)?.join(format!("{id}.log")))
}

fn to_view(app: &AppHandle, task: ScheduledTask) -> ScheduledTaskView {
    let now = Local::now();
    let (description, next) = match parse_cron(&task.schedule) {
        Ok(spec) => {
            let next = next_run(&spec, now.naive_local())
                .and_then(|n| n.and_local_timezone(Local).single())
                .map(|d| d.to_rfc3339());
            (describe(&spec), if task.enabled { next } else { None })
        }
        Err(e) => (format!("invalid schedule: {e}"), None),
    };
    let (last_run_at, last_output_tail) = match log_path(app, &task.id) {
        Ok(path) if path.exists() => {
            let mtime = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .map(|t| DateTime::<Local>::from(t).to_rfc3339());
            let tail = crate::modules::tail::tail_file(&path, TAIL_PREVIEW_LINES).unwrap_or_default();
            (mtime, tail)
        }
        _ => (None, String::new()),
    };
    ScheduledTaskView { task, description, next_run: next, last_run_at, last_output_tail }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command(async)]
pub fn scheduled_list(app: AppHandle, db: State<crate::db::Db>) -> Result<Vec<ScheduledTaskView>, String> {
    let tasks = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        list(&conn).map_err(|e| e.to_string())?
    };
    Ok(tasks.into_iter().map(|t| to_view(&app, t)).collect())
}

/// Create (`id` null) or update. The definition is persisted first and the
/// crontab synced second, so a missing `crontab` binary yields a clear error
/// without losing what the user typed — they can install cron and toggle the
/// task to retry.
#[tauri::command(async)]
pub fn scheduled_save(
    app: AppHandle,
    db: State<crate::db::Db>,
    id: Option<String>,
    name: String,
    schedule: String,
    command: String,
    working_dir: Option<String>,
    site_id: Option<String>,
    enabled: bool,
) -> Result<ScheduledTaskView, String> {
    let (name, schedule, command) = validate_definition(&name, &schedule, &command)?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    if let Some(sid) = &site_id {
        if !site_exists(&conn, sid) {
            return Err("selected site not found".to_string());
        }
    }
    let site_path = site_id.as_deref().and_then(|sid| site_project_path(&conn, sid));
    let working_dir = resolve_working_dir(working_dir.as_deref(), site_path.as_deref())?;

    let id = match id {
        Some(id) => {
            find(&conn, &id)?;
            conn.execute(
                "UPDATE scheduled_tasks SET name = ?1, schedule = ?2, command = ?3, working_dir = ?4, site_id = ?5, enabled = ?6 WHERE id = ?7",
                (&name, &schedule, &command, &working_dir, &site_id, enabled as i64, &id),
            )
            .map_err(|e| e.to_string())?;
            id
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO scheduled_tasks (id, name, schedule, command, working_dir, site_id, enabled, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                (&id, &name, &schedule, &command, &working_dir, &site_id, enabled as i64, chrono::Utc::now().to_rfc3339()),
            )
            .map_err(|e| e.to_string())?;
            id
        }
    };

    sync_crontab(&app, &conn).map_err(|e| format!("task saved, but the crontab was not updated: {e}"))?;
    Ok(to_view(&app, find(&conn, &id)?))
}

#[tauri::command(async)]
pub fn scheduled_delete(app: AppHandle, db: State<crate::db::Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM scheduled_tasks WHERE id = ?1", [&id]).map_err(|e| e.to_string())?;
    if let Ok(path) = log_path(&app, &id) {
        let _ = std::fs::remove_file(path);
    }
    sync_crontab(&app, &conn).map_err(|e| format!("task deleted, but the crontab was not updated: {e}"))
}

#[tauri::command(async)]
pub fn scheduled_set_enabled(
    app: AppHandle,
    db: State<crate::db::Db>,
    id: String,
    enabled: bool,
) -> Result<ScheduledTaskView, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    find(&conn, &id)?;
    conn.execute("UPDATE scheduled_tasks SET enabled = ?1 WHERE id = ?2", (enabled as i64, &id))
        .map_err(|e| e.to_string())?;
    sync_crontab(&app, &conn).map_err(|e| format!("setting saved, but the crontab was not updated: {e}"))?;
    Ok(to_view(&app, find(&conn, &id)?))
}

/// Runs the task once, right now, exactly as cron would (same `sh -c`, same
/// working dir, same log file) — for "did I get the command right?" without
/// waiting for the next tick. Returns as soon as the process is spawned; the
/// page polls the log for output, since a task may legitimately take minutes.
#[tauri::command(async)]
pub fn scheduled_run_now(app: AppHandle, db: State<crate::db::Db>, id: String) -> Result<(), String> {
    let task = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        find(&conn, &id)?
    };
    if !std::path::Path::new(&task.working_dir).is_dir() {
        return Err(format!("working directory does not exist: {}", task.working_dir));
    }
    let path = log_path(&app, &id)?;
    let mut log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("failed to open log file: {e}"))?;
    // A header so a manual run is distinguishable from cron's in the log.
    let _ = writeln!(log_file, "--- manual run at {} ---", Local::now().format("%Y-%m-%d %H:%M:%S"));
    let stderr_file = log_file.try_clone().map_err(|e| e.to_string())?;

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&task.command)
        .current_dir(&task.working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|e| format!("failed to start command: {e}"))?;
    // Reap the child in the background so it never lingers as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[tauri::command(async)]
pub fn scheduled_logs(app: AppHandle, id: String, tail: u32) -> Result<String, String> {
    let path = log_path(&app, &id)?;
    if !path.exists() {
        return Ok("(no runs logged yet)".to_string());
    }
    let text = crate::modules::tail::tail_file(&path, tail as usize).map_err(|e| e.to_string())?;
    if text.trim().is_empty() {
        Ok("(no runs logged yet)".to_string())
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::tail::tail_lines;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap()
    }

    fn task(id: &str, schedule: &str, command: &str, dir: &str, enabled: bool) -> ScheduledTask {
        ScheduledTask {
            id: id.to_string(),
            name: format!("Task {id}"),
            schedule: schedule.to_string(),
            command: command.to_string(),
            working_dir: dir.to_string(),
            site_id: None,
            enabled,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    // --- parsing ---

    #[test]
    fn parses_wildcards_steps_ranges_and_lists() {
        let spec = parse_cron("*/15 1-3 1,15 * mon-fri").unwrap();
        assert_eq!(spec.minute.values, vec![0, 15, 30, 45]);
        assert!(!spec.minute.any);
        assert_eq!(spec.hour.values, vec![1, 2, 3]);
        assert_eq!(spec.dom.values, vec![1, 15]);
        assert!(spec.month.any);
        assert_eq!(spec.dow.values, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn parses_month_and_day_names_case_insensitively() {
        let spec = parse_cron("0 0 1 JAN,jul Sun").unwrap();
        assert_eq!(spec.month.values, vec![1, 7]);
        assert_eq!(spec.dow.values, vec![0]);
    }

    #[test]
    fn sunday_seven_is_normalised_to_zero() {
        let spec = parse_cron("0 0 * * 7").unwrap();
        assert_eq!(spec.dow.values, vec![0]);
        let spec = parse_cron("0 0 * * 0,7").unwrap();
        assert_eq!(spec.dow.values, vec![0]);
    }

    #[test]
    fn rejects_malformed_expressions() {
        assert!(parse_cron("* * * *").is_err());
        assert!(parse_cron("* * * * * *").is_err());
        assert!(parse_cron("60 * * * *").is_err());
        assert!(parse_cron("* 24 * * *").is_err());
        assert!(parse_cron("* * 0 * *").is_err());
        assert!(parse_cron("* * * 13 *").is_err());
        assert!(parse_cron("* * * * 8").is_err());
        assert!(parse_cron("*/0 * * * *").is_err());
        assert!(parse_cron("5-1 * * * *").is_err());
        assert!(parse_cron("abc * * * *").is_err());
        assert!(parse_cron("* * * * ,").is_err());
    }

    // --- description ---

    fn d(expr: &str) -> String {
        describe(&parse_cron(expr).unwrap())
    }

    #[test]
    fn describes_preset_shapes() {
        assert_eq!(d("* * * * *"), "Every minute");
        assert_eq!(d("*/5 * * * *"), "Every 5 minutes");
        assert_eq!(d("0 * * * *"), "Every hour");
        assert_eq!(d("30 * * * *"), "Every hour at :30");
        assert_eq!(d("0 3 * * *"), "At 03:00 every day");
        assert_eq!(d("0 3 * * 1"), "At 03:00 on Monday");
        assert_eq!(d("0 */6 * * *"), "Every 6 hours");
    }

    #[test]
    fn describes_day_combinations() {
        assert_eq!(d("0 9 * * 1-5"), "At 09:00 on weekdays");
        assert_eq!(d("0 9 * * 0,6"), "At 09:00 on weekends");
        assert_eq!(d("0 9 * * mon,wed,fri"), "At 09:00 on Monday, Wednesday and Friday");
        assert_eq!(d("0 0 1 * *"), "At 00:00 on day 1 of the month");
        assert_eq!(d("0 0 1 jan *"), "At 00:00 on day 1 of the month in January");
        assert_eq!(d("0 8,20 * * *"), "At 08:00 and 20:00 every day");
        assert_eq!(d("*/10 * * * sat"), "Every 10 minutes on Saturday");
    }

    #[test]
    fn describes_anything_else_generically() {
        assert_eq!(d("1,2 3,4 * * *"), "At minute 1 and 2 past hour 3 and 4 every day");
    }

    // --- next run ---

    fn n(expr: &str, after: NaiveDateTime) -> Option<NaiveDateTime> {
        next_run(&parse_cron(expr).unwrap(), after)
    }

    #[test]
    fn next_run_is_strictly_after_now() {
        // 2026-03-02 is a Monday.
        assert_eq!(n("* * * * *", at(2026, 3, 2, 10, 0)), Some(at(2026, 3, 2, 10, 1)));
        assert_eq!(n("0 10 * * *", at(2026, 3, 2, 10, 0)), Some(at(2026, 3, 3, 10, 0)));
    }

    #[test]
    fn next_run_handles_steps_days_and_months() {
        assert_eq!(n("*/15 * * * *", at(2026, 3, 2, 10, 7)), Some(at(2026, 3, 2, 10, 15)));
        assert_eq!(n("0 3 * * *", at(2026, 3, 2, 10, 0)), Some(at(2026, 3, 3, 3, 0)));
        assert_eq!(n("0 3 * * 5", at(2026, 3, 2, 10, 0)), Some(at(2026, 3, 6, 3, 0)));
        assert_eq!(n("0 0 1 * *", at(2026, 3, 2, 10, 0)), Some(at(2026, 4, 1, 0, 0)));
        assert_eq!(n("0 0 25 dec *", at(2026, 3, 2, 10, 0)), Some(at(2026, 12, 25, 0, 0)));
        assert_eq!(n("0 0 1 1 *", at(2026, 3, 2, 10, 0)), Some(at(2027, 1, 1, 0, 0)));
    }

    #[test]
    fn next_run_uses_or_when_both_day_fields_are_set() {
        // Day 15 or a Sunday, whichever comes first after Monday 2 March 2026.
        assert_eq!(n("0 0 15 * 0", at(2026, 3, 2, 10, 0)), Some(at(2026, 3, 8, 0, 0)));
    }

    #[test]
    fn next_run_gives_up_after_a_year() {
        assert_eq!(n("0 0 31 feb *", at(2026, 3, 2, 10, 0)), None);
    }

    // --- crontab text ---

    #[test]
    fn cron_line_quotes_paths_and_escapes_percent() {
        let t = task("abc", "*/5  * * * *", "date +%F && php artisan schedule:run", "/home/me/my site", true);
        assert_eq!(
            cron_line(&t, "/data/scheduled-logs/abc.log"),
            r"*/5 * * * * cd '/home/me/my site' && date +\%F && php artisan schedule:run >> '/data/scheduled-logs/abc.log' 2>&1"
        );
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn render_block_includes_only_enabled_tasks() {
        let tasks = vec![
            task("a", "* * * * *", "echo a", "/a", true),
            task("b", "* * * * *", "echo b", "/b", false),
        ];
        let block = render_block(&tasks, "/logs").unwrap();
        assert!(block.starts_with(BLOCK_START));
        assert!(block.ends_with(&format!("{BLOCK_END}\n")));
        assert!(block.contains("# Task a\n* * * * * cd '/a' && echo a >> '/logs/a.log' 2>&1\n"));
        assert!(!block.contains("echo b"));
    }

    #[test]
    fn render_block_is_none_when_nothing_is_enabled() {
        assert_eq!(render_block(&[task("a", "* * * * *", "echo", "/a", false)], "/logs"), None);
        assert_eq!(render_block(&[], "/logs"), None);
    }

    #[test]
    fn render_block_strips_newlines_from_names() {
        let mut t = task("a", "* * * * *", "echo a", "/a", true);
        t.name = "sneaky\n* * * * * rm -rf /".to_string();
        let block = render_block(&[t], "/logs").unwrap();
        assert_eq!(block.lines().count(), 4);
        assert!(block.contains("# sneaky* * * * * rm -rf /\n"));
    }

    #[test]
    fn replace_appends_block_to_unmanaged_crontab() {
        let existing = "MAILTO=me\n0 1 * * * backup.sh\n";
        let block = "S\nline\nE\n";
        assert_eq!(replace_managed_block(existing, Some(block)), "MAILTO=me\n0 1 * * * backup.sh\nS\nline\nE\n");
    }

    #[test]
    fn replace_adds_missing_trailing_newline_before_appending() {
        assert_eq!(replace_managed_block("0 1 * * * backup.sh", Some("B\n")), "0 1 * * * backup.sh\nB\n");
        assert_eq!(replace_managed_block("", Some("B\n")), "B\n");
    }

    #[test]
    fn replace_swaps_existing_block_in_place_preserving_surroundings() {
        let existing = format!("# keep me\n{BLOCK_START}\nold line\n{BLOCK_END}\n# also keep\n");
        let block = format!("{BLOCK_START}\nnew line\n{BLOCK_END}\n");
        assert_eq!(
            replace_managed_block(&existing, Some(&block)),
            format!("# keep me\n{BLOCK_START}\nnew line\n{BLOCK_END}\n# also keep\n")
        );
    }

    #[test]
    fn replace_removes_block_when_none_is_given() {
        let existing = format!("# keep me\n{BLOCK_START}\nold line\n{BLOCK_END}\n# also keep\n");
        assert_eq!(replace_managed_block(&existing, None), "# keep me\n# also keep\n");
        assert_eq!(replace_managed_block("plain\n", None), "plain\n");
    }

    #[test]
    fn replace_is_idempotent() {
        let block = format!("{BLOCK_START}\nx\n{BLOCK_END}\n");
        let once = replace_managed_block("a\n", Some(&block));
        let twice = replace_managed_block(&once, Some(&block));
        assert_eq!(once, twice);
    }

    // --- definitions ---

    #[test]
    fn validate_definition_normalises_and_rejects() {
        let (name, schedule, command) = validate_definition(" Cron ", "0   3 * * *", " php artisan schedule:run ").unwrap();
        assert_eq!(name, "Cron");
        assert_eq!(schedule, "0 3 * * *");
        assert_eq!(command, "php artisan schedule:run");
        assert!(validate_definition("", "* * * * *", "x").is_err());
        assert!(validate_definition("n", "* * * *", "x").is_err());
        assert!(validate_definition("n", "* * * * *", " ").is_err());
        assert!(validate_definition("n", "* * * * *", "a\nb").is_err());
    }

    #[test]
    fn resolve_working_dir_prefers_explicit_then_site() {
        assert_eq!(resolve_working_dir(Some("/x"), Some("/s")).unwrap(), "/x");
        assert_eq!(resolve_working_dir(Some(" "), Some("/s")).unwrap(), "/s");
        assert!(resolve_working_dir(None, None).is_err());
    }

    #[test]
    fn tail_lines_returns_last_n() {
        assert_eq!(tail_lines("a\nb\nc", 2), "b\nc");
        assert_eq!(tail_lines("a", 0), "");
    }
}
