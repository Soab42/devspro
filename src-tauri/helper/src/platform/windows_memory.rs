//! Windows: the Memory module's three operations.
//!
//! These go straight to Win32 instead of `sc.exe` and friends, through a few hand-written
//! `extern` declarations rather than a crate, keeping this binary dependency-free: `sc
//! start`/`sc stop` return while the service is still starting or stopping and report the
//! outcome only in localised text, and there is no shipped tool at all for the standby
//! list.
//!
//! - `service_control` — start or stop a service through the Service Control Manager and
//!   wait (up to 30 s) until it has actually started or stopped.
//! - `drop_caches` — the counterpart of `sync; echo 3 > drop_caches`: write modified
//!   pages out, then empty the standby list, the RAM Windows holds cached file and
//!   process pages in. Both are commands to `NtSetSystemInformation` (the call RAMMap's
//!   "Empty Standby List" makes); they need administrator rights plus
//!   SeProfileSingleProcessPrivilege, which an elevated token holds but has to switch
//!   on.
//! - `reclaim_swap` — not available: nothing in Windows pages memory back in on demand.

use std::ffi::c_void;
use std::io;
use std::time::{Duration, Instant};

use crate::OpResult;

type Handle = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct ServiceStatus {
    service_type: u32,
    current_state: u32,
    controls_accepted: u32,
    win32_exit_code: u32,
    service_specific_exit_code: u32,
    check_point: u32,
    wait_hint: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Luid {
    low: u32,
    high: i32,
}

/// TOKEN_PRIVILEGES with its one-element array spelled out.
#[repr(C)]
struct TokenPrivileges {
    count: u32,
    luid: Luid,
    attributes: u32,
}

#[link(name = "advapi32")]
extern "system" {
    fn OpenSCManagerW(machine: *const u16, database: *const u16, access: u32) -> Handle;
    fn OpenServiceW(scm: Handle, name: *const u16, access: u32) -> Handle;
    fn StartServiceW(service: Handle, argc: u32, argv: *const *const u16) -> i32;
    fn ControlService(service: Handle, control: u32, status: *mut ServiceStatus) -> i32;
    fn QueryServiceStatus(service: Handle, status: *mut ServiceStatus) -> i32;
    fn CloseServiceHandle(handle: Handle) -> i32;
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    fn LookupPrivilegeValueW(system: *const u16, name: *const u16, luid: *mut Luid) -> i32;
    fn AdjustTokenPrivileges(
        token: Handle,
        disable_all: i32,
        new_state: *const TokenPrivileges,
        buffer_length: u32,
        previous_state: *mut c_void,
        return_length: *mut u32,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
}

#[link(name = "ntdll")]
extern "system" {
    fn NtSetSystemInformation(class: i32, info: *mut c_void, length: u32) -> i32;
}

const SC_MANAGER_CONNECT: u32 = 0x0001;
const SERVICE_QUERY_STATUS: u32 = 0x0004;
const SERVICE_START: u32 = 0x0010;
const SERVICE_STOP: u32 = 0x0020;
const SERVICE_CONTROL_STOP: u32 = 1;
const SERVICE_STOPPED: u32 = 1;
const SERVICE_RUNNING: u32 = 4;
const ERROR_SERVICE_ALREADY_RUNNING: i32 = 1056;
const ERROR_SERVICE_NOT_ACTIVE: i32 = 1062;

const TOKEN_QUERY: u32 = 0x0008;
const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
const SE_PRIVILEGE_ENABLED: u32 = 0x0002;
const ERROR_NOT_ALL_ASSIGNED: i32 = 1300;

const SYSTEM_MEMORY_LIST_INFORMATION: i32 = 80;
const MEMORY_FLUSH_MODIFIED_LIST: u32 = 3;
const MEMORY_PURGE_STANDBY_LIST: u32 = 4;

/// How long a start or stop may take before it's reported as still pending.
const SERVICE_WAIT: Duration = Duration::from_secs(30);

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A Service Control Manager handle, closed on drop.
struct ScHandle(Handle);

impl Drop for ScHandle {
    fn drop(&mut self) {
        // SAFETY: only built from a non-null handle, closed exactly once.
        unsafe { CloseServiceHandle(self.0) };
    }
}

fn state_of(service: &ScHandle) -> io::Result<u32> {
    let mut status = ServiceStatus::default();
    // SAFETY: open handle with SERVICE_QUERY_STATUS, valid out-parameter.
    if unsafe { QueryServiceStatus(service.0, &mut status) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(status.current_state)
}

fn failure(message: String) -> OpResult {
    OpResult { success: false, message }
}

/// `action` and `unit` were already checked by the caller (start/stop only, and a name
/// that matches the unit-name rule).
pub fn service_control(action: &str, unit: &str) -> OpResult {
    let starting = action == "start";
    // SAFETY: null machine and database mean the local, active database.
    let scm = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if scm.is_null() {
        return failure(format!("could not open the service manager: {}", io::Error::last_os_error()));
    }
    let scm = ScHandle(scm);
    let name = wide(unit);
    let access = SERVICE_QUERY_STATUS | if starting { SERVICE_START } else { SERVICE_STOP };
    // SAFETY: `name` is NUL-terminated and outlives the call.
    let service = unsafe { OpenServiceW(scm.0, name.as_ptr(), access) };
    if service.is_null() {
        return failure(format!("could not open service {unit}: {}", io::Error::last_os_error()));
    }
    let service = ScHandle(service);

    let requested = if starting {
        // SAFETY: open handle with SERVICE_START; no arguments.
        unsafe { StartServiceW(service.0, 0, std::ptr::null()) }
    } else {
        let mut status = ServiceStatus::default();
        // SAFETY: open handle with SERVICE_STOP, valid out-parameter.
        unsafe { ControlService(service.0, SERVICE_CONTROL_STOP, &mut status) }
    };
    if requested == 0 {
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(ERROR_SERVICE_ALREADY_RUNNING) if starting => {
                return OpResult { success: true, message: format!("{unit} was already running") }
            }
            Some(ERROR_SERVICE_NOT_ACTIVE) if !starting => {
                return OpResult { success: true, message: format!("{unit} was not running") }
            }
            _ => return failure(format!("could not {action} {unit}: {error}")),
        }
    }

    let wanted = if starting { SERVICE_RUNNING } else { SERVICE_STOPPED };
    let deadline = Instant::now() + SERVICE_WAIT;
    loop {
        match state_of(&service) {
            Ok(state) if state == wanted => {
                return OpResult {
                    success: true,
                    message: format!("{} {unit}", if starting { "started" } else { "stopped" }),
                }
            }
            // A service that stops on its own straight after starting failed to start.
            Ok(SERVICE_STOPPED) if starting => return failure(format!("{unit} stopped again right after starting")),
            Ok(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(250)),
            Ok(_) => {
                return failure(format!(
                    "{unit} is still {} after {} s",
                    if starting { "starting" } else { "stopping" },
                    SERVICE_WAIT.as_secs()
                ))
            }
            Err(e) => return failure(format!("could not read the state of {unit}: {e}")),
        }
    }
}

/// Switches on one privilege the elevated token holds but has disabled.
fn enable_privilege(name: &str) -> io::Result<()> {
    let mut token: Handle = std::ptr::null_mut();
    // SAFETY: the current-process pseudo-handle and a valid out-parameter.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut luid = Luid::default();
    let name = wide(name);
    // SAFETY: `name` is NUL-terminated; valid out-parameter.
    let result = if unsafe { LookupPrivilegeValueW(std::ptr::null(), name.as_ptr(), &mut luid) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        let privileges = TokenPrivileges { count: 1, luid, attributes: SE_PRIVILEGE_ENABLED };
        // SAFETY: open token with TOKEN_ADJUST_PRIVILEGES; no previous state asked for.
        let ok = unsafe { AdjustTokenPrivileges(token, 0, &privileges, 0, std::ptr::null_mut(), std::ptr::null_mut()) };
        // It "succeeds" without enabling anything when the token lacks the privilege,
        // and says so only through the last error.
        let error = io::Error::last_os_error();
        if ok == 0 || error.raw_os_error() == Some(ERROR_NOT_ALL_ASSIGNED) {
            Err(error)
        } else {
            Ok(())
        }
    };
    // SAFETY: opened above, closed once.
    unsafe { CloseHandle(token) };
    result
}

fn memory_list_command(command: u32) -> Result<(), String> {
    let mut command = command;
    // SAFETY: the information is one 4-byte command, passed with its size.
    let status = unsafe {
        NtSetSystemInformation(SYSTEM_MEMORY_LIST_INFORMATION, (&mut command as *mut u32).cast(), 4)
    };
    if status >= 0 {
        Ok(())
    } else {
        Err(format!("NTSTATUS {:#010x}", status as u32))
    }
}

pub fn drop_caches() -> OpResult {
    if let Err(e) = enable_privilege("SeProfileSingleProcessPrivilege") {
        return failure(format!("could not enable the privilege that emptying the standby list needs: {e}"));
    }
    // Like `sync` on Linux: modified pages can't be dropped until they're written out.
    // A failure here is reported, but doesn't stop the purge.
    let flushed = memory_list_command(MEMORY_FLUSH_MODIFIED_LIST).is_ok();
    match memory_list_command(MEMORY_PURGE_STANDBY_LIST) {
        Ok(()) => OpResult {
            success: true,
            message: if flushed {
                "wrote modified pages out and emptied the standby list (Windows' file cache)".to_string()
            } else {
                "emptied the standby list (writing modified pages out failed, so those stayed)".to_string()
            },
        },
        Err(e) => failure(format!("could not empty the standby list: {e}")),
    }
}

pub fn reclaim_swap() -> OpResult {
    failure(
        "not available on Windows: it has no way to move paged-out memory back into RAM on demand — \
         each app's pages come back as the app uses them"
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_service_is_reported_not_waited_on() {
        let r = service_control("stop", "devspro-no-such-service");
        assert!(!r.success);
        assert!(r.message.contains("devspro-no-such-service"), "{}", r.message);
    }

    #[test]
    fn reclaiming_swap_says_why_it_is_unavailable() {
        let r = reclaim_swap();
        assert!(!r.success);
        assert!(r.message.starts_with("not available on Windows"));
    }

    /// Run by hand: elevated it really empties the standby list (harmless, but it costs
    /// the machine its file cache); unelevated it must fail on the privilege, cleanly.
    /// `cargo test -p ldm-helper drop_caches -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn drop_caches_on_this_machine() {
        let r = drop_caches();
        println!("{} — {}", r.success, r.message);
    }
}
