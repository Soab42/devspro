//! Live system snapshot for the dashboard — CPU, memory, disk, network, GPU and
//! temperatures, polled by the frontend every couple of seconds. Reads `/proc`
//! and `/sys` directly and shells out to a couple of standard CLIs (`df`,
//! `lspci`, optionally `nvidia-smi`) rather than pulling in a system-info crate,
//! matching how the rest of this app talks to the OS (see `processes::mod`,
//! `docker::mod`).
//!
//! CPU and network figures are rates, which need two samples. Rather than
//! sleeping inside the command (which would block the frontend's poll for no
//! reason), the previous sample is kept in `SystemState` and diffed against
//! whatever the frontend's last poll handed us — the polling interval itself
//! becomes the sample window.

use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
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
    pub load_avg_1: f32,
    pub load_avg_5: f32,
    pub load_avg_15: f32,
    /// The single number that answers "how hot is the CPU"; `None` when the
    /// machine exposes no CPU sensor at all (common in VMs and containers).
    pub temperature: Option<TemperatureReading>,
    /// Every sensor the CPU's hwmon driver exposes — package plus per core.
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

#[derive(Clone, Copy, Default)]
struct CpuTicks {
    idle: u64,
    total: u64,
}

#[derive(Default)]
pub struct SystemState {
    // Index 0 is the aggregate "cpu" line, the rest are "cpu0", "cpu1", ...
    prev_cpu: Mutex<Option<Vec<CpuTicks>>>,
    prev_net: Mutex<Option<(Instant, HashMap<String, (u64, u64)>)>>,
}

fn read_cpu_ticks() -> Vec<CpuTicks> {
    let content = fs::read_to_string("/proc/stat").unwrap_or_default();
    content
        .lines()
        .filter(|l| l.starts_with("cpu"))
        .map(|line| {
            let fields: Vec<u64> = line
                .split_whitespace()
                .skip(1)
                .filter_map(|f| f.parse().ok())
                .collect();
            // user nice system idle iowait irq softirq steal guest guest_nice
            let idle = fields.get(3).copied().unwrap_or(0) + fields.get(4).copied().unwrap_or(0);
            let total: u64 = fields.iter().sum();
            CpuTicks { idle, total }
        })
        .collect()
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

fn cpu_model() -> String {
    let content = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    content
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string())
}

fn load_avg() -> (f32, f32, f32) {
    let content = fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let mut parts = content.split_whitespace();
    let mut next = || parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    (next(), next(), next())
}

fn read_memory() -> MemoryInfo {
    let content = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut map = HashMap::new();
    for line in content.lines() {
        if let Some((key, rest)) = line.split_once(':') {
            let value = rest
                .trim()
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            map.insert(key.to_string(), value);
        }
    }
    let total = *map.get("MemTotal").unwrap_or(&0);
    let available = *map.get("MemAvailable").unwrap_or(&0);
    let swap_total = *map.get("SwapTotal").unwrap_or(&0);
    let swap_free = *map.get("SwapFree").unwrap_or(&0);
    MemoryInfo {
        total_kb: total,
        used_kb: total.saturating_sub(available),
        available_kb: available,
        swap_total_kb: swap_total,
        swap_used_kb: swap_total.saturating_sub(swap_free),
    }
}

fn read_uptime() -> u64 {
    fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_string))
        .and_then(|v| v.parse::<f64>().ok())
        .map(|v| v as u64)
        .unwrap_or(0)
}

fn read_os_release() -> (String, String) {
    let content = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let mut map = HashMap::new();
    for line in content.lines() {
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.to_string(), v.trim_matches('"').to_string());
        }
    }
    let name = map
        .get("PRETTY_NAME")
        .or_else(|| map.get("NAME"))
        .cloned()
        .unwrap_or_else(|| "Linux".to_string());
    let version = map.get("VERSION_ID").cloned().unwrap_or_default();
    (name, version)
}

const PSEUDO_FILESYSTEMS: &[&str] = &[
    "tmpfs",
    "devtmpfs",
    "squashfs",
    "overlay",
    "proc",
    "sysfs",
    "cgroup",
    "cgroup2",
    "devpts",
    "efivarfs",
    "pstore",
    "bpf",
    "tracefs",
    "debugfs",
    "mqueue",
    "hugetlbfs",
    "fusectl",
    "configfs",
    "binfmt_misc",
    "autofs",
    "ramfs",
];

fn read_disks() -> Vec<DiskInfo> {
    let output = Command::new("df")
        .args(["-B1", "--output=source,fstype,size,used,avail,target"])
        .output();
    let Ok(output) = output else {
        return vec![];
    };
    if !output.status.success() {
        return vec![];
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 6 {
                return None;
            }
            let device = cols[0].to_string();
            let fs_type = cols[1].to_string();
            if !device.starts_with('/') || PSEUDO_FILESYSTEMS.contains(&fs_type.as_str()) {
                return None;
            }
            Some(DiskInfo {
                device,
                fs_type,
                total_bytes: cols[2].parse().unwrap_or(0),
                used_bytes: cols[3].parse().unwrap_or(0),
                available_bytes: cols[4].parse().unwrap_or(0),
                mount_point: cols[5..].join(" "),
            })
        })
        .collect()
}

/* ---------------------------------------------------------- temperatures -- */

/// hwmon drivers that report a CPU die temperature, best first. The vendor
/// drivers read the on-die sensors; `acpitz` is a firmware thermal zone that is
/// often the mainboard rather than the CPU, so it is only a last resort.
const CPU_HWMON_NAMES: &[&str] = &["coretemp", "k10temp", "zenpower", "cpu_thermal", "acpitz"];

/// hwmon reports millidegrees in every `temp*` file, so one reader covers
/// `_input`, `_max` and `_crit` alike.
fn read_hwmon_millidegrees(dir: &Path, file: &str) -> Option<f32> {
    let raw = fs::read_to_string(dir.join(file)).ok()?;
    let milli: f64 = raw.trim().parse().ok()?;
    Some((milli / 1000.0) as f32)
}

/// Every `tempN_input` under one hwmon directory, in sensor order. `fallback`
/// names the readings on drivers that ship no `tempN_label` (most single-sensor
/// ones do not).
fn read_hwmon_temps(dir: &Path, fallback: &str) -> Vec<TemperatureReading> {
    let Ok(entries) = fs::read_dir(dir) else {
        return vec![];
    };
    let mut readings: Vec<(u32, TemperatureReading)> = entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().to_string();
            let index: u32 = file
                .strip_prefix("temp")?
                .strip_suffix("_input")?
                .parse()
                .ok()?;
            let celsius = read_hwmon_millidegrees(dir, &file)?;
            // An unpopulated or disconnected sensor reads 0; nothing in a
            // running machine is actually at 0 °C, so treat it as absent.
            if celsius <= 0.0 {
                return None;
            }
            let label = fs::read_to_string(dir.join(format!("temp{index}_label")))
                .ok()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| fallback.to_string());
            Some((
                index,
                TemperatureReading {
                    label,
                    celsius,
                    high_c: read_hwmon_millidegrees(dir, &format!("temp{index}_max")),
                    critical_c: read_hwmon_millidegrees(dir, &format!("temp{index}_crit")),
                },
            ))
        })
        .collect();
    // readdir order is arbitrary; the sensor index is the vendor's own ordering
    // (package first, then core 0, 1, ...), which is what we want to display.
    readings.sort_by_key(|(index, _)| *index);
    readings.into_iter().map(|(_, reading)| reading).collect()
}

/// `(driver name, directory)` for every hwmon device on the machine.
fn hwmon_devices() -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir("/sys/class/hwmon") else {
        return vec![];
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            let name = fs::read_to_string(dir.join("name")).ok()?.trim().to_string();
            Some((name, dir))
        })
        .collect()
}

fn read_cpu_temps() -> Vec<TemperatureReading> {
    let devices = hwmon_devices();
    for wanted in CPU_HWMON_NAMES {
        let Some((name, dir)) = devices.iter().find(|(name, _)| name == wanted) else {
            continue;
        };
        let temps = read_hwmon_temps(dir, name);
        if !temps.is_empty() {
            return temps;
        }
    }
    vec![]
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

fn read_net_bytes() -> HashMap<String, (u64, u64)> {
    let content = fs::read_to_string("/proc/net/dev").unwrap_or_default();
    content
        .lines()
        .skip(2)
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let fields: Vec<&str> = rest.split_whitespace().collect();
            let rx = fields.first()?.parse().ok()?;
            let tx = fields.get(8)?.parse().ok()?;
            Some((name.trim().to_string(), (rx, tx)))
        })
        .collect()
}

fn parse_lspci_mm_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in line.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    fields.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        fields.push(current);
    }
    fields
}

fn driver_for_pci_slot(slot: &str) -> Option<String> {
    let path = format!("/sys/bus/pci/devices/0000:{slot}/driver");
    let target = fs::read_link(path).ok()?;
    target.file_name().map(|f| f.to_string_lossy().to_string())
}

/// A GPU's temperature straight from the kernel driver. `amdgpu`, `i915` and
/// `nouveau` all hang an hwmon device off the PCI device, which is the only way
/// to get a number out of the cards `nvidia-smi` knows nothing about.
fn temp_for_pci_slot(slot: &str) -> Option<TemperatureReading> {
    let hwmon_root = PathBuf::from(format!("/sys/bus/pci/devices/0000:{slot}/hwmon"));
    let dirs = fs::read_dir(hwmon_root).ok()?;
    for entry in dirs.flatten() {
        let name = fs::read_to_string(entry.path().join("name"))
            .unwrap_or_default()
            .trim()
            .to_string();
        let label = if name.is_empty() { "GPU".to_string() } else { name };
        if let Some(reading) = primary_temp(&read_hwmon_temps(&entry.path(), &label)) {
            return Some(reading);
        }
    }
    None
}

fn read_gpus_from_nvidia_smi() -> Option<Vec<GpuInfo>> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,driver_version,utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(
        text.lines()
            .filter_map(|line| {
                let cols: Vec<&str> = line.split(',').map(str::trim).collect();
                if cols.len() < 6 {
                    return None;
                }
                Some(GpuInfo {
                    name: cols[0].to_string(),
                    driver: Some(cols[1].to_string()),
                    usage_percent: cols[2].parse().ok(),
                    memory_used_mb: cols[3].parse().ok(),
                    memory_total_mb: cols[4].parse().ok(),
                    // nvidia-smi's CSV query has no column for the slowdown or
                    // shutdown thresholds, so the frontend falls back to its
                    // own GPU defaults for those.
                    temperature: cols[5].parse().ok().map(|celsius| TemperatureReading {
                        label: "GPU".to_string(),
                        celsius,
                        high_c: None,
                        critical_c: None,
                    }),
                })
            })
            .collect(),
    )
}

// Fallback for GPUs without a queryable vendor tool (e.g. an open-source driver
// like nouveau, which doesn't expose live utilization) — at least surface what's
// there, honestly reporting usage/memory as unavailable rather than faking a number.
fn read_gpus_from_lspci() -> Vec<GpuInfo> {
    let output = Command::new("lspci").args(["-mm"]).output();
    let Ok(output) = output else {
        return vec![];
    };
    if !output.status.success() {
        return vec![];
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter(|l| {
            l.contains("\"VGA compatible controller\"")
                || l.contains("\"3D controller\"")
                || l.contains("\"Display controller\"")
        })
        .map(|line| {
            let fields = parse_lspci_mm_line(line);
            let slot = fields.first().cloned().unwrap_or_default();
            let vendor = fields.get(2).cloned().unwrap_or_default();
            let device = fields.get(3).cloned().unwrap_or_default();
            GpuInfo {
                name: format!("{vendor} {device}").trim().to_string(),
                driver: driver_for_pci_slot(&slot),
                usage_percent: None,
                memory_used_mb: None,
                memory_total_mb: None,
                temperature: temp_for_pci_slot(&slot),
            }
        })
        .collect()
}

fn read_gpus() -> Vec<GpuInfo> {
    if let Some(gpus) = read_gpus_from_nvidia_smi() {
        if !gpus.is_empty() {
            return gpus;
        }
    }
    read_gpus_from_lspci()
}

#[tauri::command]
pub fn system_info_snapshot(state: tauri::State<SystemState>) -> SystemSnapshot {
    let cur_cpu = read_cpu_ticks();
    let per_core_len = cur_cpu.len().saturating_sub(1);
    let (usage_percent, per_core_percent) = {
        let mut prev_guard = state.prev_cpu.lock().unwrap();
        let result = match prev_guard.as_deref() {
            Some(prev) if prev.len() == cur_cpu.len() => {
                let overall = cpu_percent(&prev[0], &cur_cpu[0]);
                let per_core = prev[1..]
                    .iter()
                    .zip(cur_cpu[1..].iter())
                    .map(|(p, c)| cpu_percent(p, c))
                    .collect();
                (overall, per_core)
            }
            _ => (0.0, vec![0.0; per_core_len]),
        };
        *prev_guard = Some(cur_cpu);
        result
    };

    let (load_avg_1, load_avg_5, load_avg_15) = load_avg();
    let temperatures = read_cpu_temps();
    let cpu = CpuInfo {
        model: cpu_model(),
        core_count: per_core_len,
        usage_percent,
        per_core_percent,
        load_avg_1,
        load_avg_5,
        load_avg_15,
        temperature: primary_temp(&temperatures),
        temperatures,
    };

    let network = {
        let cur_net = read_net_bytes();
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

    let hostname = fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap_or_default()
        .trim()
        .to_string();
    let (os_name, os_version) = read_os_release();
    let kernel_version = fs::read_to_string("/proc/sys/kernel/osrelease")
        .unwrap_or_default()
        .trim()
        .to_string();

    SystemSnapshot {
        hostname,
        os_name,
        os_version,
        kernel_version,
        uptime_seconds: read_uptime(),
        cpu,
        memory: read_memory(),
        disks: read_disks(),
        network,
        gpus: read_gpus(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repo has no `tempfile` dev-dependency; the temp_dir + uuid pattern
    /// used by `workers` and `nginx_ssl::ca` is the house style.
    fn fake_hwmon(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devspro-hwmon-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        for (name, contents) in files {
            fs::write(dir.join(name), contents).unwrap();
        }
        dir
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
    fn reads_labelled_sensors_in_index_order_with_thresholds() {
        let dir = fake_hwmon(&[
            ("name", "coretemp\n"),
            ("temp1_input", "72000\n"),
            ("temp1_label", "Package id 0\n"),
            ("temp1_max", "100000\n"),
            ("temp1_crit", "100000\n"),
            ("temp2_input", "66000\n"),
            ("temp2_label", "Core 0\n"),
            // A stray non-temperature file must not be mistaken for a sensor.
            ("in0_input", "1200\n"),
        ]);
        let temps = read_hwmon_temps(&dir, "coretemp");
        fs::remove_dir_all(&dir).ok();

        assert_eq!(temps.len(), 2);
        assert_eq!(temps[0].label, "Package id 0");
        assert_eq!(temps[0].celsius, 72.0);
        assert_eq!(temps[0].high_c, Some(100.0));
        assert_eq!(temps[0].critical_c, Some(100.0));
        assert_eq!(temps[1].label, "Core 0");
        assert_eq!(temps[1].celsius, 66.0);
        assert_eq!(temps[1].high_c, None);
    }

    #[test]
    fn falls_back_to_the_driver_name_and_skips_zero_readings() {
        let dir = fake_hwmon(&[
            ("name", "k10temp\n"),
            ("temp1_input", "48000\n"),
            // An unpopulated sensor reads 0 and would otherwise show as the
            // coldest — or, on a machine with only these, as the CPU's temp.
            ("temp2_input", "0\n"),
        ]);
        let temps = read_hwmon_temps(&dir, "k10temp");
        fs::remove_dir_all(&dir).ok();

        assert_eq!(temps.len(), 1);
        assert_eq!(temps[0].label, "k10temp");
    }

    #[test]
    fn missing_hwmon_directory_yields_no_sensors() {
        let missing = std::env::temp_dir().join(format!("devspro-absent-{}", uuid::Uuid::new_v4()));
        assert!(read_hwmon_temps(&missing, "coretemp").is_empty());
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

