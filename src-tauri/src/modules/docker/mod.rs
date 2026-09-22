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
use std::path::Path;
use std::process::Command;

#[derive(Debug, Serialize)]
pub struct DockerDetection {
    pub found: bool,
    pub daemon_reachable: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

/// Two subprocess spawns down to one: the `which docker` probe is gone, because
/// spawning `docker` already answers the same question. `Err(NotFound)` from
/// `output()` *is* "the binary isn't on PATH", and every other spawn error is
/// something the caller needs to see rather than have reported as a missing
/// Docker. This runs on every page open and used to run again after every
/// container action, so the saved spawn is not free.
pub fn detect() -> DockerDetection {
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
        // Ran, but refused: docker is installed and the daemon is not answering.
        Ok(out) => DockerDetection {
            found: true,
            daemon_reachable: false,
            version: None,
            error: Some(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DockerDetection {
            found: false,
            daemon_reachable: false,
            version: None,
            error: None,
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
    #[serde(rename = "Labels", default)]
    labels: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub ports: String,
    /// The `com.docker.compose.project` label, i.e. the stack this container
    /// belongs to. `None` for a plain `docker run` container, which the UI groups
    /// under "Standalone" rather than hiding.
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
}

/// `docker ps` renders labels as one flat `k=v,k=v` string, so this is a split
/// rather than real parsing. Fine for the two compose labels read here — both are
/// identifiers that cannot contain a comma. Deliberately NOT used for
/// `com.docker.compose.project.config_files`, which holds paths and *is*
/// comma-separated when a stack has several files; that one is read per-project
/// via `docker inspect` (see `project_compose_file`) where it arrives unambiguous.
fn label(labels: &str, key: &str) -> Option<String> {
    labels
        .split(',')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
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
                compose_project: label(&raw.labels, "com.docker.compose.project"),
                compose_service: label(&raw.labels, "com.docker.compose.service"),
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

/// One container's outcome within a bulk run. Bulk actions return this per id
/// instead of a `Result` for the batch: stopping nine of ten containers is a
/// materially different outcome from stopping none, and the caller has to be able
/// to say which one failed.
#[derive(Debug, Serialize)]
pub struct ActionOutcome {
    pub id: String,
    pub success: bool,
    pub message: String,
}

/// Applies a lifecycle verb to many containers.
///
/// Sequential, not parallel. `docker stop` is mostly a 10-second wait on SIGTERM,
/// so fanning out would genuinely be faster — but the daemon serialises much of
/// this anyway, and a predictable order makes a partial failure legible ("these
/// three succeeded, then this one failed") rather than interleaved noise.
fn bulk(action: &str, ids: &[String]) -> Vec<ActionOutcome> {
    ids.iter()
        .map(|id| match run_action(action, id) {
            Ok(message) => ActionOutcome {
                id: id.clone(),
                success: true,
                message,
            },
            Err(message) => ActionOutcome {
                id: id.clone(),
                success: false,
                message,
            },
        })
        .collect()
}

#[tauri::command]
pub fn docker_bulk_action(action: String, ids: Vec<String>) -> Result<Vec<ActionOutcome>, String> {
    // The verb reaches the `docker` argv, so it is checked against a fixed list
    // rather than passed through — the frontend is not the right place for that
    // guarantee to live.
    if !matches!(action.as_str(), "start" | "stop" | "restart") {
        return Err(format!("unsupported bulk action: {action}"));
    }
    Ok(bulk(&action, &ids))
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

/// Locates the compose file backing a running stack, by asking any one of its
/// containers where it came from. Compose records this on every container it
/// creates, so there is no need for this app to keep its own registry of stacks —
/// the source of truth is the daemon, and it stays correct even when the stack was
/// started from a terminal.
///
/// The catch: a stack with **no** containers left (a `down` was run) has nothing
/// to inspect and so disappears from this lookup entirely. That is why
/// `docker_compose_up_file` takes an explicit path — it is how a stack gets
/// started again once it is no longer represented on the daemon.
fn project_compose_file(project: &str) -> Result<String, String> {
    let output = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("label=com.docker.compose.project={project}"),
            "--format",
            "{{.ID}}",
        ])
        .output()
        .map_err(|e| format!("failed to run docker: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let ids = String::from_utf8_lossy(&output.stdout);
    let id = ids
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .ok_or_else(|| format!("no containers left for stack '{project}' — use \"Load compose file\" to start it again"))?;

    Ok(compose_info(id)?.compose_file)
}

/// Runs a compose subcommand against a file, from that file's own directory.
///
/// The working directory matters: relative `build:`, `env_file:` and bind-mount
/// paths inside a compose file resolve against it, and this app's cwd is wherever
/// the desktop launcher happened to start it. Without this, a stack that works
/// perfectly from a terminal fails here with a confusing missing-path error.
fn run_compose(compose_file: &str, project: Option<&str>, args: &[&str]) -> Result<String, String> {
    let dir = std::path::Path::new(compose_file)
        .parent()
        .ok_or_else(|| format!("could not determine the directory of {compose_file}"))?
        .to_path_buf();

    let mut full: Vec<&str> = Vec::new();
    if let Some(project) = project {
        full.extend(["-p", project]);
    }
    full.extend(["-f", compose_file]);
    full.extend(args);

    let output = super::compose::build(&full)?
        .current_dir(&dir)
        .output()
        .map_err(|e| format!("failed to run docker compose: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    // Compose reports its progress ("Container x  Started") on stderr even when it
    // succeeds, so that — not stdout — is what's worth showing back.
    let report = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Ok(if report.is_empty() {
        "done".to_string()
    } else {
        report
    })
}

/// Lifecycle verbs for a whole stack.
///
/// `up` is the interesting one, and the answer to "update from there": it
/// re-reads the compose file and recreates only the services whose definition
/// actually changed. Editing the file and hitting Update is therefore the way to
/// apply a change, where `restart` would just bounce the containers with their
/// old definition still in force.
#[tauri::command]
pub fn docker_compose_project_action(project: String, action: String) -> Result<String, String> {
    let compose_file = project_compose_file(&project)?;
    // Same reasoning as `docker_bulk_action`: the verb lands in argv, so the
    // allowed set is fixed here rather than trusted from the caller.
    let args: &[&str] = match action.as_str() {
        "up" => &["up", "-d", "--remove-orphans"],
        "start" => &["start"],
        "stop" => &["stop"],
        "restart" => &["restart"],
        "pull" => &["pull"],
        // Containers and the default network go; named volumes deliberately do
        // not. `down -v` would delete database data, which is not something a
        // menu item should be able to do without a far louder confirmation than
        // this app currently shows.
        "down" => &["down", "--remove-orphans"],
        other => return Err(format!("unsupported compose action: {other}")),
    };
    run_compose(&compose_file, Some(&project), args)
}

/// Brings up a stack from a compose file the user picked, including one that has
/// no containers on the daemon yet — the entry point for a stack this app has
/// never seen. Compose derives the project name from the containing directory,
/// exactly as it would from a terminal, so a stack loaded here and one started by
/// hand are the same stack rather than two competing copies.
#[tauri::command]
pub fn docker_compose_up_file(path: String) -> Result<String, String> {
    let file = std::path::Path::new(&path);
    if !file.is_file() {
        return Err(format!("{path} is not a file"));
    }
    run_compose(&path, None, &["up", "-d", "--remove-orphans"])
}

/// The compose file behind a container or a stack, with its text, ready to edit.
///
/// `service` is `Some` only when the source was a single container: both entry
/// points open the *same whole file*, and the service name is what narrows the
/// apply afterwards to `up -d <service>` instead of the whole stack.
#[derive(Debug, Serialize)]
pub struct ComposeSource {
    pub path: String,
    pub contents: String,
    pub project: Option<String>,
    pub service: Option<String>,
}

fn read_compose_source(
    compose_file: String,
    project: Option<String>,
    service: Option<String>,
) -> Result<ComposeSource, String> {
    let contents = std::fs::read_to_string(&compose_file)
        .map_err(|e| format!("could not read {compose_file}: {e}"))?;
    Ok(ComposeSource {
        path: compose_file,
        contents,
        project,
        service,
    })
}

#[tauri::command]
pub fn docker_read_container_compose(id: String) -> Result<ComposeSource, String> {
    let info = compose_info(&id)?;
    // Read the project off the container rather than deriving it from the
    // directory: a stack started with `-p` has a name the directory doesn't
    // predict, and applying under the wrong one would create a second copy of
    // every container instead of updating these.
    let project = inspect_label(&id, "com.docker.compose.project")?;
    read_compose_source(info.compose_file, project, Some(info.service))
}

#[tauri::command]
pub fn docker_read_project_compose(project: String) -> Result<ComposeSource, String> {
    let compose_file = project_compose_file(&project)?;
    read_compose_source(compose_file, Some(project), None)
}

fn inspect_label(id: &str, label: &str) -> Result<Option<String>, String> {
    let output = Command::new("docker")
        .args([
            "inspect",
            id,
            "--format",
            &format!("{{{{index .Config.Labels \"{label}\"}}}}"),
        ])
        .output()
        .map_err(|e| format!("failed to inspect container: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(if value.is_empty() || value == "<no value>" {
        None
    } else {
        Some(value)
    })
}

/// Validates *proposed* contents by parsing them where they would actually live.
///
/// The check has to run from the real file's own directory, and under the real
/// file's name, or it reports failures the saved file wouldn't have: relative
/// `build:` / `env_file:` / bind-mount paths resolve against the compose file's
/// directory, and a `.env` beside it is picked up automatically. So the candidate
/// goes into a sibling temp file rather than `/tmp`.
///
/// Validating before writing (not after) is the point: a rejected edit leaves the
/// user's working compose file exactly as it was.
fn validate_contents(compose_file: &Path, contents: &str) -> Result<(), String> {
    let dir = compose_file
        .parent()
        .ok_or_else(|| format!("could not determine the directory of {}", compose_file.display()))?;
    // Dot-prefixed and pid-suffixed: compose globs no such name, and two windows
    // saving at once don't collide.
    let probe = dir.join(format!(".devspro-compose-check-{}.yml", std::process::id()));
    std::fs::write(&probe, contents)
        .map_err(|e| format!("could not write a temporary file next to the compose file: {e}"))?;

    let probe_arg = probe.to_string_lossy().to_string();
    let result = super::compose::build(&["-f", &probe_arg, "config", "--quiet"])
        .and_then(|mut cmd| {
            cmd.current_dir(dir)
                .output()
                .map_err(|e| format!("failed to run docker compose: {e}"))
        });
    // Before the `?`, always — a probe left behind would show up in the user's
    // repo, and on the failure path is exactly when it would be forgotten.
    let _ = std::fs::remove_file(&probe);

    let output = result?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Compose names the probe file in its errors; the user never saw that name.
    Err(stderr.trim().replace(&probe_arg, &compose_file.to_string_lossy()))
}

/// Writes edited compose contents and applies them.
///
/// `service: Some(..)` recreates just that one service; `None` brings the whole
/// stack up. Either way it is `up -d`, which re-reads the file and recreates only
/// what changed — `restart` would bounce the containers with their old definition
/// still in force.
#[tauri::command]
pub fn docker_write_compose(
    path: String,
    contents: String,
    project: Option<String>,
    service: Option<String>,
) -> Result<String, String> {
    let file = Path::new(&path);
    if !file.is_file() {
        return Err(format!("{path} is not a file"));
    }
    validate_contents(file, &contents)?;
    std::fs::write(file, &contents).map_err(|e| format!("could not write {path}: {e}"))?;

    let mut args = vec!["up", "-d"];
    match &service {
        // No `--remove-orphans` here: scoped to one service, compose would treat
        // every *other* service in the stack as an orphan and remove it.
        Some(service) => args.push(service),
        None => args.push("--remove-orphans"),
    }
    let report = run_compose(&path, project.as_deref(), &args).map_err(|e| {
        format!("saved {path}, but applying it failed: {e}\n\nThe file is valid; the containers are unchanged.")
    })?;

    Ok(match &service {
        Some(service) => format!("Saved {path} and recreated {service}.\n{report}"),
        None => format!("Saved {path} and applied it to the stack.\n{report}"),
    })
}

/// Validates a compose file without changing anything — `config` parses it,
/// resolves variables, and fails on anything malformed. Used to catch a bad file
/// before the user is left reading a half-finished `up`.
#[tauri::command]
pub fn docker_compose_validate(path: String) -> Result<String, String> {
    let file = std::path::Path::new(&path);
    if !file.is_file() {
        return Err(format!("{path} is not a file"));
    }
    let dir = file.parent().unwrap_or(std::path::Path::new("."));
    let output = super::compose::build(&["-f", &path, "config", "--quiet"])?
        .current_dir(dir)
        .output()
        .map_err(|e| format!("failed to run docker compose: {e}"))?;
    if output.status.success() {
        Ok("compose file is valid".to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPOSE_LABELS: &str = "com.docker.compose.config-hash=abc123,\
com.docker.compose.project=databases,com.docker.compose.service=redis,\
com.docker.compose.version=1.29.2";

    #[test]
    fn label_reads_compose_project_and_service() {
        assert_eq!(
            label(COMPOSE_LABELS, "com.docker.compose.project"),
            Some("databases".to_string())
        );
        assert_eq!(
            label(COMPOSE_LABELS, "com.docker.compose.service"),
            Some("redis".to_string())
        );
    }

    #[test]
    fn label_does_not_prefix_match() {
        // "com.docker.compose.project" must not pick up the value of
        // "com.docker.compose.project.config_files", which is a different label
        // that happens to start with the same text.
        let labels = "com.docker.compose.project.config_files=/a.yml,com.docker.compose.project=stack";
        assert_eq!(
            label(labels, "com.docker.compose.project"),
            Some("stack".to_string())
        );
    }

    #[test]
    fn label_returns_none_for_plain_containers() {
        assert_eq!(label("", "com.docker.compose.project"), None);
        assert_eq!(label("maintainer=someone", "com.docker.compose.project"), None);
        // An empty value is as good as absent — it must not group as "".
        assert_eq!(label("com.docker.compose.project=", "com.docker.compose.project"), None);
    }

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
