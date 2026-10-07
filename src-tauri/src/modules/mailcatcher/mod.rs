//! Runs Mailpit (https://github.com/axllent/mailpit) — a single-binary SMTP +
//! web-UI mail catcher — as one fixed container in DevsPro's *own* docker-compose
//! space, mirroring the Databases module's "we own this compose file" convention
//! but without a catalog of engines: there's only ever one instance, so no name,
//! no credentials, nothing to persist beyond which two host ports were chosen —
//! and those live right in the compose file's `ports:` list, same place the
//! Databases module keeps its own state (see `databases::service_block_text`).
//! No SQLite table, no `elevate`: Docker's own group permissions already decide
//! whether `docker` needs sudo, exactly as in the plain Docker module.

use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

pub(crate) const PROJECT_NAME: &str = "devspro_mailcatcher";
const SERVICE_NAME: &str = "mailcatcher";
const CONTAINER_NAME: &str = "devspro_mailcatcher";
const IMAGE: &str = "axllent/mailpit:latest";

fn compose_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("mailcatcher");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn compose_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let file = compose_dir(app)?.join("docker-compose.yml");
    if !file.exists() {
        fs::write(&file, "services:\n").map_err(|e| e.to_string())?;
    }
    Ok(file)
}

fn indent_of(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

/// Extracts the `mailcatcher:` service block (if present) so ports can be read
/// back and `up -d`/`rm` can be skipped/run appropriately. Only one service name
/// is ever expected here, unlike the multi-service scan in the Databases module.
fn service_block(contents: &str) -> Option<String> {
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines.iter().position(|l| l.trim_start().trim_end() == format!("{SERVICE_NAME}:"))?;
    let base_indent = indent_of(lines[start]);
    let end = lines[(start + 1)..]
        .iter()
        .position(|l| !l.trim().is_empty() && indent_of(l) <= base_indent)
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());
    Some(lines[start..end].join("\n"))
}

fn remove_service_block(contents: &str) -> String {
    let lines: Vec<&str> = contents.lines().collect();
    let Some(start) = lines.iter().position(|l| l.trim_start().trim_end() == format!("{SERVICE_NAME}:")) else {
        return contents.to_string();
    };
    let base_indent = indent_of(lines[start]);
    let end = lines[(start + 1)..]
        .iter()
        .position(|l| !l.trim().is_empty() && indent_of(l) <= base_indent)
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());
    let mut out: Vec<&str> = Vec::new();
    out.extend(&lines[..start]);
    out.extend(&lines[end..]);
    let mut result = out.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }
    result
}

fn render_service_block(smtp_port: u16, http_port: u16) -> String {
    format!(
        "  {SERVICE_NAME}:\n    image: {IMAGE}\n    container_name: {CONTAINER_NAME}\n    restart: unless-stopped\n    ports:\n      - \"{smtp_port}:1025\"\n      - \"{http_port}:8025\"\n"
    )
}

/// Reads back the host port bound to a given container port from a rendered
/// block — the read-side counterpart to `render_service_block`. Scans every
/// `key: value` style line for a bare `N:N` pattern rather than assuming a fixed
/// line index, so it stays correct even if a user hand-edits the file.
fn port_mapping_for(block: &str, container_port: u16) -> Option<u16> {
    block.lines().find_map(|line| {
        let item = line.trim().trim_start_matches('-').trim();
        let unquoted = item.trim_matches('"').trim_matches('\'');
        let (host, cont) = unquoted.rsplit_once(':')?;
        if cont.parse::<u16>() == Ok(container_port) {
            host.parse::<u16>().ok()
        } else {
            None
        }
    })
}

fn ports_from_block(block: &str) -> (Option<u16>, Option<u16>) {
    (port_mapping_for(block, 1025), port_mapping_for(block, 8025))
}

fn valid_port(port: u16) -> bool {
    port != 0
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct MailcatcherStatus {
    /// True once a `mailcatcher:` service block exists in the compose file —
    /// i.e. it's been started at least once, regardless of current run state.
    pub configured: bool,
    pub running: bool,
    pub state: Option<String>,
    pub smtp_port: Option<u16>,
    pub http_port: Option<u16>,
    pub web_url: Option<String>,
}

fn status(app: &tauri::AppHandle) -> Result<MailcatcherStatus, String> {
    let file = compose_file(app)?;
    let contents = fs::read_to_string(&file).unwrap_or_default();
    let block = service_block(&contents);
    let configured = block.is_some();
    let (smtp_port, http_port) = block.as_deref().map(ports_from_block).unwrap_or((None, None));

    let detection = super::docker::detect();
    if !detection.found || !detection.daemon_reachable {
        return Ok(MailcatcherStatus {
            configured,
            running: false,
            state: None,
            smtp_port,
            http_port,
            web_url: None,
        });
    }

    let containers = super::docker::list_containers()?;
    let container = containers.into_iter().find(|c| c.name == CONTAINER_NAME);
    let running = container.as_ref().is_some_and(|c| c.state == "running");
    let state = container.map(|c| c.state);
    let web_url = if running { http_port.map(|p| format!("http://localhost:{p}")) } else { None };

    Ok(MailcatcherStatus {
        configured,
        running,
        state,
        smtp_port,
        http_port,
        web_url,
    })
}

#[tauri::command(async)]
pub fn mailcatcher_detect_docker() -> super::docker::DockerDetection {
    super::docker::detect()
}

#[tauri::command(async)]
pub fn mailcatcher_status(app: tauri::AppHandle) -> Result<MailcatcherStatus, String> {
    status(&app)
}

#[tauri::command(async)]
pub fn mailcatcher_start(app: tauri::AppHandle, smtp_port: u16, http_port: u16) -> Result<MailcatcherStatus, String> {
    if !valid_port(smtp_port) || !valid_port(http_port) {
        return Err("ports must be between 1 and 65535".to_string());
    }
    if smtp_port == http_port {
        return Err("the SMTP and web UI ports must be different".to_string());
    }

    let file = compose_file(&app)?;
    let existing = fs::read_to_string(&file).map_err(|e| format!("could not read {}: {e}", file.display()))?;

    // Ports are only ever set at creation, same as the Databases module — once
    // the service block exists, `start` just brings the already-configured
    // container back up rather than silently re-pointing its ports.
    if service_block(&existing).is_none() {
        let mut new_content = existing;
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(&render_service_block(smtp_port, http_port));
        fs::write(&file, new_content).map_err(|e| format!("could not write {}: {e}", file.display()))?;
    }

    let file_str = file.to_string_lossy().to_string();
    let output = super::compose::build(&["-p", PROJECT_NAME, "-f", &file_str, "up", "-d", SERVICE_NAME])?
        .output()
        .map_err(|e| format!("failed to run docker compose: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "wrote the compose file but starting the container failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    status(&app)
}

#[tauri::command(async)]
pub fn mailcatcher_stop(app: tauri::AppHandle) -> Result<MailcatcherStatus, String> {
    let file = compose_file(&app)?;
    let file_str = file.to_string_lossy().to_string();
    let output = super::compose::build(&["-p", PROJECT_NAME, "-f", &file_str, "stop", SERVICE_NAME])?
        .output()
        .map_err(|e| format!("failed to run docker compose: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    status(&app)
}

#[tauri::command(async)]
pub fn mailcatcher_remove(app: tauri::AppHandle) -> Result<MailcatcherStatus, String> {
    let file = compose_file(&app)?;
    let existing = fs::read_to_string(&file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
    if service_block(&existing).is_none() {
        return status(&app);
    }

    let file_str = file.to_string_lossy().to_string();
    let output = super::compose::build(&["-p", PROJECT_NAME, "-f", &file_str, "rm", "-sf", SERVICE_NAME])?
        .output()
        .map_err(|e| format!("failed to run docker compose: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let updated = remove_service_block(&existing);
    fs::write(&file, updated).map_err(|e| format!("could not write {}: {e}", file.display()))?;

    status(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_and_parse_roundtrip() {
        let block = render_service_block(1025, 8025);
        assert!(service_block(&format!("services:\n{block}")).is_some());
        assert_eq!(ports_from_block(&block), (Some(1025), Some(8025)));
    }

    #[test]
    fn render_with_custom_ports() {
        let block = render_service_block(11025, 18025);
        assert_eq!(ports_from_block(&block), (Some(11025), Some(18025)));
        assert!(block.contains("image: axllent/mailpit:latest"));
        assert!(block.contains("container_name: devspro_mailcatcher"));
    }

    #[test]
    fn service_block_is_none_when_absent() {
        assert!(service_block("services:\n").is_none());
    }

    #[test]
    fn service_block_stops_at_next_sibling_service() {
        let contents = format!(
            "services:\n{}  another_service:\n    image: foo\n",
            render_service_block(1025, 8025)
        );
        let block = service_block(&contents).expect("block found");
        assert!(!block.contains("another_service"));
        assert!(block.contains("axllent/mailpit"));
    }

    #[test]
    fn remove_service_block_deletes_only_our_service() {
        let contents = format!(
            "services:\n{}  another_service:\n    image: foo\n",
            render_service_block(1025, 8025)
        );
        let updated = remove_service_block(&contents);
        assert!(!updated.contains("mailcatcher:"));
        assert!(updated.contains("another_service"));
    }

    #[test]
    fn remove_service_block_is_noop_when_absent() {
        let contents = "services:\n";
        assert_eq!(remove_service_block(contents), contents);
    }

    #[test]
    fn port_mapping_survives_hand_edited_quoting_styles() {
        let block = "  mailcatcher:\n    image: axllent/mailpit:latest\n    ports:\n      - 1025:1025\n      - '8025:8025'\n";
        assert_eq!(ports_from_block(block), (Some(1025), Some(8025)));
    }

    #[test]
    fn port_mapping_ignores_non_port_colon_lines() {
        let block = "  mailcatcher:\n    image: axllent/mailpit:latest\n    ports:\n      - \"1025:1025\"\n";
        assert_eq!(port_mapping_for(block, 8025), None);
    }

    #[test]
    fn zero_port_is_invalid() {
        assert!(!valid_port(0));
        assert!(valid_port(1025));
        assert!(valid_port(65535));
    }
}
