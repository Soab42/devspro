use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, SHELLEXECUTEINFOW_0,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Uses `ShellExecuteExW` rather than the simpler `ShellExecuteW` specifically for the
/// `SEE_MASK_NOCLOSEPROCESS` flag, which is the only way to get a process handle back:
/// plain `ShellExecuteW` is fire-and-forget and would let this function return `Ok(())`
/// before the helper had even run, breaking the "block until the helper exits" contract
/// in this module's doc comment. `lpVerb = "runas"` is what actually raises the native
/// UAC prompt. If the user clicks "No", `ShellExecuteExW` itself returns `FALSE` with
/// `GetLastError() == ERROR_CANCELLED`, which is surfaced as `Err` below (the Windows
/// analogue of pkexec's "denied" exit in the Linux launcher). Once launched, we
/// `WaitForSingleObject` + `GetExitCodeProcess` to get the helper's real exit code —
/// a non-zero code (ran, but the manifest apply failed) is also `Err`, matching how
/// the Linux launcher treats a non-zero `status`. `lpParameters` is a single string,
/// not an argv array, so the manifest path is quoted using the same escaping rule
/// `CommandLineToArgvW` expects — that's what the helper's `std::env::args()` parses
/// on the other end (see `helper/src/main.rs`), so an unquoted space-containing path
/// would otherwise be split into multiple argv entries. `lpFile` does *not* get this
/// treatment: it's a distinct struct field consumed directly by the shell, not part of
/// a command line, so no quoting is needed or wanted there.
pub fn launch_helper(helper_path: &Path, manifest_path: &Path) -> Result<(), String> {
    let helper_wide = to_wide_nul(helper_path.as_os_str());
    let params_wide = quote_for_argv(manifest_path);
    let verb_wide = to_wide_nul(OsStr::new("runas"));

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: ptr::null_mut(),
        lpVerb: verb_wide.as_ptr(),
        lpFile: helper_wide.as_ptr(),
        lpParameters: params_wide.as_ptr(),
        lpDirectory: ptr::null(),
        nShow: SW_SHOWNORMAL,
        hInstApp: ptr::null_mut(),
        lpIDList: ptr::null_mut(),
        lpClass: ptr::null(),
        hkeyClass: ptr::null_mut(),
        dwHotKey: 0,
        Anonymous: SHELLEXECUTEINFOW_0 {
            hIcon: ptr::null_mut(),
        },
        hProcess: ptr::null_mut(),
    };

    // SAFETY: `info` is a valid, correctly-sized SHELLEXECUTEINFOW; every pointer field
    // either points at a NUL-terminated UTF-16 buffer kept alive until this call returns
    // (helper_wide/params_wide/verb_wide) or is a deliberate null for an unused field.
    let launched = unsafe { ShellExecuteExW(&mut info) };
    if launched == 0 {
        let err = unsafe { GetLastError() };
        return if err == ERROR_CANCELLED {
            Err("elevation denied: user declined the UAC prompt".to_string())
        } else {
            Err(format!(
                "ShellExecuteExW failed to launch elevated helper (GetLastError = {err})"
            ))
        };
    }

    if info.hProcess.is_null() {
        // Should not happen given SEE_MASK_NOCLOSEPROCESS + launched != 0, but guard
        // against silently returning Ok() without ever having waited on anything.
        return Err("ShellExecuteExW reported success but returned no process handle".to_string());
    }

    // SAFETY: info.hProcess is a valid, open process handle owned by us (thanks to
    // SEE_MASK_NOCLOSEPROCESS) until we CloseHandle it below.
    let wait_result = unsafe { WaitForSingleObject(info.hProcess, INFINITE) };
    if wait_result != WAIT_OBJECT_0 {
        unsafe { CloseHandle(info.hProcess) };
        return Err(format!(
            "failed to wait for elevated helper to exit (WaitForSingleObject returned {wait_result})"
        ));
    }

    let mut exit_code: u32 = 0;
    // SAFETY: same handle as above, still valid; exit_code is a valid out-pointer.
    let got_exit_code = unsafe { GetExitCodeProcess(info.hProcess, &mut exit_code) };
    unsafe { CloseHandle(info.hProcess) };

    if got_exit_code == 0 {
        let err = unsafe { GetLastError() };
        return Err(format!(
            "elevated helper exited but its exit code could not be retrieved (GetLastError = {err})"
        ));
    }

    if exit_code == 0 {
        Ok(())
    } else {
        Err(format!(
            "elevated helper exited with status {exit_code} (denied, or the manifest apply failed)"
        ))
    }
}

/// Encodes `s` as a NUL-terminated UTF-16 buffer suitable for a `PCWSTR` field.
/// The returned `Vec` must outlive any FFI call that holds a pointer into it.
fn to_wide_nul(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Quotes `path` as a single `CommandLineToArgvW` argument and encodes it as a
/// NUL-terminated UTF-16 buffer for use as `lpParameters`. `lpParameters` is one flat
/// string (unlike `Command::arg`, which builds argv entries directly), so a path
/// containing spaces must be wrapped in escaped double quotes or it gets split into
/// multiple arguments on the helper side — this follows the same backslash/quote
/// escaping algorithm `CommandLineToArgvW` (and thus `std::env::args()` in
/// `helper/src/main.rs`) expects, including the trailing-backslash-before-closing-quote
/// edge case.
fn quote_for_argv(path: &Path) -> Vec<u16> {
    const SPACE: u16 = 0x20;
    const TAB: u16 = 0x09;
    const QUOTE: u16 = 0x22;
    const BACKSLASH: u16 = 0x5C;

    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    let needs_quotes = units.is_empty()
        || units
            .iter()
            .any(|&c| c == SPACE || c == TAB || c == QUOTE);

    if !needs_quotes {
        let mut out = units;
        out.push(0);
        return out;
    }

    let mut out: Vec<u16> = Vec::with_capacity(units.len() + 3);
    out.push(QUOTE);

    let mut i = 0;
    while i < units.len() {
        let mut backslashes = 0usize;
        while i < units.len() && units[i] == BACKSLASH {
            backslashes += 1;
            i += 1;
        }

        if i == units.len() {
            // Trailing run of backslashes right before the closing quote: double them
            // so they don't escape it.
            out.extend(std::iter::repeat(BACKSLASH).take(backslashes * 2));
        } else if units[i] == QUOTE {
            // Backslashes immediately before a literal quote: double them, then
            // escape the quote itself.
            out.extend(std::iter::repeat(BACKSLASH).take(backslashes * 2 + 1));
            out.push(QUOTE);
            i += 1;
        } else {
            // Backslashes not followed by a quote carry through unchanged.
            out.extend(std::iter::repeat(BACKSLASH).take(backslashes));
            out.push(units[i]);
            i += 1;
        }
    }

    out.push(QUOTE);
    out.push(0);
    out
}
