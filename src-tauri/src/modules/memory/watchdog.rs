//! The RAM watchdog: a background thread, started from `setup`, that closes
//! one app when memory runs out — killer's `--daemon` mode, living in DevsPro's
//! backend so it keeps working while the window is closed to the tray.
//!
//! Every 8 s it reads its config, samples (through the shared `MemoryState`, so
//! idle tracking is the page's), and when RAM use is at or over the threshold
//! and nothing was closed in the last 20 s, stops the largest eligible app —
//! the largest *idle* one when `prefer_idle` is on and any are idle. Services
//! are never eligible: stopping one needs a password prompt nobody is there to
//! answer. Nor is anything protected (see `protect`).
//!
//! Each action is appended to `<app_data_dir>/memory-watchdog.log` as a JSON
//! line, emitted as `memory://watchdog`, and announced with `notify-send` when
//! it worked.

use super::catalog::{self, AppKind};
use super::control;
use super::sample::Entry;
use super::{AppState, MemoryState, WatchdogEvent, FRESH};
use crate::db::Db;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

pub const CONFIG_KEY: &str = "memory.watchdog";
pub const EVENT: &str = "memory://watchdog";
pub const LOG_FILE: &str = "memory-watchdog.log";

const INTERVAL: Duration = Duration::from_secs(8);
const COOLDOWN: Duration = Duration::from_secs(20);
/// What `memory_watchdog_log` returns.
const LOG_SHOWN: usize = 50;
/// The log is cut back to its last `LOG_KEPT` lines once it passes this.
const LOG_MAX_BYTES: u64 = 256 * 1024;
const LOG_KEPT: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchdogConfig {
    pub enabled: bool,
    /// Act at or above this RAM use, 50–99 %.
    pub threshold_pct: u8,
    /// Close the largest idle app when there is one, rather than the largest.
    pub prefer_idle: bool,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        WatchdogConfig {
            enabled: true,
            threshold_pct: 95,
            prefer_idle: true,
        }
    }
}

impl WatchdogConfig {
    pub fn clamped(self) -> Self {
        WatchdogConfig {
            threshold_pct: self.threshold_pct.clamp(50, 99),
            ..self
        }
    }
}

/// The saved config, or the default (on, 95 %, idle first) when there is none
/// or it doesn't parse.
pub fn load_config(conn: &Connection) -> WatchdogConfig {
    crate::modules::settings::get(conn, CONFIG_KEY)
        .and_then(|raw| serde_json::from_str::<WatchdogConfig>(&raw).ok())
        .unwrap_or_default()
        .clamped()
}

pub fn save_config(conn: &Connection, config: WatchdogConfig) -> Result<WatchdogConfig, String> {
    let config = config.clamped();
    let json = serde_json::to_string(&config).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (CONFIG_KEY, &json),
    )
    .map_err(|e| e.to_string())?;
    Ok(config)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Idle,
    Largest,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Idle => "idle",
            Reason::Largest => "largest",
        }
    }
}

pub fn should_act(config: &WatchdogConfig, used_pct: u8, last_kill: Option<Instant>, now: Instant) -> bool {
    config.enabled
        && used_pct >= config.threshold_pct
        && last_kill.is_none_or(|at| now.saturating_duration_since(at) >= COOLDOWN)
}

/// Whether the watchdog may close this app: running, using RAM, not
/// protected, not a service, and with something it can actually stop — a
/// Flatpak id, or at least one process of the user's own (another user's
/// process can't be signalled, and picking it would waste the one close per
/// 20 s while memory stays full).
fn eligible(entry: &Entry, my_uid: Option<u32>) -> bool {
    let row = &entry.row;
    if row.state == AppState::Off || row.ram_mb == 0 || row.protected || row.kind == AppKind::Service {
        return false;
    }
    let has_flatpak = row.kind == AppKind::Flatpak && entry.def.as_ref().is_some_and(|d| d.flatpak_id.is_some());
    has_flatpak || (my_uid.is_some() && entry.targets.iter().any(|t| t.uid == my_uid))
}

/// Which app to close, and why. `idle_known` is false until the sampler has
/// watched CPU for long enough to call anything idle; with `prefer_idle` on,
/// the watchdog waits for that rather than fall back to the largest app —
/// which right after DevsPro starts would be whatever the user is working in.
pub fn choose_victim(entries: &[Entry], my_uid: Option<u32>, prefer_idle: bool, idle_known: bool) -> Option<(&Entry, Reason)> {
    if prefer_idle && !idle_known {
        return None;
    }
    let candidates: Vec<&Entry> = entries.iter().filter(|e| eligible(e, my_uid)).collect();
    let idle: Vec<&Entry> = candidates.iter().copied().filter(|e| e.row.state == AppState::Idle).collect();
    let (pool, reason) = if prefer_idle && !idle.is_empty() { (idle, Reason::Idle) } else { (candidates, Reason::Largest) };
    pool.into_iter().max_by_key(|e| e.row.ram_mb).map(|e| (e, reason))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Starts the watchdog thread. Linux only; elsewhere there is nothing to read.
pub fn start(app: AppHandle) {
    if cfg!(not(target_os = "linux")) {
        return;
    }
    let spawned = std::thread::Builder::new().name("memory-watchdog".to_string()).spawn(move || {
        let mut last_kill: Option<Instant> = None;
        loop {
            std::thread::sleep(INTERVAL);
            tick(&app, &mut last_kill);
        }
    });
    if let Err(e) = spawned {
        eprintln!("memory: could not start the watchdog thread: {e}");
    }
}

fn tick(app: &AppHandle, last_kill: &mut Option<Instant>) {
    let (config, catalog) = {
        let db = app.state::<Db>();
        let Ok(conn) = db.0.lock() else {
            return;
        };
        (load_config(&conn), catalog::load(&conn))
    };
    let state = app.state::<MemoryState>();
    let sample = state.sample(&catalog, FRESH);
    let now = Instant::now();
    if !should_act(&config, sample.memory.used_pct, *last_kill, now) {
        return;
    }
    let Some((victim, reason)) = choose_victim(&sample.entries, sample.my_uid, config.prefer_idle, sample.idle_known) else {
        return;
    };
    let outcome = control::stop_entry(victim);
    *last_kill = Some(now);
    state.invalidate();
    record(
        app,
        &WatchdogEvent {
            at_ms: now_ms(),
            app: victim.row.name.clone(),
            ram_mb: victim.row.ram_mb,
            ram_pct: sample.memory.used_pct,
            reason: reason.as_str().to_string(),
            ok: outcome.ok,
            message: Some(outcome.message),
        },
    );
}

fn log_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|dir| dir.join(LOG_FILE))
}

fn record(app: &AppHandle, event: &WatchdogEvent) {
    if let Some(path) = log_path(app) {
        if let Err(e) = append_line(&path, event) {
            eprintln!("memory: could not write {}: {e}", path.display());
        }
    }
    let _ = app.emit(EVENT, event);
    if event.ok {
        notify(event);
    }
}

fn append_line(path: &Path, event: &WatchdogEvent) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if fs::metadata(path).is_ok_and(|m| m.len() > LOG_MAX_BYTES) {
        let kept = crate::modules::tail::tail_file(path, LOG_KEPT)?;
        fs::write(path, format!("{kept}\n"))?;
    }
    let line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

/// Best effort; a desktop without `notify-send` just doesn't get the toast.
fn notify(event: &WatchdogEvent) {
    let child = Command::new("notify-send")
        .arg(format!("DevsPro closed {}", event.app))
        .arg(format!("{} MB, RAM was {}%", event.ram_mb, event.ram_pct))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// The last `LOG_SHOWN` events, newest first. Lines that don't parse are
/// skipped.
pub fn read_log(app: &AppHandle) -> Vec<WatchdogEvent> {
    let Some(path) = log_path(app) else {
        return Vec::new();
    };
    let Ok(text) = crate::modules::tail::tail_file(&path, LOG_SHOWN) else {
        return Vec::new();
    };
    parse_log(&text)
}

fn parse_log(text: &str) -> Vec<WatchdogEvent> {
    text.lines().rev().filter_map(|l| serde_json::from_str(l).ok()).take(LOG_SHOWN).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::memory::sample::Target;
    use crate::modules::memory::{AppDef, AppRow};

    fn entry(id: &str, kind: AppKind, state: AppState, ram_mb: u64) -> Entry {
        Entry {
            row: AppRow {
                id: id.to_string(),
                name: id.to_string(),
                category: "Other".to_string(),
                kind,
                detected: false,
                state,
                ram_mb,
                cpu_pct: 0.0,
                process_count: 1,
                can_start: false,
                needs_admin: kind == AppKind::Service,
                protected: false,
            },
            def: Some(AppDef {
                id: id.to_string(),
                name: id.to_string(),
                category: "Other".to_string(),
                kind,
                matcher: id.to_string(),
                start: None,
                flatpak_id: (kind == AppKind::Flatpak).then(|| format!("org.example.{id}")),
                service: (kind == AppKind::Service).then(|| id.to_string()),
            }),
            targets: vec![Target {
                pid: 100,
                start_time: 1,
                uid: Some(1000),
                comm: id.to_string(),
            }],
        }
    }

    fn pick(entries: &[Entry], prefer_idle: bool) -> Option<(&str, Reason)> {
        choose_victim(entries, Some(1000), prefer_idle, true).map(|(e, r)| (e.row.id.as_str(), r))
    }

    #[test]
    fn prefers_the_largest_idle_app() {
        let entries = vec![
            entry("chrome", AppKind::Process, AppState::On, 4000),
            entry("slack", AppKind::Process, AppState::Idle, 800),
            entry("discord", AppKind::Flatpak, AppState::Idle, 1200),
        ];
        assert_eq!(pick(&entries, true), Some(("discord", Reason::Idle)));
        assert_eq!(pick(&entries, false), Some(("chrome", Reason::Largest)));
    }

    #[test]
    fn falls_back_to_the_largest_when_nothing_is_idle() {
        let entries = vec![
            entry("chrome", AppKind::Process, AppState::On, 4000),
            entry("code", AppKind::Process, AppState::On, 2500),
        ];
        assert_eq!(pick(&entries, true), Some(("chrome", Reason::Largest)));
    }

    #[test]
    fn never_picks_services_protected_stopped_or_empty_apps() {
        let mut protected = entry("shell", AppKind::Process, AppState::Idle, 9000);
        protected.row.protected = true;
        let mut other_user = entry("postgres", AppKind::Process, AppState::Idle, 7000);
        other_user.targets[0].uid = Some(999);
        let entries = vec![
            entry("mysql", AppKind::Service, AppState::On, 8000),
            protected,
            other_user,
            entry("gone", AppKind::Process, AppState::Off, 0),
            entry("tiny", AppKind::Process, AppState::Idle, 0),
            entry("zoom", AppKind::Process, AppState::On, 300),
        ];
        assert_eq!(pick(&entries, true), Some(("zoom", Reason::Largest)));
        assert_eq!(pick(&entries[..5], true), None);
    }

    #[test]
    fn a_flatpak_is_eligible_without_user_processes() {
        let mut teams = entry("teams", AppKind::Flatpak, AppState::On, 900);
        teams.targets.clear();
        assert_eq!(pick(&[teams], true), Some(("teams", Reason::Largest)));
    }

    #[test]
    fn waits_for_idle_data_when_preferring_idle() {
        let entries = vec![entry("chrome", AppKind::Process, AppState::On, 4000)];
        assert!(choose_victim(&entries, Some(1000), true, false).is_none());
        assert!(choose_victim(&entries, Some(1000), false, false).is_some());
    }

    #[test]
    fn acts_at_the_threshold_with_a_cooldown() {
        let config = WatchdogConfig::default();
        let t0 = Instant::now();
        assert!(!should_act(&config, 94, None, t0));
        assert!(should_act(&config, 95, None, t0));
        assert!(should_act(&config, 100, None, t0));
        assert!(!should_act(&config, 99, Some(t0), t0 + Duration::from_secs(19)));
        assert!(should_act(&config, 99, Some(t0), t0 + Duration::from_secs(20)));
        let off = WatchdogConfig { enabled: false, ..config };
        assert!(!should_act(&off, 100, None, t0));
    }

    #[test]
    fn config_defaults_on_and_clamps() {
        assert_eq!(WatchdogConfig::default(), WatchdogConfig { enabled: true, threshold_pct: 95, prefer_idle: true });
        assert_eq!(WatchdogConfig { threshold_pct: 10, ..Default::default() }.clamped().threshold_pct, 50);
        assert_eq!(WatchdogConfig { threshold_pct: 100, ..Default::default() }.clamped().threshold_pct, 99);
        // A partial object (an older or hand-edited value) fills in defaults.
        let partial: WatchdogConfig = serde_json::from_str(r#"{"enabled":false}"#).unwrap();
        assert_eq!(partial, WatchdogConfig { enabled: false, threshold_pct: 95, prefer_idle: true });
    }

    #[test]
    fn config_round_trips_through_settings() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);").unwrap();
        assert_eq!(load_config(&conn), WatchdogConfig::default());
        let stored = save_config(&conn, WatchdogConfig { enabled: false, threshold_pct: 120, prefer_idle: false }).unwrap();
        assert_eq!(stored.threshold_pct, 99);
        assert_eq!(load_config(&conn), stored);
    }

    #[test]
    fn log_is_read_newest_first_skipping_junk() {
        let line = |app: &str, at: u64| {
            serde_json::to_string(&WatchdogEvent {
                at_ms: at,
                app: app.to_string(),
                ram_mb: 1,
                ram_pct: 96,
                reason: "idle".to_string(),
                ok: true,
                message: None,
            })
            .unwrap()
        };
        let text = format!("{}\nnot json\n{}\n", line("a", 1), line("b", 2));
        let events = parse_log(&text);
        assert_eq!(events.iter().map(|e| e.app.as_str()).collect::<Vec<_>>(), ["b", "a"]);
    }
}
