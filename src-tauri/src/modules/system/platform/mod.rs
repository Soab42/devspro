//! Per-OS system sources behind one contract, mirroring how `elevate::platform`
//! splits the elevation launcher. Each backend below implements exactly the same
//! set of free functions, and `super::system_info_snapshot` does all the
//! platform-independent work (rate math against the previous sample, assembling
//! the snapshot) on top of them:
//!
//! | | |
//! | --- | --- |
//! | `cpu_sample()` | Cumulative CPU ticks, aggregate first then one entry per logical core, plus the core count. |
//! | `cpu_model()` | Marketing name of the CPU. Never empty — fall back to `"Unknown CPU"`. |
//! | `load_avg()` | 1/5/15-minute load, or `None` where the OS has no such concept. |
//! | `memory()` | Physical and swap totals in KiB. |
//! | `uptime_seconds()` | Seconds since boot. |
//! | `os_identity()` | Hostname, OS name/version, kernel version. |
//! | `disks()` | One entry per real (non-pseudo) mounted filesystem. |
//! | `net_bytes()` | Cumulative rx/tx bytes keyed by interface name. |
//! | `gpus()` | Display adapters, measurements left `None` when unmeasurable. |
//! | `cpu_temps()` | Every CPU thermal sensor, package first. Empty where none can be read. |
//!
//! Two rules keep the contract honest, because a dashboard that invents numbers is
//! worse than one that admits it can't see them:
//!
//! - Every function returns *something* — no `Result`, no panics. A source that
//!   can't be read yields an empty vec / zero / `None`, never a crash: these run
//!   inside a `#[tauri::command]`, and a panic there cannot unwind across the FFI
//!   boundary, so it aborts the whole app rather than failing one poll.
//! - Nothing is faked. `load_avg()` returns `None` on Windows instead of zeros,
//!   and unmeasurable GPU fields stay `None`, so the UI can say "unavailable".

use super::CpuTicks;

mod nvidia;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

/// One reading of the CPU's cumulative counters.
pub struct CpuSample {
    /// The aggregate ("all cores") entry first, then one entry per logical core.
    /// May be empty, and may hold the aggregate alone on a platform that can't
    /// sample per-core — `system_info_snapshot` handles both.
    pub ticks: Vec<CpuTicks>,
    /// Logical core count, reported separately rather than derived from `ticks`
    /// so it stays correct even where per-core sampling is unavailable.
    pub core_count: usize,
}
