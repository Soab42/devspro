//! Windows system sources. Where the Linux backend reads `/proc` and shells out to
//! `df`/`lspci`, Windows has no such text interface, and the CLIs that could stand in
//! (`wmic`, PowerShell's `Get-CimInstance`) cost a process spawn of 100ms–1s+ — far too
//! much for something the dashboard polls every couple of seconds. So this backend
//! calls the Win32 APIs directly through `windows-sys`, which is already a dependency
//! for the UAC elevation launcher (`elevate::platform::windows`).
//!
//! Two exceptions, both deliberate: GPUs come from `nvidia-smi` when it's installed
//! (see `super::nvidia` — same tool, same flags as on Linux) and otherwise from the
//! display-adapter registry class, because measuring a non-NVIDIA GPU would mean
//! taking on WMI/COM for numbers the dashboard treats as optional anyway.

use std::collections::HashMap;

use windows_sys::Wdk::System::SystemInformation::{
    NtQuerySystemInformation, SystemProcessorPerformanceInformation,
};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, MIB_IF_TABLE2, IF_TYPE_SOFTWARE_LOOPBACK,
};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD,
    REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
};
use windows_sys::Win32::System::SystemInformation::{
    ComputerNamePhysicalDnsHostname, GetComputerNameExW, GetTickCount64, GlobalMemoryStatusEx,
    MEMORYSTATUSEX,
};
use windows_sys::Win32::System::WindowsProgramming::SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION;

use super::super::{CpuTicks, DiskInfo, GpuInfo, MemoryInfo, OsIdentity, TemperatureReading};
use super::CpuSample;

// ---------------------------------------------------------------------------
// CPU
// ---------------------------------------------------------------------------

/// `NtQuerySystemInformation` reports a buffer that's too small by returning this
/// rather than by writing a required length, so the only portable way to size the
/// per-core array is to try and grow.
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;

/// Starting guess for the per-core array: one processor group holds at most 64
/// logical processors, so this is the whole machine for anything short of a
/// dual-socket server.
const INITIAL_CORE_CAPACITY: usize = 64;
const MAX_CORE_CAPACITY: usize = 1024;

pub fn cpu_sample() -> CpuSample {
    let per_core = read_per_core_ticks();
    if per_core.is_empty() {
        return CpuSample {
            ticks: Vec::new(),
            core_count: 0,
        };
    }

    // Windows has no equivalent of `/proc/stat`'s leading aggregate "cpu" line, but
    // that line is by definition the sum of the per-core ones — so summing here
    // produces the same thing the shared rate math already expects at index 0.
    let aggregate = per_core.iter().fold(CpuTicks::default(), |acc, core| CpuTicks {
        idle: acc.idle.saturating_add(core.idle),
        total: acc.total.saturating_add(core.total),
    });

    let core_count = per_core.len();
    let mut ticks = Vec::with_capacity(core_count + 1);
    ticks.push(aggregate);
    ticks.extend(per_core);
    CpuSample { ticks, core_count }
}

fn read_per_core_ticks() -> Vec<CpuTicks> {
    const ENTRY_SIZE: usize = std::mem::size_of::<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION>();

    let mut capacity = INITIAL_CORE_CAPACITY;
    loop {
        let mut buf: Vec<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION> = Vec::with_capacity(capacity);
        let mut returned_bytes: u32 = 0;

        // SAFETY: the pointer is a freshly-allocated, correctly-aligned allocation of
        // exactly `capacity` entries, and the length we hand over matches it in bytes.
        // Nothing reads from `buf` until `set_len` below caps it at the entry count the
        // kernel says it actually wrote.
        let status = unsafe {
            NtQuerySystemInformation(
                SystemProcessorPerformanceInformation,
                buf.as_mut_ptr().cast(),
                (capacity * ENTRY_SIZE) as u32,
                &mut returned_bytes,
            )
        };

        if status == STATUS_INFO_LENGTH_MISMATCH && capacity < MAX_CORE_CAPACITY {
            capacity *= 2;
            continue;
        }
        if status < 0 {
            return Vec::new();
        }

        let filled = (returned_bytes as usize / ENTRY_SIZE).min(capacity);
        // SAFETY: the call succeeded, so the kernel initialized `returned_bytes` worth
        // of entries; `filled` is that count clamped to what we allocated.
        unsafe { buf.set_len(filled) };

        return buf
            .iter()
            .map(|core| CpuTicks {
                idle: core.IdleTime.max(0) as u64,
                // KernelTime already *includes* IdleTime on Windows (and DPC/interrupt
                // time with it), so kernel + user is the core's whole elapsed budget —
                // the direct analogue of summing /proc/stat's per-state columns.
                total: (core.KernelTime.max(0) as u64).saturating_add(core.UserTime.max(0) as u64),
            })
            .collect();
    }
}

pub fn cpu_model() -> String {
    RegKey::open(
        HKEY_LOCAL_MACHINE,
        r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
    )
    .and_then(|key| key.string("ProcessorNameString"))
    .map(|name| name.trim().to_string())
    .filter(|name| !name.is_empty())
    .unwrap_or_else(|| "Unknown CPU".to_string())
}

/// Windows has no load average — nothing in the kernel tracks a run-queue length over
/// 1/5/15-minute windows. Reporting zeros would render as a genuinely idle machine on
/// the dashboard, so this says "unavailable" and lets the UI omit the figure.
pub fn load_avg() -> Option<(f32, f32, f32)> {
    None
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

pub fn memory() -> MemoryInfo {
    // SAFETY: MEMORYSTATUSEX is a plain data struct with no invalid bit patterns, and
    // `dwLength` (which the API requires be set before the call) is filled in below.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;

    // SAFETY: `status` is a valid, correctly-sized out-parameter.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return MemoryInfo {
            total_kb: 0,
            used_kb: 0,
            available_kb: 0,
            swap_total_kb: 0,
            swap_used_kb: 0,
        };
    }

    // `ullTotalPageFile` is the *commit limit* — physical RAM plus every pagefile —
    // not the pagefile alone, and Windows exposes no counter for "bytes currently
    // paged out". So the pagefile's size is the commit limit minus RAM, and its usage
    // is however much of the commit charge cannot fit in RAM. That's the same
    // derivation Task Manager's "Committed" figure rests on; it's an upper-bound
    // estimate, not a measurement, but it moves for the same reasons swap does.
    let commit_limit = status.ullTotalPageFile;
    let commit_used = commit_limit.saturating_sub(status.ullAvailPageFile);
    let swap_total = commit_limit.saturating_sub(status.ullTotalPhys);
    let swap_used = commit_used
        .saturating_sub(status.ullTotalPhys)
        .min(swap_total);

    MemoryInfo {
        total_kb: to_kb(status.ullTotalPhys),
        used_kb: to_kb(status.ullTotalPhys.saturating_sub(status.ullAvailPhys)),
        available_kb: to_kb(status.ullAvailPhys),
        swap_total_kb: to_kb(swap_total),
        swap_used_kb: to_kb(swap_used),
    }
}

fn to_kb(bytes: u64) -> u64 {
    bytes / 1024
}

pub fn uptime_seconds() -> u64 {
    // SAFETY: no arguments, no out-parameters — cannot fail.
    unsafe { GetTickCount64() / 1000 }
}

// ---------------------------------------------------------------------------
// OS identity
// ---------------------------------------------------------------------------

const CURRENT_VERSION_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

/// The build number Windows 11 started at. Needed because `ProductName` in the
/// registry still reads "Windows 10 …" on every Windows 11 install — Microsoft never
/// updated that value — so trusting it verbatim would have the dashboard confidently
/// name the wrong OS.
const WINDOWS_11_MIN_BUILD: u32 = 22000;

pub fn os_identity() -> OsIdentity {
    let key = RegKey::open(HKEY_LOCAL_MACHINE, CURRENT_VERSION_KEY);
    let read = |name: &str| key.as_ref().and_then(|k| k.string(name));
    let read_u32 = |name: &str| key.as_ref().and_then(|k| k.u32(name));

    let build = read("CurrentBuildNumber").unwrap_or_default();
    let product = read("ProductName").unwrap_or_else(|| "Windows".to_string());
    let os_name = match build.parse::<u32>() {
        Ok(build) if build >= WINDOWS_11_MIN_BUILD => product.replace("Windows 10", "Windows 11"),
        _ => product,
    };

    // `DisplayVersion` is the modern "24H2"-style label; `ReleaseId` is the older
    // "2009"-style one, still all that's there on builds before 20H2.
    let os_version = read("DisplayVersion")
        .or_else(|| read("ReleaseId"))
        .unwrap_or_default();

    let major = read_u32("CurrentMajorVersionNumber").unwrap_or(10);
    let minor = read_u32("CurrentMinorVersionNumber").unwrap_or(0);
    let kernel_version = match read_u32("UBR") {
        Some(ubr) => format!("{major}.{minor}.{build}.{ubr}"),
        None => format!("{major}.{minor}.{build}"),
    };

    OsIdentity {
        hostname: hostname(),
        os_name,
        os_version,
        kernel_version,
    }
}

fn hostname() -> String {
    // DNS hostnames top out at 255 characters, so this is sized to always be enough
    // and the two-call "ask for the length first" dance is not worth it here.
    let mut buf = [0u16; 256];
    let mut size = buf.len() as u32;

    // SAFETY: `buf` is a writable buffer of `size` UTF-16 units, and `size` is a valid
    // in/out parameter which the call updates to the length actually written.
    let ok = unsafe {
        GetComputerNameExW(
            ComputerNamePhysicalDnsHostname,
            buf.as_mut_ptr(),
            &mut size,
        )
    };
    if ok == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..size as usize])
}

// ---------------------------------------------------------------------------
// Disks
// ---------------------------------------------------------------------------

// The SDK defines these as preprocessor macros rather than as metadata constants, so
// `windows-sys` doesn't re-export them. Values from `winbase.h`.
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOTE: u32 = 4;

pub fn disks() -> Vec<DiskInfo> {
    logical_drive_roots()
        .into_iter()
        .filter(|root| {
            let root_w = to_wide_nul(root);
            // SAFETY: `root_w` is a NUL-terminated UTF-16 buffer alive for the call.
            let kind = unsafe { GetDriveTypeW(root_w.as_ptr()) };
            // Fixed disks, mounted USB storage and mapped network drives are all
            // things a developer keeps a project on. Optical drives and RAM disks are
            // the Windows analogue of the pseudo-filesystems the Linux backend skips.
            matches!(kind, DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_REMOTE)
        })
        .filter_map(|root| describe_drive(&root))
        .collect()
}

fn logical_drive_roots() -> Vec<String> {
    // SAFETY: the documented way to ask for the required length — a null buffer with
    // length 0 makes the call report the size it needs instead of writing anything.
    let needed = unsafe { GetLogicalDriveStringsW(0, std::ptr::null_mut()) };
    if needed == 0 {
        return Vec::new();
    }

    let mut buf = vec![0u16; needed as usize];
    // SAFETY: `buf` holds exactly the number of UTF-16 units the call just asked for.
    let written = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) };
    if written == 0 {
        return Vec::new();
    }

    // The result is a double-NUL-terminated list of NUL-separated roots ("C:\0D:\0\0").
    buf[..written as usize]
        .split(|&unit| unit == 0)
        .filter(|part| !part.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

fn describe_drive(root: &str) -> Option<DiskInfo> {
    let root_w = to_wide_nul(root);

    let mut available_to_caller: u64 = 0;
    let mut total: u64 = 0;
    let mut total_free: u64 = 0;
    // SAFETY: `root_w` is NUL-terminated and alive for the call; the three out-params
    // are valid, distinct locations. Fails for a card reader with no card in it or a
    // disconnected network drive, which is exactly what the `?` below drops.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            root_w.as_ptr(),
            &mut available_to_caller,
            &mut total,
            &mut total_free,
        )
    };
    if ok == 0 || total == 0 {
        return None;
    }

    let (label, fs_type) = volume_identity(&root_w);

    Some(DiskInfo {
        // The Linux backend puts a device node here ("/dev/nvme0n1p2"). A volume label
        // is the closest thing Windows has that means something to the person reading
        // it; the drive letter already appears as the mount point.
        device: if label.is_empty() {
            root.to_string()
        } else {
            label
        },
        mount_point: root.to_string(),
        fs_type,
        total_bytes: total,
        // `total_free` is the volume's real free space, whereas `available_to_caller`
        // is what this user may still write given any disk quota. Reporting used as
        // total − total_free keeps used + available ≤ total under a quota, rather than
        // overstating what's in use.
        used_bytes: total.saturating_sub(total_free),
        available_bytes: available_to_caller,
    })
}

/// Volume label and filesystem name ("NTFS", "exFAT"), each empty if unavailable.
fn volume_identity(root_w: &[u16]) -> (String, String) {
    // Both buffers are sized by `MAX_PATH + 1`, the documented maximum for either.
    let mut label = [0u16; 261];
    let mut fs_name = [0u16; 261];

    // SAFETY: `root_w` is NUL-terminated; both buffers are writable and their declared
    // lengths match their real ones. The serial/component-length/flags out-params are
    // deliberately null — the API accepts null for any it isn't asked for.
    let ok = unsafe {
        GetVolumeInformationW(
            root_w.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fs_name.as_mut_ptr(),
            fs_name.len() as u32,
        )
    };
    if ok == 0 {
        return (String::new(), String::new());
    }
    (from_wide_nul(&label), from_wide_nul(&fs_name))
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

pub fn net_bytes() -> HashMap<String, (u64, u64)> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();

    // SAFETY: `table` is a valid out-pointer; on success the call hands back an
    // allocation we own and must release with `FreeMibTable`.
    let rc = unsafe { GetIfTable2(&mut table) };
    if rc != ERROR_SUCCESS || table.is_null() {
        return HashMap::new();
    }

    // SAFETY: `Table` is a C flexible array member declared as `[MIB_IF_ROW2; 1]`;
    // `NumEntries` is how many the call actually wrote, which is what makes reading it
    // as a slice of that length sound. Nothing returns early before `FreeMibTable`.
    let rows = unsafe {
        std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize)
    };

    let interfaces = rows
        .iter()
        .filter(|row| {
            // The dashboard filters Linux's loopback out by its name ("lo"), which
            // would never match "Loopback Pseudo-Interface 1" — so drop it here
            // instead. Down interfaces are dropped too: a stock Windows install has
            // dozens of dormant WAN Miniport and tunnelling adapters that would
            // otherwise bury the real NIC in a list of permanent zeroes.
            row.Type != IF_TYPE_SOFTWARE_LOOPBACK
                && row.OperStatus == IfOperStatusUp
                && !is_filter_interface(row)
        })
        .filter_map(|row| {
            let name = match from_wide_nul(&row.Alias) {
                alias if !alias.is_empty() => alias,
                _ => from_wide_nul(&row.Description),
            };
            (!name.is_empty()).then(|| (name, (row.InOctets, row.OutOctets)))
        })
        .collect();

    // SAFETY: `table` is the allocation `GetIfTable2` gave us, freed exactly once, and
    // `rows` is not touched after this point.
    unsafe { FreeMibTable(table.cast()) };

    interfaces
}

/// Every NDIS filter bound to an adapter — QoS Packet Scheduler, the WFP MAC-layer
/// filters, the Native WiFi filter driver — shows up in `GetIfTable2` as its own row
/// *carrying the adapter's own byte counters*. So one Wi-Fi card arrives four or five
/// times over, each copy reporting the same totals, and the dashboard sums throughput
/// across interfaces: left in, a 10 MB/s download reads as 40-50 MB/s. They're
/// distinguishable only by this flag, not by anything in the name.
///
/// `InterfaceAndOperStatusFlags` is a C bitfield that `windows-sys` exposes as one
/// opaque byte, so the bits are addressed by position, in the order `netioapi.h`
/// declares them: `HardwareInterface` is bit 0 and `FilterInterface` bit 1.
fn is_filter_interface(row: &windows_sys::Win32::NetworkManagement::IpHelper::MIB_IF_ROW2) -> bool {
    const FILTER_INTERFACE_BIT: u8 = 1 << 1;
    row.InterfaceAndOperStatusFlags._bitfield & FILTER_INTERFACE_BIT != 0
}

// ---------------------------------------------------------------------------
// GPUs
// ---------------------------------------------------------------------------

/// Setup class GUID for display adapters. Every installed GPU gets a numbered subkey
/// (`0000`, `0001`, …) under here holding the driver's own description and version.
const DISPLAY_CLASS_KEY: &str =
    r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

/// How many numbered subkeys to look at. Enumerating properly would mean
/// `RegEnumKeyExW`, but these keys are literally named by a zero-padded index, and no
/// machine has 16 display adapters.
const MAX_DISPLAY_ADAPTERS: u32 = 16;

pub fn gpus() -> Vec<GpuInfo> {
    if let Some(gpus) = super::nvidia::query() {
        if !gpus.is_empty() {
            return gpus;
        }
    }
    read_gpus_from_registry()
}

/// Fallback for anything `nvidia-smi` can't see — integrated Intel/AMD graphics, or an
/// NVIDIA card whose driver tools aren't installed. Names the adapter and its driver
/// version, and leaves every measurement `None`: the registry knows what the hardware
/// is, not what it's currently doing, and a faked 0% would read as an idle GPU rather
/// than an unmeasurable one.
fn read_gpus_from_registry() -> Vec<GpuInfo> {
    (0..MAX_DISPLAY_ADAPTERS)
        .filter_map(|index| {
            let key = RegKey::open(
                HKEY_LOCAL_MACHINE,
                &format!("{DISPLAY_CLASS_KEY}\\{index:04}"),
            )?;
            // The class key also holds non-adapter subkeys (`Properties`,
            // `Configuration`); only a real adapter has a DriverDesc.
            let name = key.string("DriverDesc")?;
            Some(GpuInfo {
                name,
                driver: key.string("DriverVersion"),
                usage_percent: None,
                memory_used_mb: None,
                memory_total_mb: None,
                temperature: None,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Win32 helpers
// ---------------------------------------------------------------------------

/// A registry key that closes itself. Every read below goes through one of these, so
/// an early `?` can't leak the handle.
struct RegKey(HKEY);

impl RegKey {
    fn open(root: HKEY, subkey: &str) -> Option<Self> {
        let subkey_w = to_wide_nul(subkey);
        let mut handle: HKEY = std::ptr::null_mut();

        // SAFETY: `subkey_w` is NUL-terminated and alive for the call; `handle` is a
        // valid out-pointer, only wrapped (and thus only closed) if the call succeeds.
        let rc = unsafe {
            RegOpenKeyExW(
                root,
                subkey_w.as_ptr(),
                0,
                KEY_READ,
                &mut handle,
            )
        };
        (rc == ERROR_SUCCESS).then_some(RegKey(handle))
    }

    /// Reads a `REG_SZ`/`REG_EXPAND_SZ` value, trimmed of its trailing NUL.
    fn string(&self, name: &str) -> Option<String> {
        let name_w = to_wide_nul(name);
        let mut kind: REG_VALUE_TYPE = 0;
        let mut size: u32 = 0;

        // SAFETY: a null data pointer with a zero length is the documented way to ask
        // for a value's size and type without reading it.
        let rc = unsafe {
            RegQueryValueExW(
                self.0,
                name_w.as_ptr(),
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if rc != ERROR_SUCCESS || !matches!(kind, REG_SZ | REG_EXPAND_SZ) {
            return None;
        }

        let mut bytes = vec![0u8; size as usize];
        let mut written = size;
        // SAFETY: `bytes` is exactly `size` writable bytes, which is what the call just
        // reported it needs.
        let rc = unsafe {
            RegQueryValueExW(
                self.0,
                name_w.as_ptr(),
                std::ptr::null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut written,
            )
        };
        if rc != ERROR_SUCCESS {
            return None;
        }

        // Reassembled pair-by-pair rather than by casting to `*const u16`: the registry
        // hands back bytes with no alignment guarantee, and a misaligned u16 read is UB.
        let units: Vec<u16> = bytes[..written.min(size) as usize]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        Some(from_wide_nul(&units))
    }

    fn u32(&self, name: &str) -> Option<u32> {
        let name_w = to_wide_nul(name);
        let mut kind: REG_VALUE_TYPE = 0;
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;

        // SAFETY: `value` is a correctly-sized, correctly-aligned destination for the
        // 4 bytes a REG_DWORD holds, and `size` says so.
        let rc = unsafe {
            RegQueryValueExW(
                self.0,
                name_w.as_ptr(),
                std::ptr::null(),
                &mut kind,
                (&mut value as *mut u32).cast::<u8>(),
                &mut size,
            )
        };
        (rc == ERROR_SUCCESS && kind == REG_DWORD).then_some(value)
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a key this type opened and has not closed yet.
        unsafe { RegCloseKey(self.0) };
    }
}

/// Encodes `s` as a NUL-terminated UTF-16 buffer for a `PCWSTR` argument. The returned
/// `Vec` must outlive any FFI call holding a pointer into it.
fn to_wide_nul(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Decodes a fixed-size UTF-16 buffer that the OS filled and NUL-terminated, ignoring
/// whatever uninitialised tail follows the terminator.
fn from_wide_nul(units: &[u16]) -> String {
    let end = units.iter().position(|&unit| unit == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything here is FFI against the live machine, so there is nothing to unit-test
    /// in isolation — a wrong struct layout or a misread out-parameter produces plausible
    /// nonsense rather than a compile error. This asserts the shape of what a real
    /// Windows box must report, which is what actually catches that class of mistake:
    /// zero cores, no memory, no system drive, or a per-core array that doesn't line up
    /// with the core count all mean the API call was wired up wrong.
    #[test]
    fn reads_a_plausible_machine() {
        let sample = cpu_sample();
        assert!(sample.core_count >= 1, "a machine running this has cores");
        assert_eq!(
            sample.ticks.len(),
            sample.core_count + 1,
            "one aggregate entry plus one per core"
        );
        assert!(
            sample.ticks[0].total > 0 && sample.ticks[0].idle <= sample.ticks[0].total,
            "cumulative counters, idle a subset of the total"
        );
        assert!(!cpu_model().is_empty());
        assert_eq!(load_avg(), None, "Windows has no load average");

        let memory = memory();
        assert!(memory.total_kb > 0);
        assert!(memory.available_kb <= memory.total_kb);
        assert!(memory.used_kb <= memory.total_kb);
        assert!(memory.swap_used_kb <= memory.swap_total_kb);

        assert!(uptime_seconds() > 0);

        let identity = os_identity();
        assert!(!identity.hostname.is_empty());
        assert!(
            identity.os_name.contains("Windows"),
            "got {:?}",
            identity.os_name
        );
        // "10.0.<build>[.<ubr>]" — never left as the bare fallback with no build number.
        assert!(
            identity.kernel_version.matches('.').count() >= 2,
            "got {:?}",
            identity.kernel_version
        );

        let disks = disks();
        assert!(!disks.is_empty(), "the system drive is always mounted");
        for disk in &disks {
            assert!(!disk.mount_point.is_empty());
            assert!(disk.total_bytes > 0);
            assert!(disk.used_bytes <= disk.total_bytes);
            assert!(disk.available_bytes <= disk.total_bytes);
        }

        // Deliberately no assertion on how many interfaces or GPUs exist — a machine
        // with every adapter disabled, or with no discrete GPU, is still a valid one.
        for name in net_bytes().keys() {
            assert!(!name.is_empty(), "an interface with no name is a bad read");
        }
    }

    /// Pins the bit position `is_filter_interface` reads. Getting it wrong is silent and
    /// costly in both directions: read bit 0 instead of bit 1 and every physical adapter
    /// disappears while its filters stay, and read neither and one Wi-Fi card is counted
    /// four or five times over, inflating the dashboard's throughput to match.
    #[test]
    fn identifies_ndis_filter_interfaces_by_their_flag_bit() {
        use windows_sys::Win32::NetworkManagement::IpHelper::MIB_IF_ROW2;

        // SAFETY: MIB_IF_ROW2 is plain C data with no invalid bit patterns, and only the
        // flags byte is read below.
        let mut row: MIB_IF_ROW2 = unsafe { std::mem::zeroed() };

        // HardwareInterface (bit 0) alone — a real NIC.
        row.InterfaceAndOperStatusFlags._bitfield = 0b0000_0001;
        assert!(!is_filter_interface(&row));

        // FilterInterface (bit 1) — a QoS/WFP layer wearing the adapter's counters.
        row.InterfaceAndOperStatusFlags._bitfield = 0b0000_0010;
        assert!(is_filter_interface(&row));

        // Both set, and unrelated higher bits (ConnectorPresent, Paused, …) must not
        // change the answer either way.
        row.InterfaceAndOperStatusFlags._bitfield = 0b1111_1101;
        assert!(!is_filter_interface(&row));
        row.InterfaceAndOperStatusFlags._bitfield = 0b1111_1111;
        assert!(is_filter_interface(&row));
    }
}

/// No CPU thermal source that works without a kernel driver or admin rights,
/// so this reports none rather than guessing.
pub fn cpu_temps() -> Vec<TemperatureReading> {
    vec![]
}
