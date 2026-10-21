//! macOS system sources. Most figures come from `sysctlbyname`, which is the same data
//! the `sysctl` CLI prints but without a process spawn per field — the dashboard polls
//! every couple of seconds, and shelling out ten times per poll would cost more than
//! everything else this app does. `vm_stat`, `df` and `mount` stay as CLIs (Linux
//! shells out to `df` too), and `sw_vers` is read once and cached, since the product
//! name and version cannot change without a reboot.
//!
//! Unlike the Linux and Windows backends, this one has not been exercised on real
//! hardware — it is written from documented interfaces. Per-core CPU is the one
//! deliberate gap: it needs `host_processor_info`, whose allocation has to be handed
//! back with `vm_deallocate`, and that is not worth writing blind. `cpu_sample()`
//! therefore reports the aggregate only, and the core count separately, so the
//! dashboard's CPU percentage and core count are both right while the per-core
//! breakdown is simply absent.

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CString};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::{CpuTicks, DiskInfo, GpuInfo, MemoryInfo, OsIdentity, TemperatureReading};
use super::CpuSample;

// ---------------------------------------------------------------------------
// CPU
// ---------------------------------------------------------------------------

pub fn cpu_sample() -> CpuSample {
    // `available_parallelism` is `hw.logicalcpu` on this platform, without the FFI.
    let core_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    CpuSample {
        ticks: host_cpu_ticks().into_iter().collect(),
        core_count,
    }
}

/// Mach's aggregate CPU tick counters — the closest equivalent of `/proc/stat`'s
/// leading "cpu" line.
fn host_cpu_ticks() -> Option<CpuTicks> {
    const HOST_CPU_LOAD_INFO: c_int = 3;
    const CPU_STATE_MAX: usize = 4;
    const CPU_STATE_IDLE: usize = 2;

    // `host_info_t` is an array of `natural_t`, and the count is in those units rather
    // than in bytes — so this is 4, not 16.
    let mut ticks = [0u32; CPU_STATE_MAX];
    let mut count = CPU_STATE_MAX as u32;

    // SAFETY: both are libSystem entry points, always linked on macOS. `ticks` is a
    // correctly-aligned array of exactly `count` natural_t, which is what
    // HOST_CPU_LOAD_INFO writes; `count` is a valid in/out parameter.
    let status = unsafe {
        host_statistics(
            mach_host_self(),
            HOST_CPU_LOAD_INFO,
            ticks.as_mut_ptr(),
            &mut count,
        )
    };
    if status != 0 {
        return None;
    }

    // user + system + idle + nice, with idle called out — the same idle-versus-total
    // split the shared rate math does on every platform.
    Some(CpuTicks {
        idle: u64::from(ticks[CPU_STATE_IDLE]),
        total: ticks.iter().map(|&t| u64::from(t)).sum(),
    })
}

pub fn cpu_model() -> String {
    // Present on Apple Silicon too, where it reads "Apple M2 Pro" rather than an Intel
    // part number.
    sysctl_string("machdep.cpu.brand_string")
        .or_else(|| sysctl_string("hw.model"))
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| "Unknown CPU".to_string())
}

pub fn load_avg() -> Option<(f32, f32, f32)> {
    /// `struct loadavg` from `<sys/resource.h>`: three fixed-point averages plus the
    /// scale they're expressed in.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct LoadAvg {
        ldavg: [u32; 3],
        fscale: u64,
    }

    let raw: LoadAvg = sysctl_struct("vm.loadavg")?;
    if raw.fscale == 0 {
        return None;
    }
    let scale = raw.fscale as f32;
    Some((
        raw.ldavg[0] as f32 / scale,
        raw.ldavg[1] as f32 / scale,
        raw.ldavg[2] as f32 / scale,
    ))
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

pub fn memory() -> MemoryInfo {
    let total = sysctl_u64("hw.memsize").unwrap_or(0);
    let available = available_bytes_from_vm_stat().unwrap_or(0).min(total);
    let (swap_total, swap_used) = swap_usage();

    MemoryInfo {
        total_kb: to_kb(total),
        used_kb: to_kb(total.saturating_sub(available)),
        available_kb: to_kb(available),
        swap_total_kb: to_kb(swap_total),
        swap_used_kb: to_kb(swap_used),
    }
}

/// macOS has no single "available memory" counter the way Linux's `MemAvailable` is.
/// Free, speculative and inactive pages are the ones the kernel can hand to a new
/// allocation without swapping, which makes their sum the honest analogue — and keeps
/// `used = total - available` meaning the same thing it does on the other platforms.
fn available_bytes_from_vm_stat() -> Option<u64> {
    let output = Command::new("vm_stat").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(parse_vm_stat(&text))
}

fn parse_vm_stat(text: &str) -> u64 {
    let mut lines = text.lines();

    // "Mach Virtual Memory Statistics: (page size of 16384 bytes)"
    let page_size = lines
        .next()
        .and_then(|header| header.split_once("page size of "))
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .and_then(|size| size.parse::<u64>().ok())
        .unwrap_or(4096);

    let pages: u64 = lines
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let key = key.trim();
            if !matches!(
                key,
                "Pages free" | "Pages speculative" | "Pages inactive"
            ) {
                return None;
            }
            // Every count is printed with a trailing full stop.
            value.trim().trim_end_matches('.').parse::<u64>().ok()
        })
        .sum();

    pages.saturating_mul(page_size)
}

/// `struct xsw_usage` from `<sys/sysctl.h>` — the swap figures Activity Monitor shows.
fn swap_usage() -> (u64, u64) {
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct XswUsage {
        total: u64,
        avail: u64,
        used: u64,
        page_size: u32,
        encrypted: u32,
    }

    match sysctl_struct::<XswUsage>("vm.swapusage") {
        Some(usage) => (usage.total, usage.used),
        None => (0, 0),
    }
}

fn to_kb(bytes: u64) -> u64 {
    bytes / 1024
}

pub fn uptime_seconds() -> u64 {
    /// `struct timeval` as macOS lays it out on 64-bit: `time_t` is 64-bit, and
    /// `suseconds_t` stays 32-bit with the struct padded out to 16 bytes.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct TimeVal {
        sec: i64,
        usec: i32,
    }

    let Some(boot) = sysctl_struct::<TimeVal>("kern.boottime") else {
        return 0;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_sub(boot.sec.max(0) as u64)
}

// ---------------------------------------------------------------------------
// OS identity
// ---------------------------------------------------------------------------

pub fn os_identity() -> OsIdentity {
    let (os_name, os_version) = product_info();
    OsIdentity {
        hostname: sysctl_string("kern.hostname").unwrap_or_default(),
        os_name,
        os_version,
        kernel_version: sysctl_string("kern.osrelease").unwrap_or_default(),
    }
}

/// Read once for the life of the process: `sw_vers` is two spawns, and neither the
/// product name nor its version can change without a reboot.
fn product_info() -> (String, String) {
    static PRODUCT: OnceLock<(String, String)> = OnceLock::new();
    PRODUCT
        .get_or_init(|| {
            let name = sw_vers("-productName").unwrap_or_else(|| "macOS".to_string());
            let version = sw_vers("-productVersion").unwrap_or_default();
            (name, version)
        })
        .clone()
}

fn sw_vers(flag: &str) -> Option<String> {
    let output = Command::new("sw_vers").arg(flag).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

// ---------------------------------------------------------------------------
// Disks
// ---------------------------------------------------------------------------

pub fn disks() -> Vec<DiskInfo> {
    let types = filesystem_types();
    let Some(output) = run("df", &["-k", "-P"]) else {
        return Vec::new();
    };
    parse_df(&output, &types)
}

/// `df -P` guarantees the POSIX one-line-per-filesystem format, but macOS's `df` has no
/// column for the filesystem type — so it comes from `mount` instead, keyed by mount
/// point.
fn parse_df(text: &str, types: &HashMap<String, String>) -> Vec<DiskInfo> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 6 {
                return None;
            }
            let device = cols[0].to_string();
            // Same rule the Linux backend uses to drop pseudo-filesystems: a real disk
            // is mounted from a device node. Skips `devfs`, `map auto_home` and the
            // various synthetic mounts macOS layers under /System/Volumes.
            if !device.starts_with('/') {
                return None;
            }
            let mount_point = cols[5..].join(" ");
            // Columns are 1 KiB blocks under `-k`.
            let kb = |index: usize| cols[index].parse::<u64>().unwrap_or(0) * 1024;
            Some(DiskInfo {
                fs_type: types.get(&mount_point).cloned().unwrap_or_default(),
                device,
                total_bytes: kb(1),
                used_bytes: kb(2),
                available_bytes: kb(3),
                mount_point,
            })
        })
        .collect()
}

/// Mount point → filesystem type, from `mount`'s
/// `/dev/disk3s1s1 on / (apfs, sealed, local, read-only, journaled)` lines.
fn filesystem_types() -> HashMap<String, String> {
    match run("mount", &[]) {
        Some(output) => parse_mount(&output),
        None => HashMap::new(),
    }
}

fn parse_mount(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (_, rest) = line.split_once(" on ")?;
            let (mount_point, options) = rest.rsplit_once(" (")?;
            let fs_type = options.trim_end_matches(')').split(',').next()?;
            Some((mount_point.to_string(), fs_type.trim().to_string()))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

pub fn net_bytes() -> HashMap<String, (u64, u64)> {
    match run("netstat", &["-i", "-b", "-n"]) {
        Some(output) => parse_netstat(&output),
        None => HashMap::new(),
    }
}

/// `netstat -ibn` prints one row per interface *per address family*, all carrying the
/// same interface-wide byte totals — so the first row for a given name wins and the
/// duplicates are ignored rather than summed.
fn parse_netstat(text: &str) -> HashMap<String, (u64, u64)> {
    let mut header: Vec<&str> = Vec::new();
    let mut interfaces: HashMap<String, (u64, u64)> = HashMap::new();

    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.first() == Some(&"Name") {
            header = cols;
            continue;
        }
        // Column positions shift between macOS releases, so they're looked up by name
        // from the header rather than hardcoded.
        let (Some(rx_at), Some(tx_at)) = (
            header.iter().position(|&col| col == "Ibytes"),
            header.iter().position(|&col| col == "Obytes"),
        ) else {
            continue;
        };
        let (Some(name), Some(rx), Some(tx)) = (cols.first(), cols.get(rx_at), cols.get(tx_at))
        else {
            continue;
        };
        // The dashboard filters Linux's loopback out by name, and macOS calls it "lo0".
        if name.starts_with("lo") {
            continue;
        }
        let (Ok(rx), Ok(tx)) = (rx.parse::<u64>(), tx.parse::<u64>()) else {
            continue;
        };
        interfaces.entry((*name).to_string()).or_insert((rx, tx));
    }

    interfaces
}

// ---------------------------------------------------------------------------
// GPUs
// ---------------------------------------------------------------------------

/// Left empty on purpose. The only way to enumerate display hardware on macOS is
/// `system_profiler SPDisplaysDataType`, which takes seconds to return — unusable in
/// something polled every two seconds — and it reports no live utilization anyway. An
/// empty list makes the dashboard omit the GPU section, which is the honest outcome:
/// better than blocking each poll for a name and no measurements.
pub fn gpus() -> Vec<GpuInfo> {
    Vec::new()
}

// ---------------------------------------------------------------------------
// sysctl / process helpers
// ---------------------------------------------------------------------------

extern "C" {
    fn sysctlbyname(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *const c_void,
        newlen: usize,
    ) -> c_int;

    fn mach_host_self() -> u32;

    fn host_statistics(
        host_priv: u32,
        flavor: c_int,
        host_info_out: *mut u32,
        host_info_out_count: *mut u32,
    ) -> c_int;
}

fn sysctl_bytes(name: &str) -> Option<Vec<u8>> {
    let name = CString::new(name).ok()?;
    let mut len: usize = 0;

    // SAFETY: a null destination asks for the value's size without writing it, which is
    // the documented way to size the buffer below.
    let rc = unsafe {
        sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null(),
            0,
        )
    };
    if rc != 0 || len == 0 {
        return None;
    }

    let mut buf = vec![0u8; len];
    // SAFETY: `buf` is exactly `len` writable bytes, the size just reported.
    let rc = unsafe {
        sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    buf.truncate(len);
    Some(buf)
}

fn sysctl_string(name: &str) -> Option<String> {
    let bytes = sysctl_bytes(name)?;
    let text = String::from_utf8_lossy(&bytes);
    Some(text.trim_end_matches('\0').trim().to_string())
}

fn sysctl_u64(name: &str) -> Option<u64> {
    let bytes = sysctl_bytes(name)?;
    match bytes.len() {
        8 => Some(u64::from_le_bytes(bytes[..8].try_into().ok()?)),
        // Some integer sysctls are 32-bit (`hw.logicalcpu`), some 64 (`hw.memsize`).
        4 => Some(u64::from(u32::from_le_bytes(bytes[..4].try_into().ok()?))),
        _ => None,
    }
}

/// Reads a fixed-layout `#[repr(C)]` sysctl value. Copied out of the byte buffer rather
/// than transmuted in place, since `Vec<u8>` carries no alignment guarantee for `T`.
fn sysctl_struct<T: Copy>(name: &str) -> Option<T> {
    let bytes = sysctl_bytes(name)?;
    if bytes.len() < std::mem::size_of::<T>() {
        return None;
    }
    // SAFETY: `T` is a plain `#[repr(C)]` data struct declared to match this sysctl's
    // documented layout, and the buffer holds at least that many bytes. `read_unaligned`
    // is what makes the copy sound regardless of where the Vec's allocation landed.
    Some(unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<T>()) })
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_the_reclaimable_page_classes_at_the_reported_page_size() {
        let text = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                    Pages free:                               10.\n\
                    Pages active:                          99999.\n\
                    Pages inactive:                           20.\n\
                    Pages speculative:                        30.\n\
                    Pages wired down:                      88888.\n";
        // free + inactive + speculative = 60 pages, active/wired deliberately excluded.
        assert_eq!(parse_vm_stat(text), 60 * 16384);
    }

    #[test]
    fn falls_back_to_a_4k_page_when_the_header_is_unrecognised() {
        let text = "something else entirely\nPages free: 2.\n";
        assert_eq!(parse_vm_stat(text), 2 * 4096);
    }

    #[test]
    fn reads_filesystem_types_out_of_mount_output() {
        let text = "/dev/disk3s1s1 on / (apfs, sealed, local, read-only, journaled)\n\
                    devfs on /dev (devfs, nobrowse)\n\
                    map auto_home on /System/Volumes/Data/home (autofs, automounted)\n";
        let types = parse_mount(text);
        assert_eq!(types.get("/").map(String::as_str), Some("apfs"));
        assert_eq!(types.get("/dev").map(String::as_str), Some("devfs"));
        assert_eq!(
            types.get("/System/Volumes/Data/home").map(String::as_str),
            Some("autofs")
        );
    }

    #[test]
    fn keeps_only_device_backed_filesystems_and_tags_them_with_their_type() {
        let df = "Filesystem 1024-blocks      Used Available Capacity Mounted on\n\
                  /dev/disk3s1s1  971350180 20000000 900000000      3% /\n\
                  devfs                 197      197         0     100% /dev\n\
                  map auto_home           0        0         0     100% /System/Volumes/Data/home\n";
        let mut types = HashMap::new();
        types.insert("/".to_string(), "apfs".to_string());

        let disks = parse_df(df, &types);
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].mount_point, "/");
        assert_eq!(disks[0].device, "/dev/disk3s1s1");
        assert_eq!(disks[0].fs_type, "apfs");
        assert_eq!(disks[0].total_bytes, 971_350_180 * 1024);
        assert_eq!(disks[0].used_bytes, 20_000_000 * 1024);
        assert_eq!(disks[0].available_bytes, 900_000_000 * 1024);
    }

    // The byte columns move between releases (macOS 14 added Drop), so they are located
    // by header name — pinning them by index is the bug this guards against.
    #[test]
    fn locates_netstat_byte_columns_by_header_name() {
        let text = "Name  Mtu   Network       Address            Ipkts Ierrs     Ibytes    Opkts Oerrs     Obytes  Coll\n\
                    lo0   16384 <Link#1>                          1000     0     111111     1000     0     111111     0\n\
                    en0   1500  <Link#2>    a4:83:e7:00:00:00    500000     0  987654321   400000     0  123456789     0\n\
                    en0   1500  192.168.1   192.168.1.42         500000     -  987654321   400000     -  123456789     -\n";
        let interfaces = parse_netstat(text);
        assert_eq!(interfaces.len(), 1, "loopback dropped, en0 not double-counted");
        assert_eq!(interfaces.get("en0"), Some(&(987_654_321, 123_456_789)));
    }
}

/// No CPU thermal source that works without a kernel extension or admin rights,
/// so this reports none rather than guessing.
pub fn cpu_temps() -> Vec<TemperatureReading> {
    vec![]
}
