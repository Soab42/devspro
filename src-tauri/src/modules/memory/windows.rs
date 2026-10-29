//! Windows sources for the Memory module: everything `procfs` reads from `/proc`, and
//! the handful of actions that go through `kill`, `setsid`, `systemctl` and
//! `notify-send` on Linux.
//!
//! A scan starts from a ToolHelp snapshot (pid, parent, executable name — available for
//! every process, whoever owns it) and then opens each process with
//! `PROCESS_QUERY_LIMITED_INFORMATION` for the rest: creation time (the identity a
//! reused pid can't fake), CPU time, working set (Windows' RSS), owner and command line.
//! That right is the one Task Manager gets without elevation, so the figures cover most
//! of the machine; a process it can't open (another user's service, a protected
//! process) keeps its name and parent, shows no RAM, and can't be stopped from here —
//! the same reach `kill` has on Linux.
//!
//! What `Proc` means on this side:
//! - `start_time` is the creation time as a FILETIME (100 ns ticks since 1601), and
//!   `ticks` is kernel + user time in the same unit, so `CLK_TCK` is 10 000 000.
//! - `uid` is not a number Windows has: owners are SIDs, so each distinct SID seen gets
//!   a small number for the life of the app, and `my_uid` is DevsPro's own.
//! - `comm` and `exe_name` are the executable's name without `.exe`; names are never
//!   cut short here, so `COMM_MAX` is out of reach.
//! - `cmdline` is the real command line (Windows 8.1+ hands it out under the same
//!   limited right), else the executable's full path, else just its file name.

use super::control::SendError;
use super::procfs::{MemInfoKb, Proc};
use std::collections::{HashMap, HashSet};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock, PoisonError};

use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, HWND,
    INVALID_HANDLE_VALUE, LPARAM, STATUS_INFO_LENGTH_MISMATCH, STILL_ACTIVE, UNICODE_STRING,
};
use windows_sys::Win32::Security::{GetLengthSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_MANAGER_CONNECT,
    SERVICE_QUERY_STATUS, SERVICE_STATUS, SERVICE_STOPPED,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
    QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
};

/// FILETIME ticks per second.
pub const CLK_TCK: u64 = 10_000_000;

/// Windows doesn't cut process names short, so nothing is ever this long.
pub const COMM_MAX: usize = usize::MAX;

/// Keeps console children (`cmd`, PowerShell) from flashing a window: DevsPro is a GUI
/// process, so Windows would otherwise give each one a console of its own.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The pseudo-processes ToolHelp lists with no image behind them: the System Idle
/// Process and the kernel's System. The equivalent of Linux's kernel threads.
const KERNEL_PIDS: [u32; 2] = [0, 4];

/// An open process handle, closed on drop.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: only ever built from a handle OpenProcess/OpenProcessToken returned
        // non-null, and closed exactly once, here.
        unsafe { CloseHandle(self.0) };
    }
}

/// Why `OpenProcess` refused.
enum OpenError {
    /// No process has this pid.
    Gone,
    /// It exists, but this user may not have the asked-for access to it.
    Denied,
}

fn open(pid: u32, access: u32) -> Result<Handle, OpenError> {
    // SAFETY: no pointers involved; the result is checked before use.
    let handle = unsafe { OpenProcess(access, 0, pid) };
    if handle.is_null() {
        // SAFETY: reads this thread's last-error value, set by the failed call above.
        return Err(match unsafe { GetLastError() } {
            ERROR_INVALID_PARAMETER => OpenError::Gone,
            _ => OpenError::Denied,
        });
    }
    Ok(Handle(handle))
}

fn filetime(ft: FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

/// (creation time, kernel + user time), both in 100 ns ticks.
fn times(h: &Handle) -> Option<(u64, u64)> {
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: four valid out-parameters on an open handle.
    let ok = unsafe { GetProcessTimes(h.0, &mut created, &mut exited, &mut kernel, &mut user) };
    (ok != 0).then(|| (filetime(created), filetime(kernel) + filetime(user)))
}

/// Exited, but still held open by somebody — Windows' nearest thing to a zombie: its
/// memory is gone and there is nothing left to stop.
fn has_exited(h: &Handle) -> bool {
    let mut code = 0u32;
    // SAFETY: valid out-parameter on an open handle.
    let ok = unsafe { GetExitCodeProcess(h.0, &mut code) };
    ok != 0 && code != STILL_ACTIVE as u32
}

fn working_set_kb(h: &Handle) -> u64 {
    // SAFETY: plain C data; `cb` is set to its size as the API requires.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: valid out-parameter of the size passed.
    if unsafe { K32GetProcessMemoryInfo(h.0, &mut counters, counters.cb) } == 0 {
        return 0;
    }
    counters.WorkingSetSize as u64 / 1024
}

fn image_path(h: &Handle) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` UTF-16 units; the API writes at most that many and
    // updates `len` to the count written.
    let ok = unsafe { QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) };
    (ok != 0 && len > 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

/// The process's command line, through `ProcessCommandLineInformation`: the kernel
/// copies it out as a UNICODE_STRING followed by its text, so no reading of the other
/// process's memory is involved.
fn command_line(h: &Handle) -> Option<String> {
    // u64s, so the UNICODE_STRING header at the start is suitably aligned.
    let mut buf: Vec<u64> = vec![0; 512];
    for _ in 0..3 {
        let size = (buf.len() * 8) as u32;
        let mut needed = 0u32;
        // SAFETY: `buf` is `size` bytes, writable, and aligned for UNICODE_STRING.
        let status = unsafe {
            NtQueryInformationProcess(h.0, ProcessCommandLineInformation, buf.as_mut_ptr().cast(), size, &mut needed)
        };
        if status == STATUS_INFO_LENGTH_MISMATCH && needed > size {
            buf = vec![0; (needed as usize).div_ceil(8)];
            continue;
        }
        if status < 0 {
            return None;
        }
        // SAFETY: on success the buffer starts with a UNICODE_STRING whose `Buffer`
        // points `Length` bytes into the same allocation, just after the header.
        let text = unsafe {
            let header = &*buf.as_ptr().cast::<UNICODE_STRING>();
            if header.Buffer.is_null() || header.Length == 0 {
                return None;
            }
            std::slice::from_raw_parts(header.Buffer, header.Length as usize / 2)
        };
        let line = String::from_utf16_lossy(text).trim().to_string();
        return (!line.is_empty()).then_some(line);
    }
    None
}

/// Every owner SID seen so far, in the order first seen; a SID's index is its "uid".
static SIDS: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

fn uid_of_sid(sid: Vec<u8>) -> u32 {
    let mut sids = SIDS.lock().unwrap_or_else(PoisonError::into_inner);
    match sids.iter().position(|s| *s == sid) {
        Some(i) => i as u32,
        None => {
            sids.push(sid);
            (sids.len() - 1) as u32
        }
    }
}

fn owner_uid(process: HANDLE) -> Option<u32> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: valid out-parameter; the token is wrapped (and so closed) right away.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    let token = Handle(token);
    let mut buf: Vec<u64> = vec![0; 16];
    let mut needed = 0u32;
    for _ in 0..2 {
        let size = (buf.len() * 8) as u32;
        // SAFETY: `buf` is `size` writable bytes, aligned for TOKEN_USER.
        let ok = unsafe { GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), size, &mut needed) };
        if ok != 0 {
            // SAFETY: on success the buffer starts with a TOKEN_USER whose SID points
            // inside the same buffer, and GetLengthSid gives that SID's byte length.
            let sid = unsafe {
                let user = &*buf.as_ptr().cast::<TOKEN_USER>();
                let len = GetLengthSid(user.User.Sid) as usize;
                std::slice::from_raw_parts(user.User.Sid.cast::<u8>(), len).to_vec()
            };
            return Some(uid_of_sid(sid));
        }
        buf = vec![0; (needed as usize).div_ceil(8).max(16)];
    }
    None
}

/// The executable's name without its folder or `.exe`.
fn program_name(path_or_file: &str) -> String {
    let file = path_or_file.rsplit(['\\', '/']).next().unwrap_or(path_or_file);
    match file.len().checked_sub(4) {
        Some(cut) if file.is_char_boundary(cut) && file[cut..].eq_ignore_ascii_case(".exe") => file[..cut].to_string(),
        _ => file.to_string(),
    }
}

/// What one open handle can tell about a process.
struct Details {
    start_time: u64,
    ticks: u64,
    exited: bool,
    uid: Option<u32>,
    rss_kb: u64,
    path: Option<String>,
    cmdline: Option<String>,
}

fn details(pid: u32) -> Result<Details, OpenError> {
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let (start_time, ticks) = times(&h).unwrap_or((0, 0));
    Ok(Details {
        start_time,
        ticks,
        exited: has_exited(&h),
        uid: owner_uid(h.0),
        rss_kb: working_set_kb(&h),
        path: image_path(&h),
        cmdline: command_line(&h),
    })
}

fn build(pid: u32, ppid: u32, exe_file: &str, d: Option<Details>) -> Proc {
    let name = program_name(d.as_ref().and_then(|d| d.path.as_deref()).unwrap_or(exe_file));
    let kernel = KERNEL_PIDS.contains(&pid);
    let cmdline = if kernel {
        String::new()
    } else {
        d.as_ref()
            .and_then(|d| d.cmdline.clone().or_else(|| d.path.clone()))
            .unwrap_or_else(|| exe_file.to_string())
    };
    Proc {
        pid,
        ppid,
        comm: name.clone(),
        state: if d.as_ref().is_some_and(|d| d.exited) { 'Z' } else { 'R' },
        ticks: d.as_ref().map_or(0, |d| d.ticks),
        start_time: d.as_ref().map_or(0, |d| d.start_time),
        uid: d.as_ref().and_then(|d| d.uid),
        rss_kb: d.as_ref().map_or(0, |d| d.rss_kb),
        cmdline,
        exe_name: name,
    }
}

/// pid, parent pid and executable file name of every process, from one snapshot.
fn snapshot() -> Vec<(u32, u32, String)> {
    let mut out = Vec::new();
    // SAFETY: no arguments to get wrong; the handle is checked and closed below.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE || snap.is_null() {
        return out;
    }
    let snap = Handle(snap);
    // SAFETY: plain C data, with `dwSize` set as the API requires.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    // SAFETY: open snapshot, valid correctly-sized out-parameter throughout.
    let mut ok = unsafe { Process32FirstW(snap.0, &mut entry) };
    while ok != 0 {
        let end = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
        out.push((entry.th32ProcessID, entry.th32ParentProcessID, String::from_utf16_lossy(&entry.szExeFile[..end])));
        // SAFETY: as above.
        ok = unsafe { Process32NextW(snap.0, &mut entry) };
    }
    out
}

/// Windows keeps a process's parent pid after the parent exits, and pids are reused,
/// so a recorded parent may be some later, unrelated process. One created *after* its
/// supposed child can't be its parent: such links are cut, so that a process started
/// long ago never joins DevsPro's tree (or its ancestors) by pid coincidence.
fn drop_stale_parents(procs: &mut [Proc]) {
    let started: HashMap<u32, u64> = procs.iter().map(|p| (p.pid, p.start_time)).collect();
    for p in procs.iter_mut() {
        if let Some(&parent_start) = started.get(&p.ppid) {
            if p.start_time != 0 && parent_start != 0 && parent_start > p.start_time {
                p.ppid = 0;
            }
        }
    }
}

/// Every process, in pid order.
pub fn scan() -> Vec<Proc> {
    let mut procs: Vec<Proc> = snapshot()
        .into_iter()
        .map(|(pid, ppid, exe)| {
            let d = if KERNEL_PIDS.contains(&pid) { None } else { details(pid).ok() };
            build(pid, if pid == ppid { 0 } else { ppid }, &exe, d)
        })
        .collect();
    procs.sort_unstable_by_key(|p| p.pid);
    drop_stale_parents(&mut procs);
    procs
}

/// One process, without a snapshot: its parent isn't known here (0), which none of the
/// callers need. `None` when it's gone or can't be opened.
pub fn read_proc(pid: u32) -> Option<Proc> {
    if KERNEL_PIDS.contains(&pid) {
        return None;
    }
    let d = details(pid).ok()?;
    Some(build(pid, 0, "", Some(d)))
}

/// The identity re-read right before acting. A process that exists but can't be opened
/// reads as start time 0 — the same value the scan recorded for it — so the action goes
/// ahead and fails with "access denied" rather than claiming it had exited.
pub fn read_start_time(pid: u32) -> Option<u64> {
    match open(pid, PROCESS_QUERY_LIMITED_INFORMATION) {
        Ok(h) => Some(times(&h).map_or(0, |(start, _)| start)),
        Err(OpenError::Denied) => Some(0),
        Err(OpenError::Gone) => None,
    }
}

/// DevsPro's own owner, numbered like every other.
pub fn my_uid() -> Option<u32> {
    static UID: OnceLock<Option<u32>> = OnceLock::new();
    // SAFETY: the pseudo-handle for this process needs no closing.
    *UID.get_or_init(|| owner_uid(unsafe { GetCurrentProcess() }))
}

/// RAM and the pagefile, derived the way the dashboard's Windows backend does it
/// (`system::platform::windows::memory`): Windows reports a commit limit (RAM plus
/// every pagefile) rather than a swap size, so the pagefile is that limit minus RAM, and
/// "in swap" is the commit charge that doesn't fit in RAM — an estimate, as Task
/// Manager's own is.
pub fn read_meminfo() -> MemInfoKb {
    // SAFETY: plain C data, with `dwLength` set as the API requires.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: valid, correctly-sized out-parameter.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return MemInfoKb::default();
    }
    let commit_used = status.ullTotalPageFile.saturating_sub(status.ullAvailPageFile);
    let swap_total = status.ullTotalPageFile.saturating_sub(status.ullTotalPhys);
    let swap_used = commit_used.saturating_sub(status.ullTotalPhys).min(swap_total);
    MemInfoKb {
        total: status.ullTotalPhys / 1024,
        available: status.ullAvailPhys / 1024,
        swap_total: swap_total / 1024,
        swap_free: swap_total.saturating_sub(swap_used) / 1024,
    }
}

const DENIED: &str = "access denied — it runs as another user or as administrator";

/// Windows has no SIGTERM. The polite request is the one Task Manager's "End task" and
/// `taskkill` without `/F` make: WM_CLOSE to each of the process's visible top-level
/// windows, which an app handles like its close button (and may answer with a "save
/// changes?" prompt). A process with no window — a dev server, a browser's helper
/// processes — has nothing to ask; it counts as asked and is ended by the `force` pass
/// that follows, as an app ignoring SIGTERM is on Linux.
///
/// `force` is TerminateProcess. Either way the process is opened first and its creation
/// time compared with `start_time` on that same handle, so a pid reused since the scan
/// is never touched; and opening it with PROCESS_TERMINATE is what tells, up front,
/// whether this user may end it at all.
pub fn deliver(pid: u32, start_time: u64, force: bool) -> Result<(), SendError> {
    let h = match open(pid, PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION) {
        Ok(h) => h,
        Err(OpenError::Gone) => return Err(SendError::Gone),
        Err(OpenError::Denied) => return Err(SendError::Failed(DENIED.to_string())),
    };
    if times(&h).map_or(0, |(start, _)| start) != start_time || has_exited(&h) {
        return Err(SendError::Gone);
    }
    if !force {
        close_windows(pid);
        return Ok(());
    }
    // SAFETY: open handle with PROCESS_TERMINATE.
    if unsafe { TerminateProcess(h.0, 1) } != 0 {
        return Ok(());
    }
    // SAFETY: reads the last-error value of the failed call above.
    let error = unsafe { GetLastError() };
    if has_exited(&h) {
        Err(SendError::Gone)
    } else if error == ERROR_ACCESS_DENIED {
        Err(SendError::Failed(DENIED.to_string()))
    } else {
        Err(SendError::Failed(std::io::Error::from_raw_os_error(error as i32).to_string()))
    }
}

/// Posts WM_CLOSE to every visible top-level window `pid` owns; returns how many.
fn close_windows(pid: u32) -> usize {
    struct Search {
        pid: u32,
        windows: Vec<HWND>,
    }
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> i32 {
        // SAFETY: `lparam` is the `Search` passed to EnumWindows below, alive and not
        // otherwise borrowed for the length of that call.
        let search = unsafe { &mut *(lparam as *mut Search) };
        let mut owner = 0u32;
        // SAFETY: a window handle EnumWindows just handed over; valid out-parameter.
        unsafe { GetWindowThreadProcessId(hwnd, &mut owner) };
        // SAFETY: as above.
        if owner == search.pid && unsafe { IsWindowVisible(hwnd) } != 0 {
            search.windows.push(hwnd);
        }
        1
    }
    let mut search = Search { pid, windows: Vec::new() };
    // SAFETY: the callback only touches `search` through the pointer given here, and
    // EnumWindows returns before `search` goes out of scope.
    unsafe { EnumWindows(Some(each), &mut search as *mut Search as LPARAM) };
    for &hwnd in &search.windows {
        // SAFETY: posting (not sending) never blocks on the target; a window that closed
        // in the meantime just makes this fail.
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
    }
    search.windows.len()
}

/// Runs a start command the way `sh -c` does on Linux, through `cmd /C` — so `%VARS%`,
/// quoting and PATH lookup behave as at a prompt — wrapped in `start "" /B`, which
/// returns at once and also finds apps registered under App Paths (`chrome`, `msedge`)
/// that aren't on PATH. `cmd` exits straight away, so the app doesn't stay DevsPro's
/// descendant, which would hide it from this page; a console program runs without a
/// window, as it would under `setsid` with no terminal.
pub fn spawn_shell(command: &str) -> Result<(), String> {
    let mut cmd = Command::new("cmd");
    cmd.args(["/D", "/C"])
        .raw_arg(format!("start \"\" /B {command}"))
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(home) = std::env::var_os("USERPROFILE") {
        cmd.current_dir(home);
    }
    let mut child = cmd.spawn().map_err(|e| format!("failed to run cmd: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Which of these services are running (or starting, stopping or paused — anything but
/// stopped), asked of the Service Control Manager. Querying a service's status needs no
/// elevation. An unknown name is simply not running.
pub fn active_services(names: &[&str]) -> HashSet<String> {
    let mut active = HashSet::new();
    // SAFETY: null machine and database mean the local, active database.
    let scm = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if scm.is_null() {
        return active;
    }
    for name in names {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call.
        let svc = unsafe { OpenServiceW(scm, wide.as_ptr(), SERVICE_QUERY_STATUS) };
        if svc.is_null() {
            continue;
        }
        // SAFETY: plain C data, filled by the call below.
        let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
        // SAFETY: open service handle, valid out-parameter.
        if unsafe { QueryServiceStatus(svc, &mut status) } != 0 && status.dwCurrentState != SERVICE_STOPPED {
            active.insert(name.to_string());
        }
        // SAFETY: opened above, closed once.
        unsafe { CloseServiceHandle(svc) };
    }
    // SAFETY: opened above, closed once.
    unsafe { CloseServiceHandle(scm) };
    active
}

/// Toast text goes into XML, which goes into a single-quoted PowerShell string.
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The PowerShell that shows one toast. It is shown under PowerShell's own registered
/// app id: Windows drops toasts from an id no installed shortcut declares, which is what
/// DevsPro's would be in a development build.
fn toast_script(title: &str, body: &str) -> String {
    let xml = format!(
        "<toast><visual><binding template=''ToastGeneric''><text>{}</text><text>{}</text></binding></visual></toast>",
        xml_escape(title),
        xml_escape(body)
    );
    format!(
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
         [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] > $null; \
         $x = New-Object Windows.Data.Xml.Dom.XmlDocument; \
         $x.LoadXml('{xml}'); \
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\\WindowsPowerShell\\v1.0\\powershell.exe').Show([Windows.UI.Notifications.ToastNotification]::new($x))"
    )
}

/// `notify-send`'s stand-in: a Windows toast. Best effort, like the Linux one.
pub fn notify(title: &str, body: &str) {
    use base64::Engine;
    let script = toast_script(title, body);
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    let child = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-EncodedCommand", &encoded])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_names_lose_folder_and_exe() {
        assert_eq!(program_name(r"C:\Program Files\Google\Chrome\Application\chrome.exe"), "chrome");
        assert_eq!(program_name("Code.EXE"), "Code");
        assert_eq!(program_name("System"), "System");
        assert_eq!(program_name("Memory Compression"), "Memory Compression");
        assert_eq!(program_name(".exe"), "");
    }

    #[test]
    fn a_parent_younger_than_its_child_is_a_reused_pid() {
        let p = |pid, ppid, start| Proc {
            pid,
            ppid,
            comm: String::new(),
            state: 'R',
            ticks: 0,
            start_time: start,
            uid: None,
            rss_kb: 0,
            cmdline: "x".to_string(),
            exe_name: String::new(),
        };
        let mut procs = vec![p(10, 1, 100), p(20, 10, 200), p(30, 10, 50), p(40, 99, 10), p(50, 10, 0)];
        drop_stale_parents(&mut procs);
        let parents: Vec<u32> = procs.iter().map(|p| p.ppid).collect();
        assert_eq!(parents, [1, 10, 0, 99, 10]);
    }

    #[test]
    fn toast_text_cannot_break_out_of_the_xml_or_the_string() {
        let script = toast_script("DevsPro closed it's <b>", "a & b \"c\"");
        assert!(script.contains("<text>DevsPro closed it&apos;s &lt;b&gt;</text>"));
        assert!(script.contains("<text>a &amp; b &quot;c&quot;</text>"));
        // The only single quotes are the ones the script itself puts there.
        assert_eq!(script.matches('\'').count(), 8);
    }

    /// The live machine: this test process must be found, owned by "me", with a
    /// creation time, RAM and a command line; the kernel's pseudo-processes must read
    /// like Linux kernel threads; and the identity re-read must agree with the scan.
    #[test]
    fn scans_this_machine() {
        let me = std::process::id();
        let procs = scan();
        assert!(procs.len() > 10);
        let this = procs.iter().find(|p| p.pid == me).expect("the test process is in the scan");
        assert!(this.is_live_userspace());
        assert!(this.start_time > 0 && this.rss_kb > 0, "{this:?}");
        assert_eq!(this.uid, my_uid());
        assert!(my_uid().is_some());
        assert!(this.cmdline.contains(&this.exe_name), "{this:?}");
        assert_ne!(this.ppid, 0, "the test runner is a live parent");
        assert_eq!(read_start_time(me), Some(this.start_time));
        assert_eq!(read_proc(me).map(|p| p.start_time), Some(this.start_time));
        for kernel in procs.iter().filter(|p| KERNEL_PIDS.contains(&p.pid)) {
            assert!(!kernel.is_live_userspace(), "{kernel:?}");
        }
        let mem = read_meminfo();
        assert!(mem.total > 0 && mem.available <= mem.total);
    }

    #[test]
    fn a_wrong_start_time_is_never_ended() {
        let me = std::process::id();
        let start = read_start_time(me).unwrap();
        assert!(matches!(deliver(me, start + 1, true), Err(SendError::Gone)));
    }

    #[test]
    fn ends_a_child_process_it_started() {
        let mut child = Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let pid = child.id();
        let start = read_start_time(pid).unwrap();
        assert!(deliver(pid, start, false).is_ok(), "a windowless process counts as asked");
        assert!(deliver(pid, start, true).is_ok());
        assert!(!child.wait().unwrap().success());
        assert!(matches!(deliver(pid, start, true), Err(SendError::Gone)));
    }

    #[test]
    fn start_commands_run_through_cmd_and_return_at_once() {
        let marker = std::env::temp_dir().join(format!("devspro-memory-start-{}", std::process::id()));
        let _ = std::fs::remove_dir(&marker);
        spawn_shell(&format!("cmd /C mkdir \"{}\"", marker.display())).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(marker.exists(), "the started command ran");
        let _ = std::fs::remove_dir(&marker);
    }

    #[test]
    fn unknown_services_are_not_running() {
        assert!(active_services(&["devspro-no-such-service"]).is_empty());
        // The event log runs on every Windows machine.
        assert!(active_services(&["EventLog"]).contains("EventLog"));
    }
}
