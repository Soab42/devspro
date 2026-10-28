//! Windows. `netstat -ano` would be the obvious stand-in for `ss -tlnp`, but its state
//! column ("LISTENING") is localised on a non-English Windows, so a parser keyed to that
//! word silently returns nothing on the machines it wasn't written for. `GetExtendedTcpTable`
//! is the API netstat itself is built on: it filters to listeners for us, reports the
//! owning pid per socket, and knows nothing about display language.
//!
//! Process names come from a ToolHelp snapshot rather than `OpenProcess`, which would
//! need rights this app doesn't have for anything running as another user — the same
//! ports would then show up nameless.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, INVALID_HANDLE_VALUE};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
    TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
    TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::super::PortEntry;

/// `AF_INET` / `AF_INET6` from WinSock. Passed to `GetExtendedTcpTable` as a plain `u32`,
/// and not re-exported as constants by `windows-sys` under the features this crate takes.
const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;

/// Without this, every `taskkill` would briefly flash a console window: this app is a GUI
/// process, so Windows creates one for any console child that doesn't opt out.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Needs only `PROCESS_QUERY_LIMITED_INFORMATION`, which most processes grant even to
/// another user — a service such as WAMP's Apache included. Protected processes refuse,
/// and come back `None`.
pub fn exe_path(pid: u32) -> Option<String> {
    // SAFETY: plain call; the handle is checked and closed on every path out.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` UTF-16 units; the API writes at most that many and
    // updates `len` to the count written.
    let ok = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) };
    // SAFETY: `process` is a live handle from OpenProcess above.
    unsafe { CloseHandle(process) };
    (ok != 0 && len > 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

pub fn list_listening_ports() -> Result<Vec<PortEntry>, String> {
    let mut sockets = listening_sockets(AF_INET)?;
    sockets.extend(listening_sockets(AF_INET6)?);

    let names = process_names();
    let mut entries: Vec<PortEntry> = sockets
        .into_iter()
        .map(|(port, address, pid)| PortEntry {
            port,
            address,
            // Pid 0 is the System Idle Process, which never owns a socket — the table
            // uses it for a listener whose owner the caller isn't allowed to see.
            pid: (pid != 0).then_some(pid),
            process_name: names.get(&pid).cloned(),
            // The Linux backend fills this from /proc/<pid>/cmdline. Windows keeps another
            // process's command line in its PEB, reachable only by reading that process's
            // memory — which needs rights this app doesn't ask for and would fail for
            // anything running as another user. The UI already renders a missing command
            // as blank, so this stays honestly absent rather than guessed at from the
            // executable name.
            command: None,
        })
        .collect();

    entries.sort_by_key(|e| e.port);
    Ok(entries)
}

pub fn kill(pid: u32, force: bool) -> Result<String, String> {
    let mut command = Command::new("taskkill");
    command.args(["/PID", &pid.to_string()]);
    if force {
        command.arg("/F");
    }

    let output = command
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("failed to run taskkill: {e}"))?;

    if output.status.success() {
        // Deliberately not `/T`: the Linux backend signals exactly the pid the user
        // picked and leaves its children alone, and this page is about freeing one port,
        // not tearing down a tree.
        return Ok(format!(
            "asked pid {pid} to {}",
            if force { "terminate" } else { "close" }
        ));
    }

    // taskkill explains a refusal ("This process can only be terminated forcefully") on
    // stdout, not stderr, so both are worth looking at before falling back.
    let reason = [&output.stderr, &output.stdout]
        .into_iter()
        .map(|stream| String::from_utf8_lossy(stream).trim().to_string())
        .find(|text| !text.is_empty())
        .unwrap_or_else(|| format!("taskkill exited with {}", output.status));
    Err(reason)
}

/// Every listening socket for one address family, as `(port, address, owning pid)`.
fn listening_sockets(family: u32) -> Result<Vec<(u16, String, u32)>, String> {
    let mut buf = query_tcp_table(family)?;

    // SAFETY: `query_tcp_table` returned a buffer the API filled for this family, so it
    // starts with the matching table header. `table` is a C flexible array member declared
    // as `[T; 1]`, and `dwNumEntries` is how many entries were actually written — which is
    // what makes reading that many as a slice sound. The buffer is aligned because it was
    // allocated as `Vec<u32>`, whose alignment satisfies both row types.
    unsafe {
        Ok(if family == AF_INET {
            let table = buf.as_mut_ptr().cast::<MIB_TCPTABLE_OWNER_PID>();
            std::slice::from_raw_parts((*table).table.as_ptr(), (*table).dwNumEntries as usize)
                .iter()
                .map(|row| {
                    (
                        port_from_network_order(row.dwLocalPort),
                        IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_le_bytes())).to_string(),
                        row.dwOwningPid,
                    )
                })
                .collect()
        } else {
            let table = buf.as_mut_ptr().cast::<MIB_TCP6TABLE_OWNER_PID>();
            std::slice::from_raw_parts((*table).table.as_ptr(), (*table).dwNumEntries as usize)
                .iter()
                .map(|row| {
                    (
                        port_from_network_order(row.dwLocalPort),
                        IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)).to_string(),
                        row.dwOwningPid,
                    )
                })
                .collect()
        })
    }
}

/// Asks for the listener table, growing the buffer until it fits. The size is re-read on
/// every attempt because sockets can open between the sizing call and the fetch.
fn query_tcp_table(family: u32) -> Result<Vec<u32>, String> {
    // Allocated as u32 rather than u8 so the buffer is 4-byte aligned for the row structs.
    let mut buf: Vec<u32> = Vec::new();
    let mut size: u32 = 0;

    for _ in 0..5 {
        // SAFETY: on the first pass this is the documented sizing call (null buffer, zero
        // length), and afterwards `buf` holds at least `size` bytes — the amount the
        // previous call asked for.
        let rc = unsafe {
            GetExtendedTcpTable(
                if buf.is_empty() {
                    std::ptr::null_mut()
                } else {
                    buf.as_mut_ptr().cast()
                },
                &mut size,
                0, // unsorted: this module sorts by port itself
                family,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };

        match rc {
            ERROR_SUCCESS => return Ok(buf),
            ERROR_INSUFFICIENT_BUFFER => {
                buf = vec![0u32; (size as usize).div_ceil(std::mem::size_of::<u32>()).max(1)];
            }
            other => return Err(format!("GetExtendedTcpTable failed with code {other}")),
        }
    }
    Err("GetExtendedTcpTable kept asking for a bigger buffer".to_string())
}

/// `dwLocalPort` holds a 16-bit port in network byte order inside a 32-bit field, so the
/// low two bytes are the port with its halves the wrong way round.
fn port_from_network_order(raw: u32) -> u16 {
    u16::from_be_bytes([(raw & 0xff) as u8, ((raw >> 8) & 0xff) as u8])
}

/// pid → executable name for every process this user can enumerate.
fn process_names() -> HashMap<u32, String> {
    let mut names = HashMap::new();

    // SAFETY: no arguments to get wrong; the returned handle is checked below and closed
    // on every path out.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
        return names;
    }

    // SAFETY: PROCESSENTRY32W is plain C data, and `dwSize` — which the API requires be
    // set before the first call — is filled in immediately.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    // SAFETY: `snapshot` is open and `entry` is a valid, correctly-sized out-parameter for
    // the whole walk below.
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        let name = from_wide_nul(&entry.szExeFile);
        if !name.is_empty() {
            names.insert(entry.th32ProcessID, name);
        }
        ok = unsafe { Process32NextW(snapshot, &mut entry) };
    }

    // SAFETY: the snapshot handle is ours and closed exactly once.
    unsafe { CloseHandle(snapshot) };
    names
}

/// Decodes a fixed-size UTF-16 buffer the OS filled and NUL-terminated, ignoring whatever
/// follows the terminator.
fn from_wide_nul(units: &[u16]) -> String {
    let end = units.iter().position(|&unit| unit == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Getting this wrong is the classic mistake in this API and it fails quietly: port
    /// 3000 read without the swap is 47115, so the page would list plausible-looking
    /// ports that match nothing the user is actually running.
    #[test]
    fn reads_the_port_out_of_network_byte_order() {
        // 3000 = 0x0BB8, network order 0xB80B, which is what the field holds.
        assert_eq!(port_from_network_order(0x0000_B80B), 3000);
        assert_eq!(port_from_network_order(0x0000_5000), 80);
        assert_eq!(port_from_network_order(0x0000_BB01), 443);
        // The upper half of the field is reserved and must not leak into the answer.
        assert_eq!(port_from_network_order(0xdead_B80B), 3000);
    }

    /// Exercises the real API against the live machine — a wrong table layout or a missed
    /// out-parameter produces plausible nonsense rather than a compile error, and every
    /// Windows box has something listening (SMB, RPC) to check the shape against.
    #[test]
    fn lists_real_listening_ports_with_owners() {
        let entries = list_listening_ports().expect("listing should succeed on Windows");
        assert!(!entries.is_empty(), "a Windows machine always has TCP listeners");

        assert!(
            entries.windows(2).all(|pair| pair[0].port <= pair[1].port),
            "entries are sorted by port"
        );
        for entry in &entries {
            assert_ne!(entry.port, 0, "port 0 is never a real listener");
            assert!(!entry.address.is_empty());
            assert_ne!(entry.pid, Some(0), "pid 0 means 'owner hidden', not a process");
        }
        assert!(
            entries.iter().any(|entry| entry.process_name.is_some()),
            "the ToolHelp snapshot should name at least some owners"
        );
    }
}
