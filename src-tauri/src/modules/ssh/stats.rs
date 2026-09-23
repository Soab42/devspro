//! The status strip under a terminal: what the machine behind the focused
//! pane is doing, MobaXterm-style. One POSIX script reads `/proc`, `df` and
//! `who`; it runs through `sh` for a local pane and through an exec channel
//! on the SFTP session's SSH connection for a remote one, so the two produce
//! the same shape. Counters (CPU ticks, network bytes) come back raw and the
//! frontend turns consecutive samples into percentages and rates.

use serde::Serialize;
use std::process::Command;
use tauri::State;

use super::SshState;

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct DiskUsage {
    pub mount: String,
    pub total_kb: u64,
    pub used_kb: u64,
    pub percent: f32,
}

/// One sample. `cpu_idle` / `cpu_total` and `rx_bytes` / `tx_bytes` are
/// cumulative since boot; everything else is a point-in-time value.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct HostSample {
    pub hostname: String,
    pub cpu_idle: u64,
    pub cpu_total: u64,
    pub mem_total_kb: u64,
    pub mem_available_kb: u64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
    pub load1: f32,
    pub uptime_seconds: u64,
    pub users: u32,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub disks: Vec<DiskUsage>,
}

/// Sections are fenced with `@@name` lines so a missing tool (no `who`, no
/// `df -x`) blanks one section rather than shifting the rest. Every command
/// tolerates failure: the strip degrades to what it could read.
pub const STATS_SCRIPT: &str = r#"echo @@host; hostname 2>/dev/null || cat /etc/hostname 2>/dev/null
echo @@cpu; head -n 1 /proc/stat 2>/dev/null
echo @@mem; grep -E '^(MemTotal|MemAvailable|MemFree|SwapTotal|SwapFree):' /proc/meminfo 2>/dev/null
echo @@load; cat /proc/loadavg 2>/dev/null
echo @@uptime; cat /proc/uptime 2>/dev/null
echo @@users; who 2>/dev/null | wc -l
echo @@net; cat /proc/net/dev 2>/dev/null
echo @@df; df -P -k -x tmpfs -x devtmpfs -x squashfs -x overlay -x efivarfs 2>/dev/null || df -P -k 2>/dev/null
"#;

/// Turns the script's output into a sample. Tolerant by design: a section
/// that is missing or malformed leaves its fields at zero.
pub fn parse_sample(output: &str) -> HostSample {
    let mut sample = HostSample::default();
    let mut section = "";
    let mut mem_free = 0u64;
    let mut have_available = false;
    for line in output.lines() {
        let line = line.trim_end();
        if let Some(name) = line.strip_prefix("@@") {
            section = name.trim();
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        match section {
            "host" => {
                if sample.hostname.is_empty() {
                    sample.hostname = words[0].to_string();
                }
            }
            "cpu" => {
                if words[0] == "cpu" {
                    let ticks: Vec<u64> = words[1..].iter().filter_map(|w| w.parse().ok()).collect();
                    sample.cpu_total = ticks.iter().sum();
                    // idle + iowait, the same pair the system module treats as idle.
                    sample.cpu_idle = ticks.get(3).copied().unwrap_or(0) + ticks.get(4).copied().unwrap_or(0);
                }
            }
            "mem" => {
                let value: u64 = words.get(1).and_then(|w| w.parse().ok()).unwrap_or(0);
                match words[0] {
                    "MemTotal:" => sample.mem_total_kb = value,
                    "MemAvailable:" => {
                        sample.mem_available_kb = value;
                        have_available = true;
                    }
                    "MemFree:" => mem_free = value,
                    "SwapTotal:" => sample.swap_total_kb = value,
                    "SwapFree:" => sample.swap_free_kb = value,
                    _ => {}
                }
            }
            "load" => sample.load1 = words[0].parse().unwrap_or(0.0),
            "uptime" => sample.uptime_seconds = words[0].parse::<f64>().unwrap_or(0.0) as u64,
            "users" => sample.users = words[0].parse().unwrap_or(0),
            "net" => {
                // "eth0: rx_bytes rx_packets ... (8 fields) tx_bytes ..." — the
                // interface name may or may not be glued to the first number.
                let (name, rest) = match line.split_once(':') {
                    Some(pair) => pair,
                    None => continue,
                };
                let name = name.trim();
                if name == "lo" || name.starts_with("docker") || name.starts_with("veth") || name.starts_with("br-") {
                    continue;
                }
                let nums: Vec<u64> = rest.split_whitespace().filter_map(|w| w.parse().ok()).collect();
                if nums.len() >= 9 {
                    sample.rx_bytes += nums[0];
                    sample.tx_bytes += nums[8];
                }
            }
            "df" => {
                // Filesystem 1024-blocks Used Available Capacity Mounted on
                if words.len() < 6 || words[0] == "Filesystem" {
                    continue;
                }
                let total_kb: u64 = words[1].parse().unwrap_or(0);
                let used_kb: u64 = words[2].parse().unwrap_or(0);
                if total_kb == 0 {
                    continue;
                }
                let percent = words[4].trim_end_matches('%').parse().unwrap_or(0.0);
                sample.disks.push(DiskUsage { mount: words[5..].join(" "), total_kb, used_kb, percent });
            }
            _ => {}
        }
    }
    // Old kernels have no MemAvailable; MemFree is the honest fallback.
    if !have_available {
        sample.mem_available_kb = mem_free;
    }
    sample
}

fn run_local() -> Result<String, String> {
    let out = Command::new("sh")
        .arg("-c")
        .arg(STATS_SCRIPT)
        .output()
        .map_err(|e| format!("could not run sh: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A sample of the local machine, or of the host behind an SFTP session.
#[tauri::command(async)]
pub fn term_host_stats(state: State<SshState>, session: Option<String>) -> Result<HostSample, String> {
    let output = match session {
        None => run_local()?,
        Some(id) => super::sftp::exec_on_session(&state, &id, STATS_SCRIPT)?,
    };
    Ok(parse_sample(&output))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "@@host\nvmi2875812\n@@cpu\ncpu  1000 0 500 8000 100 0 20 0 0 0\ncpu0 1 2 3 4 5 6 7 0 0 0\n@@mem\nMemTotal:       12247040 kB\nMemFree:         3000000 kB\nMemAvailable:    5600000 kB\nSwapTotal:       2097148 kB\nSwapFree:        2000000 kB\n@@load\n0.45 0.30 0.20 1/300 12345\n@@uptime\n8553600.55 34000000.00\n@@users\n3\n@@net\nInter-|   Receive |  Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n    lo: 500 1 0 0 0 0 0 0 500 1 0 0 0 0 0 0\n  eth0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0\ndocker0: 99 0 0 0 0 0 0 0 99 0 0 0 0 0 0 0\n@@df\nFilesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda1 100000 59000 41000 59% /\n/dev/sda2 1000 150 850 15% /boot\n/dev/sda3 500 30 470 6% /boot/efi\n";

    #[test]
    fn parses_every_section() {
        let s = parse_sample(SAMPLE);
        assert_eq!(s.hostname, "vmi2875812");
        assert_eq!((s.cpu_total, s.cpu_idle), (9620, 8100));
        assert_eq!((s.mem_total_kb, s.mem_available_kb, s.swap_total_kb, s.swap_free_kb), (12247040, 5600000, 2097148, 2000000));
        assert_eq!(s.load1, 0.45);
        assert_eq!(s.uptime_seconds, 8553600);
        assert_eq!(s.users, 3);
        assert_eq!((s.rx_bytes, s.tx_bytes), (1000, 2000));
        assert_eq!(s.disks.len(), 3);
        assert_eq!(s.disks[0], DiskUsage { mount: "/".into(), total_kb: 100000, used_kb: 59000, percent: 59.0 });
        assert_eq!(s.disks[2].mount, "/boot/efi");
    }

    #[test]
    fn missing_sections_leave_zeros_and_memfree_stands_in_for_memavailable() {
        let s = parse_sample("@@host\nbox\n@@mem\nMemTotal: 100 kB\nMemFree: 40 kB\n@@df\n");
        assert_eq!(s.hostname, "box");
        assert_eq!(s.mem_available_kb, 40);
        assert_eq!(s.cpu_total, 0);
        assert!(s.disks.is_empty());
        assert_eq!(parse_sample(""), HostSample::default());
    }

    #[test]
    fn local_script_runs_here() {
        let s = parse_sample(&run_local().unwrap());
        assert!(!s.hostname.is_empty());
        assert!(s.cpu_total > 0);
        assert!(s.mem_total_kb > 0);
    }
}
