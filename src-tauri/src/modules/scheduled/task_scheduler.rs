//! Windows backend for scheduled tasks: Task Scheduler instead of cron.
//!
//! Each DevsPro task becomes one Task Scheduler task named `\DevsPro\<id>`,
//! registered from an XML definition with `schtasks /Create /XML /F`. Only
//! that folder is ever touched — the user's other tasks are never listed,
//! changed or deleted, the same promise the crontab block makes on Linux.
//! Disabled tasks stay registered but with `<Enabled>false</Enabled>`, so
//! Task Scheduler's own UI agrees with the switch in DevsPro.
//!
//! Task Scheduler has no cron syntax, so the cron expression is translated
//! into calendar triggers: the days a cron spec matches become a daily /
//! weekly / monthly rule, and the times of day become a start time plus a
//! repetition interval. Almost everything the UI can produce maps; the rare
//! expression that would need more than Task Scheduler's 48 triggers is
//! rejected at save time with a clear message.
//!
//! The XML building and the cron mapping are pure and compiled on every
//! platform so the tests run everywhere; only the `schtasks` calls are
//! Windows-only.

use super::{CronSpec, ScheduledTask, DAY_LONG, MONTH_LONG};

/// Task Scheduler folder that holds every task DevsPro manages.
pub const TASK_FOLDER: &str = r"\DevsPro\";

/// Task Scheduler refuses a task with more triggers than this.
const MAX_TRIGGERS: usize = 48;

/// Any date in the past works as the anchor of a calendar trigger; only the
/// time of day in it matters for daily / weekly / monthly schedules.
const START_DATE: &str = "2025-01-01";

/// Which days a trigger fires on — the four calendar schedules Task
/// Scheduler offers, minus the interval variants cron can't express anyway.
#[derive(Debug, Clone, PartialEq)]
pub enum DayRule {
    Daily,
    /// Weekdays, 0 = Sunday.
    Weekly(Vec<u8>),
    /// Days of the month in the given months (1-based).
    Monthly { days: Vec<u8>, months: Vec<u8> },
    /// Weekdays in the given months — cron's `* * * jan mon`.
    MonthlyWeekdays { weekdays: Vec<u8>, months: Vec<u8> },
}

/// Times of day as an arithmetic progression: `count` firings `step`
/// minutes apart starting `start` minutes after midnight.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeRun {
    pub start: u16,
    pub step: u16,
    pub count: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    pub days: DayRule,
    pub times: TimeRun,
}

/// Cron's day rule as Task Scheduler rules. Both day fields restricted means
/// "either matches" in cron, which is two triggers here.
fn day_rules(spec: &CronSpec) -> Vec<DayRule> {
    let months = spec.month.values.clone();
    let weekday_rule = || {
        if spec.month.any {
            DayRule::Weekly(spec.dow.values.clone())
        } else {
            DayRule::MonthlyWeekdays { weekdays: spec.dow.values.clone(), months: months.clone() }
        }
    };
    match (spec.dom.any, spec.dow.any) {
        (true, true) if spec.month.any => vec![DayRule::Daily],
        (true, true) => vec![DayRule::Monthly { days: (1..=31).collect(), months: months.clone() }],
        (false, true) => vec![DayRule::Monthly { days: spec.dom.values.clone(), months: months.clone() }],
        (true, false) => vec![weekday_rule()],
        (false, false) => vec![
            DayRule::Monthly { days: spec.dom.values.clone(), months: months.clone() },
            weekday_rule(),
        ],
    }
}

/// `(first, step)` when the values are evenly spaced; a single value is a
/// progression with step 0.
fn progression(values: &[u16]) -> Option<(u16, u16)> {
    match values {
        [] => None,
        [only] => Some((*only, 0)),
        [a, b, ..] => {
            let step = b - a;
            values.windows(2).all(|w| w[1] - w[0] == step).then_some((*a, step))
        }
    }
}

/// The times of day as few runs as possible, picking the smallest of:
/// everything as one run (`*/15 * * * *`), one run per listed hour (minutes
/// evenly spaced), one run per listed minute (hours evenly spaced), or one
/// trigger per time as the last resort.
fn time_runs(spec: &CronSpec) -> Vec<TimeRun> {
    let minutes: Vec<u16> = spec.minute.values.iter().map(|m| *m as u16).collect();
    let hours: Vec<u16> = spec.hour.values.iter().map(|h| *h as u16).collect();
    let all: Vec<u16> = hours.iter().flat_map(|h| minutes.iter().map(move |m| h * 60 + m)).collect();

    let mut options: Vec<Vec<TimeRun>> = Vec::new();
    if let Some((start, step)) = progression(&all) {
        options.push(vec![TimeRun { start, step, count: all.len() as u16 }]);
    }
    if let Some((m0, step)) = progression(&minutes) {
        options.push(
            hours.iter().map(|h| TimeRun { start: h * 60 + m0, step, count: minutes.len() as u16 }).collect(),
        );
    }
    if let Some((h0, step)) = progression(&hours) {
        options.push(
            minutes.iter().map(|m| TimeRun { start: h0 * 60 + m, step: step * 60, count: hours.len() as u16 }).collect(),
        );
    }
    options.push(all.iter().map(|t| TimeRun { start: *t, step: 0, count: 1 }).collect());
    options.into_iter().min_by_key(Vec::len).unwrap_or_default()
}

/// The Task Scheduler triggers equivalent to a cron spec, or an error naming
/// why it can't be expressed.
pub fn triggers_for(spec: &CronSpec) -> Result<Vec<Trigger>, String> {
    let days = day_rules(spec);
    let runs = time_runs(spec);
    let total = days.len() * runs.len();
    if total > MAX_TRIGGERS {
        return Err(format!(
            "Windows Task Scheduler can't express this schedule (it would need {total} triggers, the limit is {MAX_TRIGGERS}) — use evenly spaced minutes or hours, e.g. */5 or 0,30"
        ));
    }
    Ok(days
        .iter()
        .flat_map(|d| runs.iter().map(move |r| Trigger { days: d.clone(), times: r.clone() }))
        .collect())
}

// ---------------------------------------------------------------------------
// XML
// ---------------------------------------------------------------------------

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn weekday_elements(days: &[u8]) -> String {
    days.iter().map(|d| format!("<{} />", DAY_LONG[*d as usize])).collect()
}

fn month_elements(months: &[u8]) -> String {
    months.iter().map(|m| format!("<{} />", MONTH_LONG[*m as usize - 1])).collect()
}

fn trigger_xml(t: &Trigger) -> String {
    let mut out = String::from("<CalendarTrigger>");
    out.push_str(&format!(
        "<StartBoundary>{START_DATE}T{:02}:{:02}:00</StartBoundary>",
        t.times.start / 60,
        t.times.start % 60
    ));
    if t.times.count > 1 {
        // Duration runs one minute past the last firing so it is included.
        let duration = t.times.step * (t.times.count - 1) + 1;
        out.push_str(&format!(
            "<Repetition><Interval>PT{}M</Interval><Duration>PT{duration}M</Duration><StopAtDurationEnd>false</StopAtDurationEnd></Repetition>",
            t.times.step
        ));
    }
    match &t.days {
        DayRule::Daily => out.push_str("<ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>"),
        DayRule::Weekly(days) => out.push_str(&format!(
            "<ScheduleByWeek><WeeksInterval>1</WeeksInterval><DaysOfWeek>{}</DaysOfWeek></ScheduleByWeek>",
            weekday_elements(days)
        )),
        DayRule::Monthly { days, months } => {
            let days: String = days.iter().map(|d| format!("<Day>{d}</Day>")).collect();
            out.push_str(&format!(
                "<ScheduleByMonth><DaysOfMonth>{days}</DaysOfMonth><Months>{}</Months></ScheduleByMonth>",
                month_elements(months)
            ));
        }
        DayRule::MonthlyWeekdays { weekdays, months } => out.push_str(&format!(
            // Weeks 1-4 plus "last" covers every occurrence of a weekday.
            "<ScheduleByMonthDayOfWeek><Weeks><Week>1</Week><Week>2</Week><Week>3</Week><Week>4</Week><Week>Last</Week></Weeks><DaysOfWeek>{}</DaysOfWeek><Months>{}</Months></ScheduleByMonthDayOfWeek>",
            weekday_elements(weekdays),
            month_elements(months)
        )),
    }
    out.push_str("</CalendarTrigger>");
    out
}

/// The `cmd` arguments for a task: same shape as the cron line — change into
/// the working directory, run the command, append both streams to the log.
/// `/s` makes cmd strip only the outer quotes, so the command's own quotes
/// survive untouched.
pub fn cmd_arguments(task: &ScheduledTask, log_path: &str) -> String {
    format!(
        r#"/d /s /c "cd /d "{}" && {} >> "{}" 2>&1""#,
        quotable_dir(&task.working_dir),
        task.command,
        log_path
    )
}

/// A folder path that is safe right before a closing quote: conhost
/// re-parses its command line with the C runtime rules, where `\"` is an
/// escaped quote, so a trailing backslash would swallow the quote. A drive
/// root keeps meaning itself as `C:\.`.
fn quotable_dir(dir: &str) -> String {
    let trimmed = dir.trim_end_matches(['\\', '/']);
    if trimmed.len() < dir.len() && trimmed.ends_with(':') {
        format!(r"{trimmed}\.")
    } else {
        trimmed.to_string()
    }
}

/// The full Task Scheduler definition for one task.
///
/// `cmd` is started through `conhost --headless` so a task firing every
/// minute doesn't flash a console window on the desktop. Interactive token:
/// the task runs as the signed-in user without storing a password, which
/// also means it only runs while that user is logged on. Parallel instances
/// mirror cron, which never waits for the previous run either.
pub fn task_xml(task: &ScheduledTask, triggers: &[Trigger], log_path: &str) -> String {
    let triggers: String = triggers.iter().map(trigger_xml).collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Author>DevsPro</Author><Description>{name} — managed by DevsPro, edit it there</Description></RegistrationInfo>
  <Triggers>{triggers}</Triggers>
  <Principals><Principal id="Author"><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>Parallel</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Enabled>{enabled}</Enabled>
  </Settings>
  <Actions Context="Author">
    <Exec><Command>%SystemRoot%\System32\conhost.exe</Command><Arguments>--headless %SystemRoot%\System32\cmd.exe {args}</Arguments></Exec>
  </Actions>
</Task>
"#,
        name = xml_escape(&task.name),
        enabled = task.enabled,
        args = xml_escape(&cmd_arguments(task, log_path)),
    )
}

pub fn task_name(id: &str) -> String {
    format!("{TASK_FOLDER}{id}")
}

/// Task names from `schtasks /Query /FO CSV /NH` output that live directly
/// in the DevsPro folder. The first CSV column is the full task name, which
/// never contains a quote; the same task can repeat once per trigger.
pub fn managed_names(csv: &str) -> Vec<String> {
    let mut names: Vec<String> = csv
        .lines()
        .filter_map(|line| line.strip_prefix('"')?.split('"').next())
        .filter(|name| name.starts_with(TASK_FOLDER) && !name[TASK_FOLDER.len()..].contains('\\'))
        .map(str::to_string)
        .collect();
    names.dedup();
    names
}

// ---------------------------------------------------------------------------
// schtasks
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod schtasks {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn run(args: &[&str]) -> Result<std::process::Output, String> {
        Command::new("schtasks")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("failed to run schtasks: {e}"))
    }

    fn check(out: std::process::Output, what: &str) -> Result<(), String> {
        if out.status.success() {
            return Ok(());
        }
        let msg = String::from_utf8_lossy(&out.stderr);
        let msg = if msg.trim().is_empty() { String::from_utf8_lossy(&out.stdout) } else { msg };
        Err(format!("{what}: {}", msg.trim()))
    }

    /// Creates or replaces the task. schtasks only reads the XML from a file,
    /// written as UTF-16 with a BOM to match the declared encoding.
    pub fn register(task: &ScheduledTask, triggers: &[Trigger], log_path: &str) -> Result<(), String> {
        let xml = task_xml(task, triggers, log_path);
        let file = std::env::temp_dir().join(format!("devspro-task-{}.xml", task.id));
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(&file, bytes).map_err(|e| format!("failed to write task definition: {e}"))?;
        let path = file.to_string_lossy().into_owned();
        let out = run(&["/Create", "/TN", &task_name(&task.id), "/XML", &path, "/F"]);
        let _ = std::fs::remove_file(&file);
        check(out?, "Task Scheduler rejected the task")
    }

    /// Every task currently in the DevsPro folder. A folder that doesn't
    /// exist yet makes schtasks fail, which simply means there are none.
    pub fn list_managed() -> Result<Vec<String>, String> {
        let out = run(&["/Query", "/TN", TASK_FOLDER, "/FO", "CSV", "/NH"])?;
        if !out.status.success() {
            return Ok(Vec::new());
        }
        Ok(managed_names(&String::from_utf8_lossy(&out.stdout)))
    }

    pub fn delete(name: &str) -> Result<(), String> {
        check(run(&["/Delete", "/TN", name, "/F"])?, "failed to remove the scheduled task")
    }
}

#[cfg(windows)]
pub use schtasks::{delete, list_managed, register};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::scheduled::parse_cron;

    fn triggers(expr: &str) -> Vec<Trigger> {
        triggers_for(&parse_cron(expr).unwrap()).unwrap()
    }

    fn run(start: u16, step: u16, count: u16) -> TimeRun {
        TimeRun { start, step, count }
    }

    #[test]
    fn every_minute_is_one_daily_trigger_repeating_all_day() {
        assert_eq!(triggers("* * * * *"), vec![Trigger { days: DayRule::Daily, times: run(0, 1, 1440) }]);
    }

    #[test]
    fn evenly_spaced_minutes_and_hours_are_one_trigger() {
        assert_eq!(triggers("*/5 * * * *")[0].times, run(0, 5, 288));
        assert_eq!(triggers("0 * * * *")[0].times, run(0, 60, 24));
        assert_eq!(triggers("30 */6 * * *")[0].times, run(30, 360, 4));
        assert_eq!(triggers("0 8,20 * * *")[0].times, run(480, 720, 2));
        assert_eq!(triggers("*/15 1-3 * * *")[0].times, run(60, 15, 12));
        assert_eq!(triggers("0 3 * * *"), vec![Trigger { days: DayRule::Daily, times: run(180, 0, 1) }]);
    }

    #[test]
    fn uneven_steps_split_per_hour_or_per_minute() {
        // */7 restarts at :00 every hour, so the gap 56 -> 60 breaks one run;
        // one hourly run per minute value (9) beats one run per hour (24).
        let t = triggers("*/7 * * * *");
        assert_eq!(t.len(), 9);
        assert_eq!(t[1].times, run(7, 60, 24));
        let t = triggers("*/7 0,1,3 * * *");
        assert_eq!(t.len(), 3);
        assert_eq!(t[1].times, run(60, 7, 9));
        // Uneven minutes but every hour: one run per minute value.
        let t = triggers("1,2,4 * * * *");
        assert_eq!(t.iter().map(|t| t.times.clone()).collect::<Vec<_>>(), vec![run(1, 60, 24), run(2, 60, 24), run(4, 60, 24)]);
        let t = triggers("1,2 3,4 * * *");
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn day_fields_map_to_weekly_and_monthly_rules() {
        assert_eq!(triggers("0 9 * * 1-5")[0].days, DayRule::Weekly(vec![1, 2, 3, 4, 5]));
        assert_eq!(
            triggers("0 0 1,15 * *")[0].days,
            DayRule::Monthly { days: vec![1, 15], months: (1..=12).collect() }
        );
        assert_eq!(
            triggers("0 0 * jan,jul *")[0].days,
            DayRule::Monthly { days: (1..=31).collect(), months: vec![1, 7] }
        );
        assert_eq!(
            triggers("0 0 * dec sun")[0].days,
            DayRule::MonthlyWeekdays { weekdays: vec![0], months: vec![12] }
        );
    }

    #[test]
    fn both_day_fields_become_two_triggers_like_crons_or() {
        let t = triggers("0 0 15 * 0");
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].days, DayRule::Monthly { days: vec![15], months: (1..=12).collect() });
        assert_eq!(t[1].days, DayRule::Weekly(vec![0]));
    }

    #[test]
    fn schedules_needing_too_many_triggers_are_rejected() {
        let err = triggers_for(&parse_cron("1,2,4,8,16 0,1,3,7,9,13,15,19,20,22 * * *").unwrap()).unwrap_err();
        assert!(err.contains("can't express"), "{err}");
        // The OR rule doubles the count: 24 per-time triggers fit, 25 don't.
        assert_eq!(triggers("1,2,4 0,1,3,7,9,13,15,19 1 * 1").len(), 48);
        assert!(triggers_for(&parse_cron("1,2,4,8,16 0,1,3,7,9 1 * 1").unwrap()).is_err());
    }

    #[test]
    fn trigger_xml_has_repetition_only_when_repeating() {
        let x = trigger_xml(&triggers("*/5 * * * *")[0]);
        assert!(x.contains("<StartBoundary>2025-01-01T00:00:00</StartBoundary>"));
        assert!(x.contains("<Interval>PT5M</Interval><Duration>PT1436M</Duration>"));
        assert!(x.contains("<ScheduleByDay>"));
        let x = trigger_xml(&triggers("15 3 * * mon,fri")[0]);
        assert!(x.contains("T03:15:00"));
        assert!(!x.contains("Repetition"));
        assert!(x.contains("<DaysOfWeek><Monday /><Friday /></DaysOfWeek>"));
        let x = trigger_xml(&triggers("0 0 1 feb *")[0]);
        assert!(x.contains("<DaysOfMonth><Day>1</Day></DaysOfMonth><Months><February /></Months>"));
    }

    fn task(enabled: bool) -> ScheduledTask {
        ScheduledTask {
            id: "abc".to_string(),
            name: "A & <B>".to_string(),
            schedule: "* * * * *".to_string(),
            command: r#"php artisan "schedule:run""#.to_string(),
            working_dir: r"C:\My Sites\app".to_string(),
            site_id: None,
            enabled,
            created_at: String::new(),
        }
    }

    #[test]
    fn cmd_arguments_cd_run_and_append_to_log() {
        assert_eq!(
            cmd_arguments(&task(true), r"C:\logs\abc.log"),
            r#"/d /s /c "cd /d "C:\My Sites\app" && php artisan "schedule:run" >> "C:\logs\abc.log" 2>&1""#
        );
    }

    #[test]
    fn trailing_backslashes_never_touch_the_closing_quote() {
        assert_eq!(quotable_dir(r"C:\sites\app\"), r"C:\sites\app");
        assert_eq!(quotable_dir(r"C:\"), r"C:\.");
        assert_eq!(quotable_dir(r"D:\x"), r"D:\x");
    }

    #[test]
    fn task_xml_escapes_text_and_reflects_enabled() {
        let x = task_xml(&task(false), &triggers("* * * * *"), r"C:\logs\abc.log");
        assert!(x.contains("A &amp; &lt;B&gt; — managed by DevsPro"));
        assert!(x.contains("<Enabled>false</Enabled>"));
        assert!(x.contains("&amp;&amp; php artisan &quot;schedule:run&quot;"));
        assert!(task_xml(&task(true), &[], "l").contains("<Enabled>true</Enabled>"));
    }

    #[test]
    fn managed_names_keeps_only_direct_children_of_the_folder() {
        let csv = "\"\\DevsPro\\a\",\"N/A\",\"Ready\"\r\n\"\\DevsPro\\a\",\"N/A\",\"Ready\"\r\n\"\\DevsPro\\sub\\b\",\"N/A\",\"Ready\"\r\n\"\\Other\",\"N/A\",\"Ready\"\r\n";
        assert_eq!(managed_names(csv), vec![r"\DevsPro\a".to_string()]);
        assert_eq!(task_name("x"), r"\DevsPro\x");
    }
}
