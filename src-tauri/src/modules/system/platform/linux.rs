//! Linux system sources. Reads `/proc` directly and shells out to a couple of
//! standard CLIs (`df`, `lspci`, optionally `nvidia-smi`) rather than pulling in a
//! system-info crate, matching how the rest of this app talks to the OS (see
//! `processes::mod`, `docker::mod`).

use std::collections::HashMap;
use std::fs;
use std::process::Command;

use super::super::{CpuTicks, DiskInfo, GpuInfo, MemoryInfo, OsIdentity};
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
                temperature_c: None,
            }
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
