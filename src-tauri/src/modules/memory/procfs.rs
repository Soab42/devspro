//! Everything this module knows about a process comes from `/proc`, read
//! directly — no `ps`, no `pgrep`. One scan is a few hundred small file reads,
//! where killer spawned `ps` once per app on every refresh. The parsers are pure
//! functions over the file contents so they can be tested against hand-written
//! samples.
//!
//! RSS comes from `VmRSS` in `status` (already read for the uid) rather than
//! `statm` × page size: same kernel counter, already in kB, and it avoids
//! guessing the page size, which isn't 4 KiB on every arm64 kernel.
//!
//! On Windows the readers at the bottom (`scan`, `read_proc`, `read_start_time`,
//! `my_uid`, `read_meminfo`) and the two constants come from `super::windows` instead,
//! filling the same `Proc` and `MemInfoKb`; the parsers stay, as plain functions.

// The parsers are only called from the Linux readers (and the tests) on Windows.
#![cfg_attr(windows, allow(dead_code))]

use super::MemoryInfo;
#[cfg(not(windows))]
use std::fs;
#[cfg(not(windows))]
use std::sync::OnceLock;

#[cfg(windows)]
pub use super::windows::{my_uid, read_meminfo, read_proc, read_start_time, scan, CLK_TCK, COMM_MAX};

/// Clock ticks per second for `utime`, `stime` and `starttime`. The kernel
/// exports these in USER_HZ, which is 100 on every architecture Linux userspace
/// runs on; `sysconf(_SC_CLK_TCK)` would say the same, and `libc` isn't a direct
/// dependency of this crate.
#[cfg(not(windows))]
pub const CLK_TCK: u64 = 100;

/// The kernel cuts `comm` to 15 bytes (TASK_COMM_LEN - 1), so
/// `gnome-session-binary` shows up as `gnome-session-b`.
#[cfg(not(windows))]
pub const COMM_MAX: usize = 15;

/// The fields of `/proc/<pid>/stat` this module uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    pub comm: String,
    pub state: char,
    pub ppid: u32,
    /// utime + stime, in clock ticks.
    pub ticks: u64,
    /// Field 22: when the process started, in clock ticks after boot. Together
    /// with the pid this names one process for good — a pid gets reused, a
    /// (pid, start time) pair doesn't.
    pub start_time: u64,
}

/// `comm` sits in parentheses and may itself contain spaces and parentheses (a
/// process can call itself `a) b (c`), so the fields after it are counted from
/// the *last* `)` rather than by splitting the whole line on whitespace.
pub fn parse_stat(raw: &str) -> Option<Stat> {
    let open = raw.find('(')?;
    let close = raw.rfind(')')?;
    if close < open {
        return None;
    }
    let comm = raw[open + 1..close].to_string();
    // rest[0] is field 3 (state), so field n is rest[n - 3].
    let rest: Vec<&str> = raw[close + 1..].split_whitespace().collect();
    let state = rest.first()?.chars().next()?;
    let ppid = rest.get(1)?.parse().ok()?;
    let utime: u64 = rest.get(11)?.parse().ok()?;
    let stime: u64 = rest.get(12)?.parse().ok()?;
    let start_time = rest.get(19)?.parse().ok()?;
    Some(Stat {
        comm,
        state,
        ppid,
        ticks: utime + stime,
        start_time,
    })
}

/// The fields of `/proc/<pid>/status` this module uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// The real uid, the first of the four on the `Uid:` line.
    pub uid: Option<u32>,
    /// Absent (so 0) for kernel threads and zombies.
    pub rss_kb: u64,
}

pub fn parse_status(raw: &str) -> Status {
    let mut status = Status::default();
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            status.uid = rest.split_whitespace().next().and_then(|v| v.parse().ok());
        } else if let Some(rest) = line.strip_prefix("VmRSS:") {
            status.rss_kb = rest.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0);
        }
    }
    status
}

/// `cmdline` is argv, NUL-separated. Returns the arguments joined with spaces —
/// what killer's `match` strings were written against, since they came from
/// `ps -o args` — and the basename of argv[0]. A login shell's `-zsh` keeps its
/// dash here; the keep list strips it.
///
/// Chromium-style processes overwrite argv with one space-separated string, so
/// when there is a single argument the program name is taken up to the first
/// space.
pub fn parse_cmdline(raw: &[u8]) -> (String, String) {
    let parts: Vec<String> = raw
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    let Some(first) = parts.first() else {
        return (String::new(), String::new());
    };
    let program = if parts.len() == 1 {
        first.split(' ').next().unwrap_or(first)
    } else {
        first.as_str()
    };
    let exe_name = program.rsplit('/').next().unwrap_or(program).to_string();
    (parts.join(" "), exe_name)
}

/// One process, as one scan saw it.
#[derive(Debug, Clone)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
    pub state: char,
    pub ticks: u64,
    pub start_time: u64,
    pub uid: Option<u32>,
    pub rss_kb: u64,
    pub cmdline: String,
    pub exe_name: String,
}

impl Proc {
    /// Kernel threads have an empty cmdline, and so does a zombie — which has
    /// already given its memory back and can't be signalled into exiting.
    pub fn is_live_userspace(&self) -> bool {
        !self.cmdline.is_empty() && self.state != 'Z' && self.state != 'X'
    }
}

/// `None` when the process is gone (or vanished halfway through being read).
#[cfg(not(windows))]
pub fn read_proc(pid: u32) -> Option<Proc> {
    let stat = parse_stat(&fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)?;
    let status = parse_status(&fs::read_to_string(format!("/proc/{pid}/status")).ok()?);
    let (cmdline, exe_name) = parse_cmdline(&fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default());
    Some(Proc {
        pid,
        ppid: stat.ppid,
        comm: stat.comm,
        state: stat.state,
        ticks: stat.ticks,
        start_time: stat.start_time,
        uid: status.uid,
        rss_kb: status.rss_kb,
        cmdline,
        exe_name,
    })
}

/// Every process visible in `/proc`, in pid order. Empty where there is no
/// `/proc` (macOS).
#[cfg(not(windows))]
pub fn scan() -> Vec<Proc> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids: Vec<u32> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse().ok()))
        .collect();
    pids.sort_unstable();
    pids.into_iter().filter_map(read_proc).collect()
}

/// Just the start time — the identity re-read right before a signal.
#[cfg(not(windows))]
pub fn read_start_time(pid: u32) -> Option<u64> {
    parse_stat(&fs::read_to_string(format!("/proc/{pid}/stat")).ok()?).map(|s| s.start_time)
}

/// The real uid DevsPro runs as. `None` on macOS, which leaves the detected
/// list and the full reset empty.
#[cfg(not(windows))]
pub fn my_uid() -> Option<u32> {
    static UID: OnceLock<Option<u32>> = OnceLock::new();
    *UID.get_or_init(|| fs::read_to_string("/proc/self/status").ok().and_then(|s| parse_status(&s).uid))
}

/// The `/proc/meminfo` lines this module uses, in kB.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemInfoKb {
    pub total: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

pub fn parse_meminfo(raw: &str) -> MemInfoKb {
    let mut info = MemInfoKb::default();
    for line in raw.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let value = rest.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0);
        match key {
            "MemTotal" => info.total = value,
            "MemAvailable" => info.available = value,
            "SwapTotal" => info.swap_total = value,
            "SwapFree" => info.swap_free = value,
            _ => {}
        }
    }
    info
}

#[cfg(not(windows))]
pub fn read_meminfo() -> MemInfoKb {
    parse_meminfo(&fs::read_to_string("/proc/meminfo").unwrap_or_default())
}

impl MemInfoKb {
    /// `used_pct` is (MemTotal - MemAvailable) / MemTotal — what killer showed
    /// (via `free`), and what the watchdog compares against its threshold.
    /// "Available" counts reclaimable cache as free, so a full page cache alone
    /// never trips it.
    pub fn summary(&self) -> MemoryInfo {
        let available = self.available.min(self.total);
        let used_pct = if self.total == 0 {
            0
        } else {
            ((self.total - available) * 100 / self.total) as u8
        };
        MemoryInfo {
            total_mb: self.total / 1024,
            available_mb: available / 1024,
            used_pct,
            swap_total_mb: self.swap_total / 1024,
            swap_used_mb: self.swap_total.saturating_sub(self.swap_free) / 1024,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Everything after the comm, shaped like a Chrome renderer's line.
    const STAT_TAIL: &str = "S 8954 8858 8858 0 -1 4194560 3214 0 0 0 1234 567 0 0 20 0 18 0 98765 1234567 4321 18446744073709551615";

    #[test]
    fn parses_a_plain_stat_line() {
        let s = parse_stat(&format!("8977 (chrome) {STAT_TAIL}")).unwrap();
        assert_eq!(s.comm, "chrome");
        assert_eq!(s.state, 'S');
        assert_eq!(s.ppid, 8954);
        assert_eq!(s.ticks, 1234 + 567);
        assert_eq!(s.start_time, 98765);
    }

    #[test]
    fn comm_with_spaces_and_parentheses_does_not_shift_the_fields() {
        let s = parse_stat(&format!("42 (Web Content) {STAT_TAIL}")).unwrap();
        assert_eq!(s.comm, "Web Content");
        assert_eq!(s.start_time, 98765);

        let s = parse_stat(&format!("42 (a) b (c)) {STAT_TAIL}")).unwrap();
        assert_eq!(s.comm, "a) b (c)");
        assert_eq!(s.ppid, 8954);
        assert_eq!(s.ticks, 1801);
        assert_eq!(s.start_time, 98765);
    }

    #[test]
    fn truncated_or_garbled_stat_is_rejected() {
        assert_eq!(parse_stat("42 (x) S 1 2 3"), None);
        assert_eq!(parse_stat("no parens at all"), None);
        assert_eq!(parse_stat(""), None);
    }

    #[test]
    fn status_gives_real_uid_and_rss() {
        let raw = "Name:\tchrome\nUmask:\t0002\nState:\tS (sleeping)\nUid:\t1000\t0\t0\t0\nGid:\t1000\t1000\t1000\t1000\nVmRSS:\t  418692 kB\nThreads:\t30\n";
        assert_eq!(parse_status(raw), Status { uid: Some(1000), rss_kb: 418692 });
        // Kernel threads have no VmRSS line.
        assert_eq!(parse_status("Name:\tkworker/0:1\nUid:\t0\t0\t0\t0\n"), Status { uid: Some(0), rss_kb: 0 });
    }

    #[test]
    fn cmdline_joins_argv_and_names_the_program() {
        let (cmd, exe) = parse_cmdline(b"/usr/bin/php\0-S\0[::]:8080\0-t\0/var/www/html\0");
        assert_eq!(cmd, "/usr/bin/php -S [::]:8080 -t /var/www/html");
        assert_eq!(exe, "php");

        // Chromium rewrites argv into one string.
        let (cmd, exe) = parse_cmdline(b"/opt/google/chrome/chrome --type=renderer --lang=en-US\0");
        assert_eq!(cmd, "/opt/google/chrome/chrome --type=renderer --lang=en-US");
        assert_eq!(exe, "chrome");

        assert_eq!(parse_cmdline(b"-zsh\0").1, "-zsh");
        assert_eq!(parse_cmdline(b""), (String::new(), String::new()));
    }

    #[test]
    fn meminfo_summary_uses_available_not_free() {
        let raw = "MemTotal:       16066572 kB\nMemFree:         1188276 kB\nMemAvailable:    5620872 kB\nBuffers:           88876 kB\nSwapTotal:      20260340 kB\nSwapFree:       12845860 kB\n";
        let kb = parse_meminfo(raw);
        assert_eq!(
            kb,
            MemInfoKb { total: 16066572, available: 5620872, swap_total: 20260340, swap_free: 12845860 }
        );
        let info = kb.summary();
        assert_eq!(info.total_mb, 15690);
        assert_eq!(info.available_mb, 5489);
        assert_eq!(info.used_pct, 65); // (16066572 - 5620872) * 100 / 16066572 = 65.01
        assert_eq!(info.swap_total_mb, 19785);
        assert_eq!(info.swap_used_mb, 7240);
    }

    #[test]
    fn empty_meminfo_does_not_divide_by_zero() {
        let info = parse_meminfo("").summary();
        assert_eq!(info.used_pct, 0);
        assert_eq!(info.total_mb, 0);
    }
}
