//! Linux. Built on `ss` (iproute2) and `/proc/<pid>/cmdline` — this is the code the
//! module ran before the platform split, moved rather than rewritten, including the
//! two `ss`-output parsers and their tests.

use std::process::Command;

use super::super::PortEntry;

pub fn list_listening_ports() -> Result<Vec<PortEntry>, String> {
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

pub fn kill(pid: u32, force: bool) -> Result<String, String> {
    super::unix_kill(pid, force)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `ss -tlnp` packs the owner into one field, e.g.
    /// `users:(("node",pid=12345,fd=20))`.
    #[test]
    fn pulls_the_owner_out_of_ss_process_field() {
        let field = r#"users:(("node",pid=12345,fd=20))"#;
        assert_eq!(extract_pid(field), Some(12345));
        assert_eq!(extract_process_name(field).as_deref(), Some("node"));
    }

    #[test]
    fn reports_no_owner_when_ss_could_not_see_one() {
        // Without root, `ss` prints the socket but omits the users:(...) part for
        // processes belonging to another user.
        assert_eq!(extract_pid(""), None);
        assert_eq!(extract_process_name(""), None);
    }
}
