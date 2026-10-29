//! Windows: runs are `conhost` + `cmd` processes whose command line names the log, and
//! each is ended with a `taskkill` tree kill.

use super::{roots, Entry};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE, UNICODE_STRING};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The executables a run consists of before the user's own command.
const WRAPPERS: &[&str] = &["conhost.exe", "cmd.exe"];

/// The part of a run's command line that identifies the task. Lower case, as
/// it is compared against a lower-cased command line.
pub fn marker(id: &str) -> String {
    format!(r"scheduled-logs\{id}.log").to_ascii_lowercase()
}

/// Kills every run of the task, each with its whole process tree (the user's
/// command is a child of `cmd`, which is a child of `conhost`). Returns how
/// many runs were found.
pub fn stop_runs(id: &str) -> usize {
    let marker = marker(id);
    let procs = snapshot(&marker);
    let roots = roots(&procs);
    for pid in &roots {
        // Exit code ignored, as for workers: a grandchild exiting mid-walk
        // makes `/T` complain while the tree is in fact gone.
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .output();
    }
    roots.len()
}

fn snapshot(marker: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    // SAFETY: no pointers are passed; the handle is checked and closed below.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE || snap.is_null() {
        return out;
    }
    // SAFETY: PROCESSENTRY32W is plain C data; dwSize is set before first use, as required.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    // SAFETY: `snap` is open and `entry` is a correctly sized out-parameter throughout.
    let mut ok = unsafe { Process32FirstW(snap, &mut entry) };
    while ok != 0 {
        let exe = from_wide_nul(&entry.szExeFile).to_ascii_lowercase();
        let matches = WRAPPERS.contains(&exe.as_str())
            && command_line(entry.th32ProcessID).is_some_and(|c| c.to_ascii_lowercase().contains(marker));
        out.push(Entry { pid: entry.th32ProcessID, parent: entry.th32ParentProcessID, matches });
        // SAFETY: as above.
        ok = unsafe { Process32NextW(snap, &mut entry) };
    }
    // SAFETY: the snapshot handle is ours and closed exactly once.
    unsafe { CloseHandle(snap) };
    out
}

fn from_wide_nul(units: &[u16]) -> String {
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

/// The process's full command line. Needs only PROCESS_QUERY_LIMITED_INFORMATION,
/// which any process of the same user grants.
fn command_line(pid: u32) -> Option<String> {
    // SAFETY: plain call; the handle is checked and closed on every path out.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let result = (|| {
        let mut needed: u32 = 0;
        // SAFETY: a zero-length query only reports the size it needs.
        unsafe { NtQueryInformationProcess(process, ProcessCommandLineInformation, std::ptr::null_mut(), 0, &mut needed) };
        if needed == 0 {
            return None;
        }
        // u64s, so the UNICODE_STRING header at the front is properly aligned.
        let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: `buf` is writable for `needed` bytes and outlives the call.
        let status = unsafe {
            NtQueryInformationProcess(process, ProcessCommandLineInformation, buf.as_mut_ptr().cast(), needed, &mut needed)
        };
        if status < 0 {
            return None;
        }
        // SAFETY: on success the buffer starts with a UNICODE_STRING whose
        // Buffer points at `Length` bytes inside `buf`, which is still alive.
        unsafe {
            let header = &*(buf.as_ptr() as *const UNICODE_STRING);
            if header.Buffer.is_null() {
                return None;
            }
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(header.Buffer, header.Length as usize / 2)))
        }
    })();
    // SAFETY: the process handle is ours and closed exactly once.
    unsafe { CloseHandle(process) };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_is_the_log_path_tail_in_lower_case() {
        assert_eq!(marker("2181CA6A-x"), r"scheduled-logs\2181ca6a-x.log");
    }

    #[test]
    fn a_run_now_style_process_is_found_and_stopped() {
        let id = format!("test-{}", uuid::Uuid::new_v4());
        let log = std::env::temp_dir().join("scheduled-logs").join(format!("{id}.log"));
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        // Same shape as a real run: cmd with the log path on its command line,
        // running something that would not exit on its own for a minute.
        let mut child = Command::new("cmd")
            .raw_arg(format!(r#"/d /s /c "ping -n 60 127.0.0.1 >> "{}" 2>&1""#, log.display()))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert_eq!(stop_runs(&id), 1);
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert_eq!(stop_runs(&id), 0);
        let _ = std::fs::remove_file(log);
    }
}
