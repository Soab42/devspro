//! Memory — which apps are using the RAM, and ways to get it back. A port of
//! `killer`, a curses script the user ran in a terminal: the same app list with
//! start/stop, the same RAM watchdog, drop cache + reclaim swap, and the full
//! reset. The watchdog now lives in DevsPro's backend, so it keeps working
//! while the window sits in the tray.
//!
//! Linux only, like `processes` and `system`: everything is read from `/proc`.
//! Elsewhere the snapshot comes back empty and every action says unsupported.
//!
//! - `procfs`   — parsing `/proc/<pid>/{stat,status,cmdline}` and `/proc/meminfo`.
//! - `protect`  — what is never stopped: DevsPro's own tree, its ancestors, the
//!   desktop session.
//! - `catalog`  — the editable app list (`memory.apps`), its seed, auto-categories.
//! - `sample`   — a scan turned into app rows: grouping, CPU %, idle.
//! - `control`  — stop, start, drop caches, full reset; every signal is
//!   re-validated against a fresh scan first.
//! - `watchdog` — the background thread that closes an app when RAM runs out.

pub mod catalog;
mod control;
mod procfs;
mod protect;
mod sample;
pub mod watchdog;

pub use catalog::{AppDef, AppKind};
pub use watchdog::WatchdogConfig;

use crate::db::Db;
use crate::elevate::OpResult;
use sample::{Sample, SampleInput, Sampler};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State};

/// Callers this close together share one scan: the page polls every 4 s and
/// the watchdog every 8 s, and a Stop right after a poll needn't rescan.
const FRESH: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryInfo {
    pub total_mb: u64,
    pub available_mb: u64,
    /// (MemTotal - MemAvailable) * 100 / MemTotal.
    pub used_pct: u8,
    pub swap_total_mb: u64,
    pub swap_used_mb: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppState {
    On,
    /// Running, not a service, and under 1 % CPU for at least 16 s.
    Idle,
    Off,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppRow {
    /// Catalog id, or `auto:<comm>` for a detected app.
    pub id: String,
    pub name: String,
    pub category: String,
    pub kind: AppKind,
    /// Found running, not in the catalog.
    pub detected: bool,
    pub state: AppState,
    /// Sum of its processes' RSS.
    pub ram_mb: u64,
    /// Sum over its processes, % of one core, since the previous sample.
    pub cpu_pct: f32,
    pub process_count: u32,
    /// Has a start command / Flatpak id / unit, and is off.
    pub can_start: bool,
    /// A service: starting or stopping it shows the password prompt.
    pub needs_admin: bool,
    /// Matches only DevsPro's ancestors or desktop-session processes (or is a
    /// system unit): the watchdog and the reset never touch it, and Stop
    /// refuses it.
    pub protected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchdogEvent {
    pub at_ms: u64,
    /// The app's display name.
    pub app: String,
    pub ram_mb: u64,
    pub ram_pct: u8,
    /// "idle" or "largest".
    pub reason: String,
    pub ok: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemorySnapshot {
    pub taken_at_ms: u64,
    pub memory: MemoryInfo,
    pub apps: Vec<AppRow>,
    pub watchdog: WatchdogConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionResult {
    pub id: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResetTarget {
    pub pid: u32,
    /// `/proc/<pid>/stat` field 22; `memory_reset` refuses the pid if it no
    /// longer matches.
    pub start_time: u64,
    pub name: String,
    /// The command line, cut at 300 characters.
    pub command: String,
    pub ram_mb: u64,
    /// The name of the app row this process is counted under.
    pub app: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResetPreview {
    pub processes: Vec<ResetTarget>,
    /// Units of running catalog services.
    pub services: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResetRequest {
    /// (pid, start_time) pairs, as `[pid, start_time]` in JSON.
    pub processes: Vec<(u32, u64)>,
    pub services: Vec<String>,
    /// Also drop caches and reclaim swap, in the same elevated run.
    pub drop_caches: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResetSummary {
    pub killed: u32,
    pub failed: Vec<String>,
    /// The elevated helper's results: service stops, then the cache steps.
    pub admin: Vec<OpResult>,
}

/// The sampler and the latest sample, shared by every command and the
/// watchdog thread. One lock, held for the length of a scan, so two samples
/// never interleave and CPU deltas always run forward in time.
#[derive(Default)]
pub struct MemoryState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    sampler: Sampler,
    last: Option<Arc<Sample>>,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl MemoryState {
    /// The latest sample if it is younger than `max_age`, otherwise a new one.
    fn sample(&self, catalog: &[AppDef], max_age: Duration) -> Arc<Sample> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(last) = &inner.last {
            if last.at.elapsed() < max_age {
                return last.clone();
            }
        }
        let active_units = sample::active_units(catalog);
        let sample = Arc::new(inner.sampler.build(SampleInput {
            now: Instant::now(),
            now_ms: now_ms(),
            procs: procfs::scan(),
            meminfo: procfs::read_meminfo(),
            catalog,
            active_units: &active_units,
            self_pid: std::process::id(),
            my_uid: procfs::my_uid(),
        }));
        inner.last = Some(sample.clone());
        sample
    }

    /// Forgets the cached sample, after anything that changed what it shows.
    fn invalidate(&self) {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner).last = None;
    }
}

fn ensure_linux() -> Result<(), String> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        Err("the Memory module is only supported on Linux".to_string())
    }
}

fn read_catalog(db: &Db) -> Result<Vec<AppDef>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(catalog::load(&conn))
}

#[tauri::command(async)]
pub fn memory_snapshot(db: State<Db>, state: State<MemoryState>) -> Result<MemorySnapshot, String> {
    let (catalog, watchdog) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        (catalog::load(&conn), watchdog::load_config(&conn))
    };
    let sample = state.sample(&catalog, FRESH);
    Ok(MemorySnapshot {
        taken_at_ms: sample.at_ms,
        memory: sample.memory.clone(),
        apps: sample.entries.iter().map(|e| e.row.clone()).collect(),
        watchdog,
    })
}

#[tauri::command(async)]
pub fn memory_stop(app: AppHandle, db: State<Db>, state: State<MemoryState>, ids: Vec<String>) -> Result<Vec<ActionResult>, String> {
    ensure_linux()?;
    let catalog = read_catalog(&db)?;
    let sample = state.sample(&catalog, FRESH);
    let results = control::stop(&app, &sample, &ids);
    state.invalidate();
    Ok(results)
}

#[tauri::command(async)]
pub fn memory_start(app: AppHandle, db: State<Db>, state: State<MemoryState>, id: String) -> Result<ActionResult, String> {
    ensure_linux()?;
    let catalog = read_catalog(&db)?;
    let sample = state.sample(&catalog, FRESH);
    let Some(entry) = sample.entry(&id) else {
        return Ok(ActionResult {
            id,
            ok: false,
            message: "That app isn't in the list any more".to_string(),
        });
    };
    let outcome = control::start(&app, entry);
    state.invalidate();
    Ok(outcome)
}

#[tauri::command(async)]
pub fn memory_apps_get(db: State<Db>) -> Result<Vec<AppDef>, String> {
    read_catalog(&db)
}

#[tauri::command(async)]
pub fn memory_apps_set(db: State<Db>, state: State<MemoryState>, apps: Vec<AppDef>) -> Result<(), String> {
    let apps = catalog::validate(apps)?;
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        catalog::save(&conn, &apps)?;
    }
    state.invalidate();
    Ok(())
}

#[tauri::command(async)]
pub fn memory_watchdog_get(db: State<Db>) -> Result<WatchdogConfig, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(watchdog::load_config(&conn))
}

#[tauri::command(async)]
pub fn memory_watchdog_set(db: State<Db>, config: WatchdogConfig) -> Result<WatchdogConfig, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    watchdog::save_config(&conn, config)
}

#[tauri::command(async)]
pub fn memory_watchdog_log(app: AppHandle) -> Result<Vec<WatchdogEvent>, String> {
    Ok(watchdog::read_log(&app))
}

/// JS passes this as `{ reclaimSwap }`, Tauri's camelCase for argument names.
#[tauri::command(async)]
pub fn memory_drop_caches(app: AppHandle, reclaim_swap: bool) -> Result<Vec<OpResult>, String> {
    ensure_linux()?;
    control::drop_caches(&app, reclaim_swap)
}

#[tauri::command(async)]
pub fn memory_reset_preview(db: State<Db>, state: State<MemoryState>) -> Result<ResetPreview, String> {
    ensure_linux()?;
    let catalog = read_catalog(&db)?;
    let sample = state.sample(&catalog, FRESH);
    Ok(control::reset_preview(&sample))
}

#[tauri::command(async)]
pub fn memory_reset(app: AppHandle, db: State<Db>, state: State<MemoryState>, request: ResetRequest) -> Result<ResetSummary, String> {
    ensure_linux()?;
    let catalog = read_catalog(&db)?;
    let summary = control::reset(&app, &catalog, request);
    state.invalidate();
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn json_shapes_match_the_frontend_contract() {
        let row = AppRow {
            id: "auto:claude".to_string(),
            name: "claude".to_string(),
            category: "Background".to_string(),
            kind: AppKind::Process,
            detected: true,
            state: AppState::Idle,
            ram_mb: 1,
            cpu_pct: 0.5,
            process_count: 1,
            can_start: false,
            needs_admin: false,
            protected: false,
        };
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(json["state"], "idle");
        assert_eq!(json["kind"], "process");
        assert!(json.get("process_count").is_some() && json.get("needs_admin").is_some());

        let request: ResetRequest =
            serde_json::from_str(r#"{"processes":[[123,456789]],"services":["nginx"],"drop_caches":true}"#).unwrap();
        assert_eq!(request.processes, [(123, 456789)]);
        assert!(request.drop_caches);
    }

    /// Smoke test against this machine's real `/proc`: two scans 1.5 s apart,
    /// then the top 10 apps by RAM. Reads only — no signals, no `systemctl`
    /// (services therefore show as off). Run with
    /// `cargo test --lib memory::tests::smoke -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn smoke_top_apps_on_this_machine() {
        let catalog = catalog::seed();
        let mut sampler = Sampler::default();
        let none = HashSet::new();
        let take = |sampler: &mut Sampler| {
            sampler.build(SampleInput {
                now: Instant::now(),
                now_ms: now_ms(),
                procs: procfs::scan(),
                meminfo: procfs::read_meminfo(),
                catalog: &catalog,
                active_units: &none,
                self_pid: std::process::id(),
                my_uid: procfs::my_uid(),
            })
        };
        take(&mut sampler);
        std::thread::sleep(Duration::from_millis(1500));
        let s = take(&mut sampler);

        let m = &s.memory;
        println!(
            "RAM {}% used, {} MB free of {} MB; swap {} of {} MB used",
            m.used_pct, m.available_mb, m.total_mb, m.swap_used_mb, m.swap_total_mb
        );
        let count = |g: protect::Guard| s.guards.iter().filter(|x| **x == g).count();
        println!(
            "{} processes: {} free, {} kept (session/ancestors), {} DevsPro tree (this test binary + any running DevsPro)",
            s.procs.len(),
            count(protect::Guard::Free),
            count(protect::Guard::Kept),
            count(protect::Guard::Tree)
        );
        let mut rows: Vec<&AppRow> = s.entries.iter().map(|e| &e.row).filter(|r| r.state != AppState::Off).collect();
        rows.sort_by(|a, b| b.ram_mb.cmp(&a.ram_mb));
        println!("{:<26} {:<14} {:>8} {:>7} {:>5}  {:<5} flags", "app", "category", "RAM MB", "CPU %", "procs", "state");
        for r in rows.iter().take(10) {
            let mut flags = Vec::new();
            if r.detected {
                flags.push("detected");
            }
            if r.protected {
                flags.push("protected");
            }
            if r.needs_admin {
                flags.push("admin");
            }
            println!(
                "{:<26} {:<14} {:>8} {:>7.1} {:>5}  {:<5} {}",
                r.name,
                r.category,
                r.ram_mb,
                r.cpu_pct,
                r.process_count,
                format!("{:?}", r.state).to_lowercase(),
                flags.join(",")
            );
        }
        let preview = control::reset_preview(&s);
        println!("full reset would offer {} processes", preview.processes.len());
        let kept: Vec<String> = s
            .procs
            .iter()
            .zip(&s.guards)
            .filter(|(p, g)| **g == protect::Guard::Kept && p.uid == s.my_uid)
            .map(|(p, _)| p.comm.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        println!("kept (your processes): {}", kept.join(" "));
        let devspro_visible = s.entries.iter().any(|e| e.row.name.to_lowercase().contains("devspro"));
        println!("a DevsPro row is visible: {devspro_visible}");
    }
}
