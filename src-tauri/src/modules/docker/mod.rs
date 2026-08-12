//! Docker container manager — start/stop/restart, status, and logs. Shells out to
//! the `docker` CLI rather than the Engine API: no extra HTTP-over-socket client
//! needed, and it naturally respects however the user already authenticates to
//! their daemon (docker group membership, rootless, remote DOCKER_HOST, ...).
//!
//! Deliberately no elevation and no local persistence — unlike nginx/SSL sites,
//! there's nothing here this app "owns"; it's just a live view over containers
//! that already exist. A user in the `docker` group needs no sudo prompt at all,
//! which is the common case and why this module has no `elevate` involvement.

use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Serialize)]
pub struct DockerDetection {
    pub found: bool,
    pub daemon_reachable: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

pub fn detect() -> DockerDetection {
    let binary_found = Command::new("which")
        .arg("docker")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !binary_found {
        return DockerDetection {
            found: false,
            daemon_reachable: false,
            version: None,
            error: None,
        };
    }

    match Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
    {
        Ok(out) if out.status.success() => DockerDetection {
            found: true,
            daemon_reachable: true,
            version: Some(String::from_utf8_lossy(&out.stdout).trim().to_string()),
            error: None,
        },
        Ok(out) => DockerDetection {
            found: true,
            daemon_reachable: false,
            version: None,
            error: Some(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        },
        Err(e) => DockerDetection {
            found: true,
            daemon_reachable: false,
            version: None,
            error: Some(e.to_string()),
        },
    }
}

#[derive(Debug, Deserialize)]
struct RawContainer {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Names")]
    names: String,
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "State")]
    state: String,
    #[serde(rename = "Status")]
    status: String,
    #[serde(rename = "Ports")]
    ports: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub ports: String,
}

pub fn list_containers() -> Result<Vec<Container>, String> {
    let output = Command::new("docker")
        .args(["ps", "-a", "--format", "{{json .}}"])
        .output()
        .map_err(|e| format!("failed to run docker: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let raw: RawContainer =
                serde_json::from_str(line).map_err(|e| format!("failed to parse docker output: {e}"))?;
            Ok(Container {
                id: raw.id,
                name: raw.names,
                image: raw.image,
                state: raw.state,
                status: raw.status,
                ports: raw.ports,
            })
        })
        .collect()
}

fn run_action(action: &str, id: &str) -> Result<String, String> {
    let output = Command::new("docker")
        .args([action, id])
        .output()
        .map_err(|e| format!("failed to run docker {action}: {e}"))?;
    if output.status.success() {
        Ok(format!("{action}ed {id}"))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[tauri::command]
pub fn docker_detect() -> DockerDetection {
    detect()
}

#[tauri::command]
pub fn docker_list_containers() -> Result<Vec<Container>, String> {
    list_containers()
}

#[tauri::command]
pub fn docker_start(id: String) -> Result<String, String> {
    run_action("start", &id)
}

#[tauri::command]
pub fn docker_stop(id: String) -> Result<String, String> {
    run_action("stop", &id)
}

#[tauri::command]
pub fn docker_restart(id: String) -> Result<String, String> {
    run_action("restart", &id)
}

struct ComposeInfo {
    service: String,
    compose_file: String,
}

/// Docker has no "remap this port" verb — port bindings are fixed at container
/// creation. The only real way to change one is to recreate the container. For a
/// container created by `docker compose` (identified via its own labels), that
/// means editing the `ports:` line in the actual compose file and recreating just
/// that service — which also makes the change durable (survives the next
/// `docker compose up`), unlike a one-off `docker run` replacement would.
fn compose_info(id: &str) -> Result<ComposeInfo, String> {
    let output = Command::new("docker")
        .args([
            "inspect",
            id,
            "--format",
            "{{index .Config.Labels \"com.docker.compose.service\"}}\t{{index .Config.Labels \"com.docker.compose.project.config_files\"}}",
        ])
        .output()
        .map_err(|e| format!("failed to inspect container: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.trim().split('\t');
    let (service, compose_files) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if service.is_empty() || compose_files.is_empty() {
        return Err("this container wasn't created by docker compose, so its ports can't be edited this way".to_string());
    }
    let compose_file = compose_files.split(',').next().unwrap_or(compose_files).trim().to_string();
    Ok(ComposeInfo {
        service: service.to_string(),
        compose_file,
    })
}

/// Finds the `- "<host>:<container_port>"` line under `service:`'s `ports:` list
/// and swaps in `new_host_port`, preserving indentation and quote style. Only
/// understands the common block/dash-list compose syntax — anything else (flow
/// style, long `target:`/`published:` mappings) fails clearly rather than
/// guessing and corrupting the file.
fn rewrite_port_mapping(contents: &str, service: &str, container_port: u16, new_host_port: u16) -> Result<String, String> {
    let lines: Vec<&str> = contents.lines().collect();
    let indent_of = |l: &str| l.len() - l.trim_start().len();

    let service_start = lines
        .iter()
        .position(|l| l.trim_start().trim_end() == format!("{service}:"))
        .ok_or_else(|| format!("could not find service '{service}' in the compose file"))?;
    let service_indent = indent_of(lines[service_start]);

    let service_end = lines[(service_start + 1)..]
        .iter()
        .position(|l| !l.trim().is_empty() && indent_of(l) <= service_indent)
        .map(|i| service_start + 1 + i)
        .unwrap_or(lines.len());

    let ports_line = lines[(service_start + 1)..service_end]
        .iter()
        .position(|l| l.trim_start().trim_end() == "ports:")
        .map(|i| service_start + 1 + i)
        .ok_or_else(|| format!("service '{service}' has no ports: list in the compose file"))?;
    let ports_indent = indent_of(lines[ports_line]);

    let mut target_idx = None;
    for (i, l) in lines[(ports_line + 1)..service_end].iter().enumerate() {
        if l.trim().is_empty() {
            continue;
        }
        if indent_of(l) <= ports_indent {
            break;
        }
        let item = l.trim_start().trim_start_matches('-').trim();
        let unquoted = item.trim_matches('"').trim_matches('\'');
        if let Some((_host, cont)) = unquoted.rsplit_once(':') {
            if cont.parse::<u16>() == Ok(container_port) {
                target_idx = Some(ports_line + 1 + i);
                break;
            }
        }
    }
    let target_idx =
        target_idx.ok_or_else(|| format!("could not find a port mapping for container port {container_port} under service '{service}'"))?;

    let old_line = lines[target_idx];
    let leading_ws = &old_line[..indent_of(old_line)];
    let dash_and_rest = old_line.trim_start();
    let quote = if dash_and_rest.contains('"') {
        '"'
    } else if dash_and_rest.contains('\'') {
        '\''
    } else {
        '\0'
    };
    let mapping = format!("{new_host_port}:{container_port}");
    let new_item = if quote == '\0' {
        format!("- {mapping}")
    } else {
        format!("- {quote}{mapping}{quote}")
    };

    let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    new_lines[target_idx] = format!("{leading_ws}{new_item}");
    Ok(new_lines.join("\n") + "\n")
}

pub fn change_port(id: &str, container_port: u16, new_host_port: u16) -> Result<String, String> {
    let info = compose_info(id)?;
    let contents =
        std::fs::read_to_string(&info.compose_file).map_err(|e| format!("could not read {}: {e}", info.compose_file))?;
    let updated = rewrite_port_mapping(&contents, &info.service, container_port, new_host_port)?;
    std::fs::write(&info.compose_file, updated).map_err(|e| format!("could not write {}: {e}", info.compose_file))?;

    let output = super::compose::build(&["-f", &info.compose_file, "up", "-d", &info.service])?
        .output()
        .map_err(|e| format!("failed to run docker compose: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "updated {} but recreating the container failed: {}",
            info.compose_file,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(format!(
        "{} now maps host port {new_host_port} -> container port {container_port}",
        info.service
    ))
}

#[tauri::command]
pub fn docker_change_port(id: String, container_port: u16, new_host_port: u16) -> Result<String, String> {
    change_port(&id, container_port, new_host_port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_port_mapping_replaces_matching_container_port() {
        let compose = "\
services:
  redis:
    image: redis:7
    ports:
      - \"6379:6379\"
  mysql:
    image: mysql:8
    ports:
      - \"3306:3306\"
";
        let updated = rewrite_port_mapping(compose, "redis", 6379, 16379).unwrap();
        assert!(updated.contains("      - \"16379:6379\""));
        // Untouched service keeps its original mapping.
        assert!(updated.contains("      - \"3306:3306\""));
    }

    #[test]
    fn rewrite_port_mapping_preserves_unquoted_style() {
        let compose = "\
services:
  app:
    ports:
      - 8080:80
";
        let updated = rewrite_port_mapping(compose, "app", 80, 9090).unwrap();
        assert!(updated.contains("      - 9090:80"));
    }

    #[test]
    fn rewrite_port_mapping_errors_on_missing_service() {
        let compose = "services:\n  app:\n    ports:\n      - \"80:80\"\n";
        let err = rewrite_port_mapping(compose, "missing", 80, 81).unwrap_err();
        assert!(err.contains("missing"));
    }

    #[test]
    fn rewrite_port_mapping_errors_on_missing_container_port() {
        let compose = "services:\n  app:\n    ports:\n      - \"80:80\"\n";
        let err = rewrite_port_mapping(compose, "app", 443, 8443).unwrap_err();
        assert!(err.contains("443"));
    }

    #[test]
    fn rewrite_port_mapping_errors_on_missing_ports_block() {
        let compose = "services:\n  app:\n    image: foo\n";
        let err = rewrite_port_mapping(compose, "app", 80, 81).unwrap_err();
        assert!(err.contains("ports:"));
    }
}

/// Combines stdout+stderr from `docker logs`. The two streams are captured
/// separately and concatenated, not merged in true chronological order — an
/// acceptable simplification for a quick-look viewer, not a substitute for
/// `docker logs -f` in a real terminal.
#[tauri::command]
pub fn docker_logs(id: String, tail: u32) -> Result<String, String> {
    let output = Command::new("docker")
        .args(["logs", "--tail", &tail.to_string(), &id])
        .output()
        .map_err(|e| format!("failed to fetch logs: {e}"))?;
    let mut combined = String::from_utf8_lossy(&output.stdout).to_string();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    if combined.is_empty() {
        combined.push_str("(no log output)");
    }
    Ok(combined)
}
