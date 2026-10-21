//! Linux system sources. Reads `/proc` and `/sys` directly and shells out to a couple of
//! standard CLIs (`df`, `lspci`, optionally `nvidia-smi`) rather than pulling in a
//! system-info crate, matching how the rest of this app talks to the OS (see
//! `processes::mod`, `docker::mod`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use super::super::{primary_temp, CpuTicks, DiskInfo, GpuInfo, MemoryInfo, OsIdentity, TemperatureReading};
use super::CpuSample;

pub fn cpu_sample() -> CpuSample {
    let ticks = read_cpu_ticks();
    // `/proc/stat`'s aggregate "cpu" line comes first, then one "cpuN" line per
    // logical core — so the per-core count is everything after the aggregate.
    let core_count = ticks.len().saturating_sub(1);
    CpuSample { ticks, core_count }
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

pub fn cpu_model() -> String {
    let content = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    content
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string())
}

pub fn load_avg() -> Option<(f32, f32, f32)> {
    let content = fs::read_to_string("/proc/loadavg").ok()?;
    let mut parts = content.split_whitespace();
    let mut next = || parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    Some((next(), next(), next()))
}

pub fn memory() -> MemoryInfo {
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

pub fn uptime_seconds() -> u64 {
    fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_string))
        .and_then(|v| v.parse::<f64>().ok())
        .map(|v| v as u64)
        .unwrap_or(0)
}

pub fn os_identity() -> OsIdentity {
    let (os_name, os_version) = read_os_release();
    OsIdentity {
        hostname: fs::read_to_string("/proc/sys/kernel/hostname")
            .unwrap_or_default()
            .trim()
            .to_string(),
        os_name,
        os_version,
        kernel_version: fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
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

pub fn disks() -> Vec<DiskInfo> {
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

pub fn cpu_temps() -> Vec<TemperatureReading> {
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

pub fn net_bytes() -> HashMap<String, (u64, u64)> {
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

/// `(name, driver, pci slot)` per display adapter. The hardware doesn't change
/// while the app runs, and `lspci` is a process spawn plus a walk of the PCI bus,
/// so it is read once rather than on every 2-second poll. The live temperature is
/// still read from `/sys` each time.
fn lspci_display_adapters() -> &'static [(String, Option<String>, String)] {
    static ADAPTERS: OnceLock<Vec<(String, Option<String>, String)>> = OnceLock::new();
    ADAPTERS.get_or_init(read_lspci_display_adapters)
}

// Fallback for GPUs without a queryable vendor tool (e.g. an open-source driver
// like nouveau, which doesn't expose live utilization) — at least surface what's
// there, honestly reporting usage/memory as unavailable rather than faking a number.
fn read_lspci_display_adapters() -> Vec<(String, Option<String>, String)> {
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
            let name = format!("{vendor} {device}").trim().to_string();
            let driver = driver_for_pci_slot(&slot);
            (name, driver, slot)
        })
        .collect()
}

fn read_gpus_from_lspci() -> Vec<GpuInfo> {
    lspci_display_adapters()
        .iter()
        .map(|(name, driver, slot)| GpuInfo {
            name: name.clone(),
            driver: driver.clone(),
            usage_percent: None,
            memory_used_mb: None,
            memory_total_mb: None,
            temperature: temp_for_pci_slot(slot),
        })
        .collect()
}

pub fn gpus() -> Vec<GpuInfo> {
    if let Some(gpus) = super::nvidia::query() {
        if !gpus.is_empty() {
            return gpus;
        }
    }
    read_gpus_from_lspci()
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
    }
}
