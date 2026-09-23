//! Lists every listening TCP port and whatever process owns it (the node/next/php/
//! whatever-dev-server processes people forget running in a background terminal),
//! with a way to kill them. Linux only for v1 — built on `ss` (iproute2) and
//! `/proc/<pid>/cmdline`, neither of which has an equivalent invoked the same way
//! on macOS/Windows.
//!
//! No elevation: a normal `kill` only works on processes owned by the same user
//! anyway, which covers the actual use case (a dev server *you* started). Killing
//! someone else's/root's process is out of scope on purpose — this isn't a task
//! manager, and adding a sudo path here would be an easy way to shoot yourself in
//! the foot.

use serde::Serialize;
use std::process::Command;

#[derive(Debug, Serialize, Clone)]
pub struct PortEntry {
    pub port: u16,
    pub address: String,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub command: Option<String>,
}

fn extract_pid(process_field: &str) -> Option<u32> {
    let idx = process_field.find("pid=")?;
    let rest = &process_field[idx + 4..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn extract_process_name(process_field: &str) -> Option<String> {
    let idx = process_field.find("((\"")?;
    let rest = &process_field[idx + 3..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[cfg(target_os = "linux")]
fn read_cmdline(pid: u32) -> Option<String> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let parts: Vec<String> = raw
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).to_string())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

#[cfg(not(target_os = "linux"))]
fn read_cmdline(_pid: u32) -> Option<String> {
    None
}

pub fn list_ports() -> Result<Vec<PortEntry>, String> {
    if cfg!(not(target_os = "linux")) {
        return Err("listing ports is only supported on Linux right now".to_string());
    }

    let output = Command::new("ss")
        .args(["-tlnp"])
        .output()
        .map_err(|e| format!("failed to run ss: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut entries: Vec<PortEntry> = text
        .lines()
        .skip(1) // header
        .filter_map(|line| {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            let local = tokens.get(3)?;
            let (addr_raw, port_str) = local.rsplit_once(':')?;
            let port = port_str.parse::<u16>().ok()?;
            let address = addr_raw.trim_start_matches('[').trim_end_matches(']').to_string();
            let process_field = tokens.get(5..).map(|t| t.join(" ")).unwrap_or_default();
            let pid = extract_pid(&process_field);
            let process_name = extract_process_name(&process_field);
            let command = pid.and_then(read_cmdline);
            Some(PortEntry {
                port,
                address,
                pid,
                process_name,
                command,
            })
        })
        .collect();

    entries.sort_by_key(|e| e.port);
    Ok(entries)
}

#[tauri::command(async)]
pub fn processes_list_ports() -> Result<Vec<PortEntry>, String> {
    list_ports()
}

#[tauri::command(async)]
pub fn processes_kill(pid: u32, force: bool) -> Result<String, String> {
    let signal = if force { "-9" } else { "-15" };
    let output = Command::new("kill")
        .args([signal, &pid.to_string()])
        .output()
        .map_err(|e| format!("failed to run kill: {e}"))?;
    if output.status.success() {
        Ok(format!(
            "sent {} to pid {pid}",
            if force { "SIGKILL" } else { "SIGTERM" }
        ))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}
