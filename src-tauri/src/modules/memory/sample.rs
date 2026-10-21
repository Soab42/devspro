//! Turns one `/proc` scan into the rows the page shows: catalog apps matched by
//! command line, everything else of the user's grouped by `comm`, CPU % from
//! the tick delta since the previous scan, and idle tracking.
//!
//! One `Sampler` is shared by the page's polling and the watchdog (it lives in
//! `MemoryState`), so CPU and idle figures are the same whoever asked. Both are
//! time-based rather than per-sample: CPU divides by the real time between two
//! scans, and an app is idle once its CPU has stayed low for `IDLE_AFTER` of
//! wall-clock time, however often anyone sampled in between.

use super::catalog::{self, AppDef, AppKind};
use super::procfs::{MemInfoKb, Proc, CLK_TCK, COMM_MAX};
use super::protect::{self, Guard};
use super::{AppRow, AppState, MemoryInfo};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
#[cfg(not(windows))]
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// killer's thresholds: under 1 % of one core for two readings 8 s apart.
pub const IDLE_CPU_PCT: f32 = 1.0;
pub const IDLE_AFTER: Duration = Duration::from_secs(16);

/// Two scans closer together than this give a CPU figure that is mostly
/// rounding, so the previous one is kept as the baseline instead.
const MIN_CPU_WINDOW: Duration = Duration::from_millis(250);

/// One process a stop would signal, with what identifies it for good.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub pid: u32,
    pub start_time: u64,
    pub uid: Option<u32>,
    pub comm: String,
}

impl Target {
    fn of(p: &Proc) -> Self {
        Target {
            pid: p.pid,
            start_time: p.start_time,
            uid: p.uid,
            comm: p.comm.clone(),
        }
    }
}

/// One row plus what acting on it needs.
#[derive(Debug, Clone)]
pub struct Entry {
    pub row: AppRow,
    /// The catalog entry, `None` for a detected app.
    pub def: Option<AppDef>,
    /// The processes a stop would signal: every matched process that is
    /// `Guard::Free`. Empty for a protected row.
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub at: Instant,
    pub at_ms: u64,
    pub memory: MemoryInfo,
    pub entries: Vec<Entry>,
    pub procs: Vec<Proc>,
    /// Parallel to `procs`.
    pub guards: Vec<Guard>,
    pub my_uid: Option<u32>,
    /// Whether CPU has been watched for `IDLE_AFTER` yet — before that no app
    /// can have been seen idle, however quiet it is.
    pub idle_known: bool,
}

impl Sample {
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.row.id == id)
    }
}

/// When each app's CPU dropped under `IDLE_CPU_PCT`, keyed by row id.
#[derive(Debug, Default)]
pub struct IdleTracker {
    low_since: HashMap<String, Instant>,
}

impl IdleTracker {
    /// Records one CPU reading for a running, non-service app.
    pub fn observe(&mut self, id: &str, cpu_pct: f32, now: Instant) {
        if cpu_pct < IDLE_CPU_PCT {
            self.low_since.entry(id.to_string()).or_insert(now);
        } else {
            self.low_since.remove(id);
        }
    }

    pub fn is_idle(&self, id: &str, now: Instant) -> bool {
        self.low_since
            .get(id)
            .is_some_and(|since| now.saturating_duration_since(*since) >= IDLE_AFTER)
    }

    /// Drops every app not in `running` — stopped apps and services start
    /// over the next time they show up.
    pub fn retain(&mut self, running: &HashSet<&str>) {
        self.low_since.retain(|id, _| running.contains(id.as_str()));
    }
}

/// How the processes of one scan fall into apps. Indices point into the scan.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Grouping {
    /// One per catalog entry, in catalog order: (free matches, kept matches).
    /// `Guard::Tree` processes are left out entirely.
    pub catalog: Vec<(Vec<usize>, Vec<usize>)>,
    /// Detected apps, by `comm`, in order of first appearance.
    pub detected: Vec<(String, Vec<usize>)>,
}

/// How command lines and matchers are compared: as written on Linux, ignoring
/// case on Windows, where paths and program names are case-insensitive and the
/// same app shows up as `C:\Program Files\…` or `c:\program files\…`.
fn fold(text: &str) -> Cow<'_, str> {
    if cfg!(windows) {
        Cow::Owned(text.to_lowercase())
    } else {
        Cow::Borrowed(text)
    }
}

/// Catalog apps match processes of any user whose command line contains the
/// matcher (killer semantics), a process matching two entries counting in
/// both. Everything else that belongs to `my_uid`, is free, and is a live
/// userspace process becomes a detected app grouped by `comm`.
pub fn group(procs: &[Proc], guards: &[Guard], catalog: &[AppDef], my_uid: Option<u32>) -> Grouping {
    let mut grouping = Grouping {
        catalog: vec![(Vec::new(), Vec::new()); catalog.len()],
        detected: Vec::new(),
    };
    let matchers: Vec<Cow<str>> = catalog.iter().map(|def| fold(&def.matcher)).collect();
    let mut detected_index: HashMap<&str, usize> = HashMap::new();
    for (i, p) in procs.iter().enumerate() {
        if !p.is_live_userspace() || guards[i] == Guard::Tree {
            continue;
        }
        let cmdline = fold(&p.cmdline);
        let mut matched = false;
        for (c, matcher) in matchers.iter().enumerate() {
            if !matcher.is_empty() && cmdline.contains(matcher.as_ref()) {
                matched = true;
                if guards[i] == Guard::Free {
                    grouping.catalog[c].0.push(i);
                } else {
                    grouping.catalog[c].1.push(i);
                }
            }
        }
        if matched || guards[i] != Guard::Free || my_uid.is_none() || p.uid != my_uid {
            continue;
        }
        let slot = *detected_index.entry(p.comm.as_str()).or_insert_with(|| {
            grouping.detected.push((p.comm.clone(), Vec::new()));
            grouping.detected.len() - 1
        });
        grouping.detected[slot].1.push(i);
    }
    grouping
}

/// A detected app's name: its `comm`, or argv[0]'s basename when the kernel
/// cut the comm short (`WebKitWebProces` → `WebKitWebProcess`) or when the comm
/// is a thread name rather than a program — Node 24 names its main thread
/// `MainThread`, and the process's comm is its main thread's.
fn detected_name(p: &Proc) -> String {
    let truncated = p.comm.len() == COMM_MAX && p.exe_name.len() > COMM_MAX && p.exe_name.starts_with(&p.comm);
    if truncated || (p.comm == "MainThread" && !p.exe_name.is_empty()) {
        p.exe_name.clone()
    } else {
        p.comm.clone()
    }
}

/// Per-process CPU % of one core, by index into the scan, or `None` with no
/// usable baseline.
fn cpu_by_proc(procs: &[Proc], prev: Option<&(Instant, HashMap<(u32, u64), u64>)>, now: Instant) -> Option<Vec<f32>> {
    let (prev_at, prev_ticks) = prev?;
    let elapsed = now.saturating_duration_since(*prev_at);
    if elapsed < MIN_CPU_WINDOW {
        return None;
    }
    let per_tick = 100.0 / (elapsed.as_secs_f64() * CLK_TCK as f64);
    Some(
        procs
            .iter()
            .map(|p| match prev_ticks.get(&(p.pid, p.start_time)) {
                Some(before) => (p.ticks.saturating_sub(*before) as f64 * per_tick) as f32,
                None => 0.0,
            })
            .collect(),
    )
}

struct Totals {
    ram_mb: u64,
    /// Unrounded, for the idle tracker; rows show it to one decimal.
    cpu_pct: f32,
    count: u32,
}

fn totals(indices: &[usize], procs: &[Proc], cpu: Option<&Vec<f32>>) -> Totals {
    let rss_kb: u64 = indices.iter().map(|&i| procs[i].rss_kb).sum();
    let cpu_pct: f32 = cpu.map(|c| indices.iter().map(|&i| c[i]).sum()).unwrap_or(0.0);
    Totals {
        ram_mb: rss_kb / 1024,
        cpu_pct,
        count: indices.len() as u32,
    }
}

fn round1(pct: f32) -> f32 {
    (pct * 10.0).round() / 10.0
}

/// Everything one sample is built from, so `Sampler::build` stays free of IO.
pub struct SampleInput<'a> {
    pub now: Instant,
    pub now_ms: u64,
    pub procs: Vec<Proc>,
    pub meminfo: MemInfoKb,
    pub catalog: &'a [AppDef],
    /// Catalog units `systemctl is-active` reported as active.
    pub active_units: &'a HashSet<String>,
    pub self_pid: u32,
    pub my_uid: Option<u32>,
}

#[derive(Debug, Default)]
pub struct Sampler {
    /// When the baseline scan was taken, and its ticks by (pid, start time).
    prev: Option<(Instant, HashMap<(u32, u64), u64>)>,
    /// The first sample with a CPU reading.
    watching_since: Option<Instant>,
    pub idle: IdleTracker,
}

impl Sampler {
    pub fn build(&mut self, input: SampleInput) -> Sample {
        let SampleInput { now, now_ms, procs, meminfo, catalog, active_units, self_pid, my_uid } = input;
        let guards = protect::classify(&procs, self_pid);
        let grouping = group(&procs, &guards, catalog, my_uid);
        let cpu = cpu_by_proc(&procs, self.prev.as_ref(), now);
        if cpu.is_some() && self.watching_since.is_none() {
            self.watching_since = Some(now);
        }

        let mut entries = Vec::with_capacity(catalog.len() + grouping.detected.len());
        let mut running_ids: HashSet<&str> = HashSet::new();

        for (def, (free, kept)) in catalog.iter().zip(&grouping.catalog) {
            let targets: Vec<Target> = free.iter().map(|&i| Target::of(&procs[i])).collect();
            let (state, stats, protected) = match def.kind {
                AppKind::Service => {
                    let unit = def.service.as_deref().unwrap_or_default();
                    let protected = protect::is_protected_unit(unit);
                    if active_units.contains(unit) {
                        let all: Vec<usize> = free.iter().chain(kept).copied().collect();
                        (AppState::On, totals(&all, &procs, cpu.as_ref()), protected)
                    } else {
                        (AppState::Off, totals(&[], &procs, None), protected)
                    }
                }
                AppKind::Process | AppKind::Flatpak => {
                    // A row made only of kept processes (an ancestor, a session
                    // process the matcher happens to hit) still shows as
                    // running, but protected: it has nothing to stop.
                    let (indices, protected) = if !free.is_empty() { (free, false) } else { (kept, !kept.is_empty()) };
                    if indices.is_empty() {
                        (AppState::Off, totals(&[], &procs, None), false)
                    } else {
                        let stats = totals(indices, &procs, cpu.as_ref());
                        (self.observe(&def.id, stats.cpu_pct, cpu.is_some(), now), stats, protected)
                    }
                }
            };
            if state != AppState::Off && def.kind != AppKind::Service {
                running_ids.insert(def.id.as_str());
            }
            let can_start = state == AppState::Off
                && match def.kind {
                    AppKind::Process => def.start.is_some(),
                    AppKind::Flatpak => def.flatpak_id.is_some(),
                    AppKind::Service => def.service.is_some(),
                };
            entries.push(Entry {
                row: AppRow {
                    id: def.id.clone(),
                    name: def.name.clone(),
                    category: def.category.clone(),
                    kind: def.kind,
                    detected: false,
                    state,
                    ram_mb: stats.ram_mb,
                    cpu_pct: round1(stats.cpu_pct),
                    process_count: stats.count,
                    can_start,
                    needs_admin: def.kind == AppKind::Service,
                    protected,
                },
                def: Some(def.clone()),
                targets: if protected { Vec::new() } else { targets },
            });
        }

        let mut detected = Vec::with_capacity(grouping.detected.len());
        for (comm, indices) in &grouping.detected {
            let first = &procs[indices[0]];
            let id = format!("auto:{comm}");
            let stats = totals(indices, &procs, cpu.as_ref());
            let state = self.observe(&id, stats.cpu_pct, cpu.is_some(), now);
            detected.push(Entry {
                row: AppRow {
                    id,
                    name: detected_name(first),
                    category: catalog::auto_category(&first.comm, &first.cmdline).to_string(),
                    kind: AppKind::Process,
                    detected: true,
                    state,
                    ram_mb: stats.ram_mb,
                    cpu_pct: round1(stats.cpu_pct),
                    process_count: stats.count,
                    can_start: false,
                    needs_admin: false,
                    protected: false,
                },
                def: None,
                targets: indices.iter().map(|&i| Target::of(&procs[i])).collect(),
            });
        }
        detected.sort_by(|a, b| b.row.ram_mb.cmp(&a.row.ram_mb).then_with(|| a.row.name.cmp(&b.row.name)));
        let detected_ids: Vec<String> = detected.iter().map(|e| e.row.id.clone()).collect();
        running_ids.extend(detected_ids.iter().map(String::as_str));
        self.idle.retain(&running_ids);
        entries.extend(detected);

        // Only move the baseline when this window was long enough to use: two
        // polls 100 ms apart would otherwise reset it to a near-empty window.
        if cpu.is_some() || self.prev.is_none() {
            self.prev = Some((now, procs.iter().map(|p| ((p.pid, p.start_time), p.ticks)).collect()));
        }

        Sample {
            at: now,
            at_ms: now_ms,
            memory: meminfo.summary(),
            entries,
            procs,
            guards,
            my_uid,
            idle_known: self
                .watching_since
                .is_some_and(|since| now.saturating_duration_since(since) >= IDLE_AFTER),
        }
    }

    /// Feeds the idle tracker (only with a real CPU reading) and says whether
    /// a running, non-service app is On or Idle.
    fn observe(&mut self, id: &str, cpu_pct: f32, cpu_known: bool, now: Instant) -> AppState {
        if cpu_known {
            self.idle.observe(id, cpu_pct, now);
        }
        if self.idle.is_idle(id, now) {
            AppState::Idle
        } else {
            AppState::On
        }
    }
}

/// Which of the catalog's units are active, from one `systemctl is-active`
/// call (it prints one line per unit, in order). Empty when systemd isn't
/// there. On Windows the units are services, asked of the Service Control
/// Manager.
pub fn active_units(catalog: &[AppDef]) -> HashSet<String> {
    let units: Vec<&str> = catalog
        .iter()
        .filter(|a| a.kind == AppKind::Service)
        .filter_map(|a| a.service.as_deref())
        .filter(|u| protect::valid_unit_name(u))
        .collect();
    if units.is_empty() {
        return HashSet::new();
    }
    systemd_active(&units)
}

#[cfg(windows)]
fn systemd_active(units: &[&str]) -> HashSet<String> {
    super::windows::active_services(units)
}

#[cfg(not(windows))]
fn systemd_active(units: &[&str]) -> HashSet<String> {
    if cfg!(not(target_os = "linux")) {
        return HashSet::new();
    }
    let Ok(out) = Command::new("systemctl")
        .arg("is-active")
        .arg("--")
        .args(units)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return HashSet::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .zip(units)
        .filter(|(state, _)| state.trim() == "active")
        .map(|(_, unit)| unit.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::memory::protect::tests::proc;

    fn process_def(id: &str, matcher: &str) -> AppDef {
        AppDef {
            id: id.to_string(),
            name: id.to_string(),
            category: "Browsers".to_string(),
            kind: AppKind::Process,
            matcher: matcher.to_string(),
            start: Some(id.to_string()),
            flatpak_id: None,
            service: None,
        }
    }

    fn service_def(id: &str, matcher: &str, unit: &str) -> AppDef {
        AppDef {
            kind: AppKind::Service,
            start: None,
            service: Some(unit.to_string()),
            category: "Servers".to_string(),
            ..process_def(id, matcher)
        }
    }

    fn other_user(mut p: Proc, uid: u32) -> Proc {
        p.uid = Some(uid);
        p
    }

    /// DevsPro (pid 500) under gnome-shell, Chrome with two processes, a
    /// root nginx, the user's own odds and ends, a kernel thread and a zombie.
    fn machine() -> Vec<Proc> {
        let mut zombie = proc(905, 1664, "defunct", "");
        zombie.state = 'Z';
        vec![
            proc(1, 0, "systemd", "/sbin/init"),
            proc(2, 0, "kthreadd", ""),
            proc(1664, 1, "gnome-shell", "/usr/bin/gnome-shell"),
            proc(500, 1664, "devspro", "devspro"),
            proc(501, 500, "WebKitWebProces", "/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess 4 23"),
            proc(8858, 1664, "chrome", "/opt/google/chrome/chrome"),
            proc(8977, 8858, "chrome", "/opt/google/chrome/chrome --type=gpu-process"),
            other_user(proc(700, 1, "nginx", "nginx: master process /usr/sbin/nginx"), 0),
            other_user(proc(701, 700, "nginx", "nginx: worker process"), 33),
            proc(900, 1664, "termius-app", "/opt/Termius/termius-app"),
            proc(901, 900, "termius-app", "/opt/Termius/termius-app --type=renderer"),
            proc(902, 1664, "zsh", "/usr/bin/zsh"),
            proc(903, 902, "claude", "claude"),
            other_user(proc(904, 1, "postgres", "postgres: checkpointer"), 999),
            zombie,
        ]
    }

    fn build(sampler: &mut Sampler, procs: Vec<Proc>, catalog: &[AppDef], active: &[&str], now: Instant) -> Sample {
        let active: HashSet<String> = active.iter().map(|s| s.to_string()).collect();
        sampler.build(SampleInput {
            now,
            now_ms: 0,
            procs,
            meminfo: MemInfoKb::default(),
            catalog,
            active_units: &active,
            self_pid: 500,
            my_uid: Some(1000),
        })
    }

    #[test]
    fn groups_catalog_matches_of_any_user_and_detects_only_the_users_own() {
        let procs = machine();
        let guards = protect::classify(&procs, 500);
        let catalog = [process_def("chrome", "/opt/google/chrome/chrome"), service_def("nginx", "nginx", "nginx")];
        let g = group(&procs, &guards, &catalog, Some(1000));
        let pids = |v: &[usize]| v.iter().map(|&i| procs[i].pid).collect::<Vec<_>>();
        assert_eq!(pids(&g.catalog[0].0), [8858, 8977]);
        assert_eq!(pids(&g.catalog[1].0), [700, 701]);
        let detected: Vec<(&str, Vec<u32>)> = g.detected.iter().map(|(c, v)| (c.as_str(), pids(v))).collect();
        // No DevsPro or its WebKit child, no shell, no root/other-user process,
        // no kernel thread, no zombie, nothing a catalog app claimed.
        assert_eq!(detected, [("termius-app", vec![900, 901]), ("claude", vec![903])]);
    }

    #[test]
    fn devspros_own_tree_is_invisible_even_to_a_matching_catalog_entry() {
        let procs = machine();
        let guards = protect::classify(&procs, 500);
        let catalog = [process_def("webkit", "WebKitWebProcess"), process_def("devspro", "devspro")];
        let g = group(&procs, &guards, &catalog, Some(1000));
        assert_eq!(g.catalog[0], (vec![], vec![]));
        assert_eq!(g.catalog[1], (vec![], vec![]));
    }

    #[test]
    fn a_catalog_entry_matching_only_session_processes_is_protected() {
        let mut sampler = Sampler::default();
        let catalog = [process_def("shell", "gnome-shell"), process_def("chrome", "/opt/google/chrome/chrome")];
        let s = build(&mut sampler, machine(), &catalog, &[], Instant::now());
        let shell = s.entry("shell").unwrap();
        assert_eq!(shell.row.state, AppState::On);
        assert!(shell.row.protected);
        assert!(shell.targets.is_empty());
        assert_eq!(shell.row.process_count, 1);
        let chrome = s.entry("chrome").unwrap();
        assert!(!chrome.row.protected);
        assert_eq!(chrome.targets.iter().map(|t| t.pid).collect::<Vec<_>>(), [8858, 8977]);
        assert_eq!(chrome.row.ram_mb, 200);
        assert_eq!(chrome.row.process_count, 2);
        assert!(!chrome.row.can_start, "running apps can't be started");
    }

    #[test]
    fn rows_carry_kind_specific_flags() {
        let mut sampler = Sampler::default();
        let catalog = [
            service_def("nginx", "nginx", "nginx"),
            service_def("mysql", "mysqld", "mysql"),
            process_def("firefox", "firefox"),
        ];
        let s = build(&mut sampler, machine(), &catalog, &["nginx"], Instant::now());
        let nginx = s.entry("nginx").unwrap();
        assert_eq!(nginx.row.state, AppState::On);
        assert!(nginx.row.needs_admin);
        assert!(!nginx.row.can_start);
        assert_eq!(nginx.row.process_count, 2);
        let mysql = s.entry("mysql").unwrap();
        assert_eq!(mysql.row.state, AppState::Off);
        assert!(mysql.row.can_start);
        let firefox = s.entry("firefox").unwrap();
        assert_eq!(firefox.row.state, AppState::Off);
        assert!(firefox.row.can_start);
        assert_eq!(firefox.row.ram_mb, 0);

        let claude = s.entry("auto:claude").unwrap();
        assert!(claude.row.detected);
        assert_eq!(claude.row.category, "Background");
        assert!(!claude.row.can_start);
        assert!(s.entry("auto:zsh").is_none());
        assert!(s.entry("auto:devspro").is_none());
    }

    #[test]
    #[cfg(not(windows))]
    fn detected_name_undoes_comm_truncation() {
        let mut p = proc(10, 1, "io.elementary.a", "io.elementary.appcenter -s");
        assert_eq!(detected_name(&p), "io.elementary.appcenter");
        p.exe_name = "something-else".to_string();
        assert_eq!(detected_name(&p), "io.elementary.a");
        assert_eq!(detected_name(&proc(11, 1, "MainThread", "node /usr/bin/pnpm dev")), "node");
    }

    #[test]
    fn cpu_is_the_tick_delta_over_real_time() {
        let mut sampler = Sampler::default();
        let catalog = [process_def("chrome", "/opt/google/chrome/chrome")];
        let t0 = Instant::now();
        let s = build(&mut sampler, machine(), &catalog, &[], t0);
        assert_eq!(s.entry("chrome").unwrap().row.cpu_pct, 0.0, "no baseline yet");

        // 4 s later Chrome's two processes used 1 s + 0.6 s of CPU: 1.6 s over
        // 4 s is 40 % of one core.
        let mut later = machine();
        later.iter_mut().find(|p| p.pid == 8858).unwrap().ticks += CLK_TCK;
        later.iter_mut().find(|p| p.pid == 8977).unwrap().ticks += CLK_TCK * 6 / 10;
        let s = build(&mut sampler, later, &catalog, &[], t0 + Duration::from_secs(4));
        assert_eq!(s.entry("chrome").unwrap().row.cpu_pct, 40.0);

        // A reused pid (new start time) doesn't inherit the old process's ticks.
        let mut reused = machine();
        let p = reused.iter_mut().find(|p| p.pid == 8858).unwrap();
        p.start_time += 1;
        p.ticks = 5_000;
        let s = build(&mut sampler, reused, &catalog, &[], t0 + Duration::from_secs(8));
        assert_eq!(s.entry("chrome").unwrap().row.cpu_pct, 0.0);
    }

    #[test]
    fn idle_needs_sixteen_seconds_of_low_cpu() {
        let t0 = Instant::now();
        let mut idle = IdleTracker::default();
        idle.observe("a", 0.5, t0);
        assert!(!idle.is_idle("a", t0 + Duration::from_secs(15)));
        // A reading in between doesn't restart the clock.
        idle.observe("a", 0.2, t0 + Duration::from_secs(8));
        assert!(idle.is_idle("a", t0 + Duration::from_secs(16)));
        // One busy reading does.
        idle.observe("a", 3.0, t0 + Duration::from_secs(20));
        assert!(!idle.is_idle("a", t0 + Duration::from_secs(20)));
        idle.observe("a", 0.0, t0 + Duration::from_secs(24));
        assert!(!idle.is_idle("a", t0 + Duration::from_secs(39)));
        assert!(idle.is_idle("a", t0 + Duration::from_secs(40)));
        // Exactly 1 % is not "under 1 %".
        idle.observe("b", 1.0, t0);
        assert!(!idle.is_idle("b", t0 + Duration::from_secs(60)));
        // Forgetting an app that stopped.
        idle.retain(&HashSet::new());
        assert!(!idle.is_idle("a", t0 + Duration::from_secs(60)));
    }

    #[test]
    fn idle_state_comes_from_the_shared_sampler_over_time() {
        let mut sampler = Sampler::default();
        let catalog = [process_def("chrome", "/opt/google/chrome/chrome"), service_def("nginx", "nginx", "nginx")];
        let t0 = Instant::now();
        for secs in [0, 4, 8, 12] {
            let s = build(&mut sampler, machine(), &catalog, &["nginx"], t0 + Duration::from_secs(secs));
            assert_eq!(s.entry("chrome").unwrap().row.state, AppState::On, "at {secs}s");
        }
        // First real reading was at 4 s, so idle from 20 s.
        assert!(!build(&mut sampler, machine(), &catalog, &["nginx"], t0 + Duration::from_secs(19)).idle_known);
        let s = build(&mut sampler, machine(), &catalog, &["nginx"], t0 + Duration::from_secs(20));
        assert!(s.idle_known);
        assert_eq!(s.entry("chrome").unwrap().row.state, AppState::Idle);
        assert_eq!(s.entry("auto:claude").unwrap().row.state, AppState::Idle);
        // A service is never idle.
        assert_eq!(s.entry("nginx").unwrap().row.state, AppState::On);
    }

    #[test]
    fn detected_apps_are_sorted_by_ram() {
        let mut procs = machine();
        procs.iter_mut().find(|p| p.pid == 903).unwrap().rss_kb = 500 * 1024;
        let s = build(&mut Sampler::default(), procs, &[], &[], Instant::now());
        let ids: Vec<&str> = s.entries.iter().map(|e| e.row.id.as_str()).collect();
        // Without a catalog Chrome is detected too.
        assert_eq!(ids, ["auto:claude", "auto:chrome", "auto:termius-app"]);
    }
}
