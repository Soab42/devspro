//! The process list on Windows: a Toolhelp snapshot for every process's
//! executable name, and the command line only for the runtimes whose
//! arguments a blocker reads (an `npm install` runs as `node.exe npm-cli.js
//! install`; a Gradle daemon as `java.exe ... GradleDaemon`).

use super::Proc;
use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, INVALID_HANDLE_VALUE, UNICODE_STRING};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

/// Only these get their command line read; the rest are matched by name.
const NEEDS_ARGS: &[&str] = &["node", "bun", "java", "javaw", "npm", "pnpm", "yarn"];

/// Walks the process list once. A process whose command line cannot be read
/// (another user's, or exited meanwhile) keeps its name with no arguments.
pub fn snapshot() -> Vec<Proc> {
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
        let comm = comm_of(&from_wide_nul(&entry.szExeFile));
        if !comm.is_empty() {
            let args = if NEEDS_ARGS.contains(&comm.as_str()) {
                command_line(entry.th32ProcessID).map(|c| split_args(&c)).unwrap_or_default()
            } else {
                Vec::new()
            };
            out.push(Proc { comm, args });
        }
        // SAFETY: as above.
        ok = unsafe { Process32NextW(snap, &mut entry) };
    }
    // SAFETY: the snapshot handle is ours and closed exactly once.
    unsafe { CloseHandle(snap) };
    out
}

/// "Code.exe" -> "code": the shape Linux's `comm` has, so one blocker list
/// of lower-case names serves both.
fn comm_of(exe: &str) -> String {
    let lower = exe.to_ascii_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

fn from_wide_nul(units: &[u16]) -> String {
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

/// The process's full command line. Needs only PROCESS_QUERY_LIMITED_INFORMATION,
/// which any process of the same user grants.
fn command_line(pid: u32) -> Option<Vec<u16>> {
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
            Some(std::slice::from_raw_parts(header.Buffer, header.Length as usize / 2).to_vec())
        }
    })();
    // SAFETY: the process handle is ours and closed exactly once.
    unsafe { CloseHandle(process) };
    result
}

/// Splits a command line the way the C runtime would.
fn split_args(line: &[u16]) -> Vec<String> {
    let mut wide: Vec<u16> = line.to_vec();
    wide.push(0);
    let mut count: i32 = 0;
    // SAFETY: `wide` is NUL-terminated; the returned block is freed below.
    let argv = unsafe { CommandLineToArgvW(wide.as_ptr(), &mut count) };
    if argv.is_null() {
        return Vec::new();
    }
    let count = count.max(0) as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        // SAFETY: argv holds `count` NUL-terminated strings.
        unsafe {
            let arg = *argv.add(i);
            let len = (0..).take_while(|&n| *arg.add(n) != 0).count();
            out.push(String::from_utf16_lossy(std::slice::from_raw_parts(arg, len)));
        }
    }
    // SAFETY: CommandLineToArgvW allocates one block with LocalAlloc.
    unsafe { LocalFree(argv.cast()) };
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_lower_case_without_the_extension() {
        assert_eq!(comm_of("Code.exe"), "code");
        assert_eq!(comm_of("msedge.exe"), "msedge");
        assert_eq!(comm_of("System"), "system");
    }

    #[test]
    fn splits_a_quoted_command_line() {
        let line: Vec<u16> = r#""C:\Program Files\nodejs\node.exe" "C:\Program Files\nodejs\node_modules\npm\bin\npm-cli.js" install"#
            .encode_utf16()
            .collect();
        assert_eq!(
            split_args(&line),
            vec![r"C:\Program Files\nodejs\node.exe", r"C:\Program Files\nodejs\node_modules\npm\bin\npm-cli.js", "install"]
        );
    }

    #[test]
    fn the_snapshot_sees_running_processes_and_reads_a_command_line() {
        let procs = snapshot();
        assert!(procs.len() > 5);
        assert!(procs.iter().all(|p| p.comm == p.comm.to_ascii_lowercase() && !p.comm.ends_with(".exe")));
        // Our own process is not in NEEDS_ARGS, so ask for its command line directly.
        let own = command_line(std::process::id()).map(|c| split_args(&c)).unwrap_or_default();
        assert!(!own.is_empty(), "could not read our own command line");
    }
}
