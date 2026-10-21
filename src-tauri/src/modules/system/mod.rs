//! Live system snapshot for the dashboard — CPU, memory, disk, network, GPU and
//! temperatures, polled by the frontend every couple of seconds. The OS-specific reading lives in
//! `platform/`, one backend per target, behind the contract documented there; this
//! module owns the parts that are the same everywhere — the rate math and assembling
//! the snapshot the frontend receives.
//!
//! CPU and network figures are rates, which need two samples. Rather than
//! sleeping inside the command (which would block the frontend's poll for no
//! reason), the previous sample is kept in `SystemState` and diffed against
//! whatever the frontend's last poll handed us — the polling interval itself
//! becomes the sample window.

// Crate-visible so the terminal's status strip (ssh/stats.rs) can sample this
// machine on Windows, where there is no `/proc` script to run.
pub(crate) mod platform;

use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// One thermal sensor reading. `high_c` is the vendor's "running hot" point
/// (hwmon `tempN_max`) and `critical_c` the throttle/shutdown point
/// (`tempN_crit`); both are advisory and plenty of hardware reports neither.
#[derive(Debug, Serialize, Clone)]
pub struct TemperatureReading {
    pub label: String,
    pub celsius: f32,
    pub high_c: Option<f32>,
    pub critical_c: Option<f32>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CpuInfo {
    pub model: String,
    pub core_count: usize,
    pub usage_percent: f32,
    pub per_core_percent: Vec<f32>,
    /// `None` on platforms with no load average at all (Windows tracks nothing
    /// equivalent), so the UI can omit the figure instead of showing a zero that reads
    /// as a completely idle machine.
    pub load_avg_1: Option<f32>,
    pub load_avg_5: Option<f32>,
    pub load_avg_15: Option<f32>,
    /// The single number that answers "how hot is the CPU"; `None` when the
    /// machine exposes no CPU sensor at all (common in VMs and containers).
    pub temperature: Option<TemperatureReading>,
    /// Every sensor the CPU's driver exposes — package plus per core.
    pub temperatures: Vec<TemperatureReading>,
}

#[derive(Debug, Serialize, Clone)]
pub struct MemoryInfo {
    pub total_kb: u64,
    pub used_kb: u64,
    pub available_kb: u64,
    pub swap_total_kb: u64,
    pub swap_used_kb: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct DiskInfo {
    pub device: String,
    pub mount_point: String,
    pub fs_type: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct NetworkInterfaceInfo {
    pub name: String,
    pub rx_bytes_total: u64,
    pub tx_bytes_total: u64,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct GpuInfo {
    pub name: String,
    pub driver: Option<String>,
    pub usage_percent: Option<f32>,
    pub memory_used_mb: Option<f64>,
    pub memory_total_mb: Option<f64>,
    pub temperature: Option<TemperatureReading>,
}

#[derive(Debug, Serialize, Clone)]
pub struct SystemSnapshot {
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub uptime_seconds: u64,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub disks: Vec<DiskInfo>,
    pub network: Vec<NetworkInterfaceInfo>,
    pub gpus: Vec<GpuInfo>,
}

/// Who and what this machine is. Grouped because Windows reads all four from one
/// registry key, where Linux reads them from four separate files.
pub struct OsIdentity {
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct CpuTicks {
    pub(crate) idle: u64,
    pub(crate) total: u64,
}

#[derive(Default)]
pub struct SystemState {
    // Index 0 is the aggregate CPU entry, the rest are per-core.
    prev_cpu: Mutex<Option<Vec<CpuTicks>>>,
    prev_net: Mutex<Option<(Instant, HashMap<String, (u64, u64)>)>>,
    /// The CPU's marketing name never changes while the app runs, so it is read
    /// once instead of on every 2-second poll.
    cpu_model: OnceLock<String>,
}

/// The one reading that answers "how hot is this chip". A package/die sensor
/// covers the whole thing so it wins outright; failing that the hottest core is
/// the honest answer, since an average would hide the core that is throttling.
fn primary_temp(temps: &[TemperatureReading]) -> Option<TemperatureReading> {
    temps
        .iter()
        .find(|t| {
            let label = t.label.to_ascii_lowercase();
            label.starts_with("package") || label == "tctl" || label == "tdie"
        })
        .or_else(|| temps.iter().max_by(|a, b| a.celsius.total_cmp(&b.celsius)))
        .cloned()
}

fn cpu_percent(prev: &CpuTicks, cur: &CpuTicks) -> f32 {
    let total_delta = cur.total.saturating_sub(prev.total);
    if total_delta == 0 {
        return 0.0;
    }
    let idle_delta = cur.idle.saturating_sub(prev.idle);
    let active_delta = total_delta.saturating_sub(idle_delta);
    (active_delta as f32 / total_delta as f32 * 100.0).clamp(0.0, 100.0)
}

/// Overall and per-core busy percentages from the previous and current samples.
///
/// The two samples have to line up *and* actually contain the aggregate entry at index
/// 0. Checking only that their lengths match is not enough: two empty samples satisfy
/// that, and indexing `[0]` into them panicked. Inside a `#[tauri::command]` a panic
/// cannot unwind back across the FFI boundary, so it aborted the entire app rather than
/// failing one poll — which is exactly what happened on Windows, where the old
/// `/proc/stat` read yielded nothing. Any platform whose CPU source can't be read
/// returns an empty sample, so this is a reachable state, not a theoretical one.
fn cpu_percentages(prev: Option<&[CpuTicks]>, cur: &[CpuTicks]) -> (f32, Vec<f32>) {
    let per_core_len = cur.len().saturating_sub(1);
    match prev {
        Some(prev) if !cur.is_empty() && prev.len() == cur.len() => {
            let overall = cpu_percent(&prev[0], &cur[0]);
            let per_core = prev[1..]
                .iter()
                .zip(cur[1..].iter())
                .map(|(p, c)| cpu_percent(p, c))
                .collect();
            (overall, per_core)
        }
        _ => (0.0, vec![0.0; per_core_len]),
    }
}

#[tauri::command(async)]
pub fn system_info_snapshot(state: tauri::State<SystemState>) -> SystemSnapshot {
    let sample = platform::cpu_sample();
    let core_count = sample.core_count;
    let (usage_percent, per_core_percent) = {
        let mut prev_guard = state.prev_cpu.lock().unwrap();
        let result = cpu_percentages(prev_guard.as_deref(), &sample.ticks);
        *prev_guard = Some(sample.ticks);
        result
    };

    let load_avg = platform::load_avg();
    let temperatures = platform::cpu_temps();
    let cpu = CpuInfo {
        model: state.cpu_model.get_or_init(platform::cpu_model).clone(),
        core_count,
        usage_percent,
        per_core_percent,
        load_avg_1: load_avg.map(|(one, _, _)| one),
        load_avg_5: load_avg.map(|(_, five, _)| five),
        load_avg_15: load_avg.map(|(_, _, fifteen)| fifteen),
        temperature: primary_temp(&temperatures),
        temperatures,
    };

    let network = {
        let cur_net = platform::net_bytes();
        let mut prev_guard = state.prev_net.lock().unwrap();
        let now = Instant::now();
        let interfaces = match prev_guard.as_ref() {
            Some((prev_time, prev_map)) => {
                let elapsed = now.duration_since(*prev_time).as_secs_f64().max(0.001);
                cur_net
                    .iter()
                    .map(|(name, (rx, tx))| {
                        let (prev_rx, prev_tx) = prev_map.get(name).copied().unwrap_or((*rx, *tx));
                        NetworkInterfaceInfo {
                            name: name.clone(),
                            rx_bytes_total: *rx,
                            tx_bytes_total: *tx,
                            rx_bytes_per_sec: rx.saturating_sub(prev_rx) as f64 / elapsed,
                            tx_bytes_per_sec: tx.saturating_sub(prev_tx) as f64 / elapsed,
                        }
                    })
                    .collect()
            }
            None => cur_net
                .iter()
                .map(|(name, (rx, tx))| NetworkInterfaceInfo {
                    name: name.clone(),
                    rx_bytes_total: *rx,
                    tx_bytes_total: *tx,
                    rx_bytes_per_sec: 0.0,
                    tx_bytes_per_sec: 0.0,
                })
                .collect(),
        };
        *prev_guard = Some((now, cur_net));
        interfaces
    };

    let identity = platform::os_identity();

    SystemSnapshot {
        hostname: identity.hostname,
        os_name: identity.os_name,
        os_version: identity.os_version,
        kernel_version: identity.kernel_version,
        uptime_seconds: platform::uptime_seconds(),
        cpu,
        memory: platform::memory(),
        disks: platform::disks(),
        network,
        gpus: platform::gpus(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticks(idle: u64, total: u64) -> CpuTicks {
        CpuTicks { idle, total }
    }

    #[test]
    fn reports_zero_until_there_is_a_previous_sample_to_diff_against() {
        let (overall, per_core) = cpu_percentages(None, &[ticks(0, 100), ticks(0, 50), ticks(0, 50)]);
        assert_eq!(overall, 0.0);
        assert_eq!(per_core, vec![0.0, 0.0], "one entry per core, not per sample");
    }

    #[test]
    fn busy_percentage_is_the_non_idle_share_of_elapsed_ticks() {
        let prev = [ticks(100, 200), ticks(50, 100), ticks(50, 100)];
        let cur = [ticks(150, 400), ticks(100, 200), ticks(50, 200)];
        let (overall, per_core) = cpu_percentages(Some(&prev), &cur);
        // Aggregate: 200 ticks elapsed, 50 of them idle.
        assert_eq!(overall, 75.0);
        // First core idled for half its 100 ticks; the second never idled at all.
        assert_eq!(per_core, vec![50.0, 100.0]);
    }

    // Regression test for the crash this module's platform split fixed: a backend that
    // can read nothing returns an empty sample, and two of those in a row used to reach
    // `prev[0]` — a panic that aborts the process rather than failing one poll, because
    // it cannot unwind out of a #[tauri::command].
    #[test]
    fn empty_samples_report_zero_rather_than_panicking() {
        let (overall, per_core) = cpu_percentages(Some(&[]), &[]);
        assert_eq!(overall, 0.0);
        assert!(per_core.is_empty());
    }

    // A core coming online or going offline between polls changes the sample's length;
    // the mismatched pair is unusable, so that poll reports zero and the next one (now
    // with two same-shaped samples) is accurate again.
    #[test]
    fn a_changed_core_count_between_polls_reports_zero_for_that_poll() {
        let prev = [ticks(100, 200), ticks(100, 200)];
        let cur = [ticks(200, 400), ticks(100, 200), ticks(100, 200)];
        let (overall, per_core) = cpu_percentages(Some(&prev), &cur);
        assert_eq!(overall, 0.0);
        assert_eq!(per_core, vec![0.0, 0.0]);
    }

    // Counters are cumulative and only ever climb, but a wrapped or reset counter would
    // otherwise underflow the subtraction.
    #[test]
    fn a_counter_that_went_backwards_clamps_instead_of_underflowing() {
        let prev = [ticks(500, 1000)];
        let cur = [ticks(0, 0)];
        let (overall, _) = cpu_percentages(Some(&prev), &cur);
        assert_eq!(overall, 0.0);
    }

    fn reading(label: &str, celsius: f32) -> TemperatureReading {
        TemperatureReading {
            label: label.to_string(),
            celsius,
            high_c: None,
            critical_c: None,
        }
    }

    #[test]
    fn primary_temp_of_no_sensors_is_none() {
        assert!(primary_temp(&[]).is_none());
    }

    #[test]
    fn primary_temp_prefers_the_package_sensor_over_a_hotter_core() {
        let temps = vec![
            reading("Package id 0", 70.0),
            reading("Core 0", 66.0),
            reading("Core 1", 81.0),
        ];
        assert_eq!(primary_temp(&temps).unwrap().label, "Package id 0");
    }

    #[test]
    fn primary_temp_recognises_amd_die_sensors() {
        let temps = vec![reading("Tccd1", 55.0), reading("Tctl", 51.0)];
        assert_eq!(primary_temp(&temps).unwrap().label, "Tctl");
    }

    #[test]
    fn primary_temp_falls_back_to_the_hottest_sensor() {
        let temps = vec![reading("Core 0", 61.0), reading("Core 1", 74.0)];
        let hottest = primary_temp(&temps).unwrap();
        assert_eq!(hottest.label, "Core 1");
        assert_eq!(hottest.celsius, 74.0);
    }
}
