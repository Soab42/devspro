//! Measuring: how much disk a tree really uses, how much is free, and the
//! handle that lets a long scan report progress and be cancelled.

use super::{ScanProgress, PROGRESS_EVENT};
use std::collections::{HashMap, HashSet};
use std::fs;
use crate::modules::fs_compat::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Progress events are at most this far apart.
const THROTTLE: Duration = Duration::from_millis(250);

type Sink = Box<dyn Fn(ScanProgress) + Send + Sync>;

/// Every running scan's cancel flag, by scan id. Managed Tauri state.
#[derive(Default)]
pub struct CleanerState {
    scans: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl CleanerState {
    /// Flags the scan; it stops at its next check. Unknown ids are ignored.
    pub fn cancel(&self, scan_id: &str) {
        let scans = self.scans.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(flag) = scans.get(scan_id) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    pub fn running(&self) -> usize {
        self.scans.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
}

/// One scan in flight. Created at the top of a scan command and dropped at its
/// end, which unregisters it.
pub struct Scan {
    id: String,
    tab: String,
    cancel: Arc<AtomicBool>,
    registry: Option<Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>>,
    sink: Option<Sink>,
    entries: AtomicU64,
    bytes: AtomicU64,
    last_emit: Mutex<Option<Instant>>,
}

impl Scan {
    /// Registers `scan_id` and emits `cleaner://progress` on `app`.
    pub fn start(state: &CleanerState, app: &AppHandle, scan_id: &str, tab: &str) -> Scan {
        let app = app.clone();
        Scan::with_sink(state, scan_id, tab, Box::new(move |p| {
            let _ = app.emit(PROGRESS_EVENT, p);
        }))
    }

    pub fn with_sink(state: &CleanerState, scan_id: &str, tab: &str, sink: Sink) -> Scan {
        let cancel = Arc::new(AtomicBool::new(false));
        state
            .scans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(scan_id.to_string(), cancel.clone());
        let mut scan = Scan::detached_named(scan_id, tab, cancel);
        scan.registry = Some(state.scans.clone());
        scan.sink = Some(sink);
        scan
    }

    /// Not registered anywhere and emits nothing; for work done outside a scan.
    pub fn detached() -> Scan {
        Scan::detached_named("", "", Arc::new(AtomicBool::new(false)))
    }

    fn detached_named(id: &str, tab: &str, cancel: Arc<AtomicBool>) -> Scan {
        Scan {
            id: id.to_string(),
            tab: tab.to_string(),
            cancel,
            registry: None,
            sink: None,
            entries: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            last_emit: Mutex::new(None),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn entries(&self) -> u64 {
        self.entries.load(Ordering::Relaxed)
    }

    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    /// Counts work done and, if 250 ms have passed since the last event, emits
    /// one. Cheap enough to call per directory entry.
    pub fn tick(&self, entries: u64, bytes: u64, current: &Path) {
        self.entries.fetch_add(entries, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
        let Some(sink) = &self.sink else { return };
        let Ok(mut last) = self.last_emit.try_lock() else { return };
        let now = Instant::now();
        if last.map_or(true, |t| now.duration_since(t) >= THROTTLE) {
            *last = Some(now);
            drop(last);
            sink(self.progress(current));
        }
    }

    fn progress(&self, current: &Path) -> ScanProgress {
        ScanProgress {
            scan_id: self.id.clone(),
            tab: self.tab.clone(),
            entries: self.entries(),
            bytes: self.bytes(),
            current: current.to_string_lossy().into_owned(),
        }
    }
}

impl Drop for Scan {
    fn drop(&mut self) {
        if let Some(registry) = &self.registry {
            registry.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiskUsage {
    pub bytes: u64,
    pub entries: u64,
    /// The scan was cancelled before it finished, so `bytes` is a lower bound.
    pub truncated: bool,
}

/// Disk space used under `path`, like `du`: allocated blocks (`st_blocks * 512`,
/// so sparse files count what they occupy), each hard-linked file once, never
/// crossing onto another filesystem and never following a symlink. Unreadable
/// entries are skipped.
pub fn disk_usage(path: &Path, scan: &Scan) -> DiskUsage {
    disk_usage_with(path, scan, |m| m.dev())
}

/// `disk_usage` with the device lookup injectable, so the one-filesystem check
/// can be tested without a second mount.
fn disk_usage_with(path: &Path, scan: &Scan, dev_of: impl Fn(&fs::Metadata) -> u64) -> DiskUsage {
    let mut usage = DiskUsage::default();
    let Ok(root) = fs::symlink_metadata(path) else { return usage };
    let dev = dev_of(&root);
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut stack: Vec<(PathBuf, fs::Metadata)> = vec![(path.to_path_buf(), root)];

    while let Some((dir_or_file, meta)) = stack.pop() {
        if scan.is_cancelled() {
            usage.truncated = true;
            break;
        }
        if dev_of(&meta) != dev {
            continue;
        }
        if meta.nlink() > 1 && !meta.is_dir() && !seen.insert((meta.dev(), meta.ino())) {
            continue;
        }
        let size = meta.blocks() * 512;
        usage.bytes += size;
        usage.entries += 1;
        scan.tick(1, size, &dir_or_file);

        if meta.is_dir() {
            let Ok(read) = fs::read_dir(&dir_or_file) else { continue };
            for entry in read.flatten() {
                if let Ok(m) = fs::symlink_metadata(entry.path()) {
                    stack.push((entry.path(), m));
                }
            }
        }
    }
    usage
}

/// Bytes available to an unprivileged user on the filesystem holding `path`.
#[cfg(windows)]
pub fn free_bytes(path: &Path) -> Result<u64, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut available: u64 = 0;
    // SAFETY: `wide` is NUL-terminated and outlives the call; the two totals we
    // don't need may be null.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, std::ptr::null_mut(), std::ptr::null_mut()) };
    if ok == 0 {
        return Err(format!("GetDiskFreeSpaceExW {}: {}", path.display(), std::io::Error::last_os_error()));
    }
    Ok(available)
}

/// Bytes available to an unprivileged user on the filesystem holding `path`.
#[cfg(unix)]
pub fn free_bytes(path: &Path) -> Result<u64, String> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` a valid out-pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(format!("statvfs {}: {}", path.display(), std::io::Error::last_os_error()));
    }
    Ok((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("devspro-cleaner-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn write(path: &Path, len: usize) {
        fs::File::create(path).unwrap().write_all(&vec![7u8; len]).unwrap();
    }

    #[test]
    fn counts_allocated_blocks() {
        let d = scratch("blocks");
        write(&d.join("a"), 100_000);
        let u = disk_usage(&d, &Scan::detached());
        let own = fs::symlink_metadata(&d).unwrap().blocks() * 512;
        let file = fs::symlink_metadata(d.join("a")).unwrap().blocks() * 512;
        assert_eq!(u.bytes, own + file);
        assert_eq!(u.entries, 2);
        assert!(!u.truncated);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn hard_links_count_once() {
        let d = scratch("hardlink");
        write(&d.join("a"), 100_000);
        let one = disk_usage(&d, &Scan::detached()).bytes;
        fs::hard_link(d.join("a"), d.join("b")).unwrap();
        let two = disk_usage(&d, &Scan::detached());
        assert_eq!(two.bytes, one);
        assert_eq!(two.entries, 2);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn symlinks_are_not_followed() {
        let d = scratch("symlink");
        let outside = scratch("symlink-outside");
        write(&outside.join("big"), 1_000_000);
        std::os::unix::fs::symlink(&outside, d.join("link")).unwrap();
        let u = disk_usage(&d, &Scan::detached());
        assert_eq!(u.entries, 2);
        assert!(u.bytes < 1_000_000);
        fs::remove_dir_all(&d).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn missing_path_is_zero() {
        let u = disk_usage(Path::new("/nonexistent/devspro-cleaner"), &Scan::detached());
        assert_eq!(u, DiskUsage::default());
    }

    #[test]
    fn stays_on_the_roots_filesystem() {
        let d = scratch("xdev");
        fs::create_dir_all(d.join("mount")).unwrap();
        write(&d.join("mount/big"), 100_000);
        write(&d.join("here"), 100_000);
        let mount_ino = fs::symlink_metadata(d.join("mount")).unwrap().ino();
        // Pretend `mount` is another filesystem.
        let fake = |m: &fs::Metadata| if m.ino() == mount_ino { u64::MAX } else { m.dev() };
        let u = disk_usage_with(&d, &Scan::detached(), fake);
        assert_eq!(u.entries, 2, "root and `here` only");
        assert_eq!(disk_usage(&d, &Scan::detached()).entries, 4);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn cancel_truncates() {
        let d = scratch("cancel");
        write(&d.join("a"), 10);
        let scan = Scan::detached();
        scan.cancel();
        let u = disk_usage(&d, &scan);
        assert!(u.truncated);
        assert_eq!(u.entries, 0);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn registry_cancels_and_unregisters() {
        let state = CleanerState::default();
        let scan = Scan::with_sink(&state, "s1", "caches", Box::new(|_| {}));
        assert_eq!(state.running(), 1);
        assert!(!scan.is_cancelled());
        state.cancel("s1");
        state.cancel("unknown");
        assert!(scan.is_cancelled());
        drop(scan);
        assert_eq!(state.running(), 0);
    }

    #[test]
    fn progress_is_throttled() {
        let state = CleanerState::default();
        let seen = Arc::new(Mutex::new(Vec::<ScanProgress>::new()));
        let sink_seen = seen.clone();
        let scan = Scan::with_sink(&state, "s2", "large", Box::new(move |p| sink_seen.lock().unwrap().push(p)));
        for _ in 0..1000 {
            scan.tick(1, 10, Path::new("/x"));
        }
        assert_eq!(seen.lock().unwrap().len(), 1);
        std::thread::sleep(THROTTLE + Duration::from_millis(20));
        scan.tick(1, 10, Path::new("/y"));
        let events = seen.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].entries, 1001);
        assert_eq!(events[1].bytes, 10_010);
        assert_eq!(events[1].scan_id, "s2");
        assert_eq!(events[1].tab, "large");
        assert_eq!(events[1].current, "/y");
    }

    #[test]
    fn free_bytes_reads_a_real_filesystem() {
        assert!(free_bytes(&std::env::temp_dir()).unwrap() > 0);
        assert!(free_bytes(Path::new("/nonexistent/devspro")).is_err());
    }
}
