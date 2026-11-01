//! The Docker way of running Mailpit: one fixed container in DevsPro's *own*
//! docker-compose space, mirroring the Databases module's "we own this compose
//! file" convention. The host ports live right in the compose file's `ports:`
//! list, same place the Databases module keeps its own state (see
//! `databases::service_block_text`). No `elevate`: Docker's own group
//! permissions already decide whether `docker` needs sudo.
//!
//! `restart: "no"` on purpose. `unless-stopped` brought the container back every
//! time Docker Desktop started — at boot, with DevsPro closed — so it held the
//! SMTP port on a machine where nobody had asked for it that day. The container
//! runs when the user starts it here, and not otherwise.

use std::fs;
use std::path::PathBuf;

pub(crate) const PROJECT_NAME: &str = "devspro_mailcatcher";
const SERVICE_NAME: &str = "mailcatcher";
const CONTAINER_NAME: &str = "devspro_mailcatcher";
const IMAGE: &str = "axllent/mailpit:latest";

fn compose_file(dir: &std::path::Path) -> Result<PathBuf, String> {
    let file = dir.join("docker-compose.yml");
    if !file.exists() {
        fs::write(&file, "services:\n").map_err(|e| e.to_string())?;
    }
    Ok(file)
}

fn indent_of(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

/// The line range of the `mailcatcher:` service block, if present. Only one
/// service name is ever expected here, unlike the multi-service scan in the
/// Databases module.
fn service_range(lines: &[&str]) -> Option<(usize, usize)> {
    let start = lines.iter().position(|l| l.trim_start().trim_end() == format!("{SERVICE_NAME}:"))?;
    let base_indent = indent_of(lines[start]);
    let end = lines[(start + 1)..]
        .iter()
        .position(|l| !l.trim().is_empty() && indent_of(l) <= base_indent)
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());
    Some((start, end))
}

fn service_block(contents: &str) -> Option<String> {
    let lines: Vec<&str> = contents.lines().collect();
    let (start, end) = service_range(&lines)?;
    Some(lines[start..end].join("\n"))
}

fn remove_service_block(contents: &str) -> String {
    let lines: Vec<&str> = contents.lines().collect();
    let Some((start, end)) = service_range(&lines) else {
        return contents.to_string();
    };
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
        "  {SERVICE_NAME}:\n    image: {IMAGE}\n    container_name: {CONTAINER_NAME}\n    restart: \"no\"\n    ports:\n      - \"{smtp_port}:1025\"\n      - \"{http_port}:8025\"\n"
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

fn ports_from_block(block: &str) -> Option<(u16, u16)> {
    Some((port_mapping_for(block, 1025)?, port_mapping_for(block, 8025)?))
}

/// The host ports our container is configured with, or `None` when it has
/// never been created (or was removed). Reads a file — no Docker call.
pub fn configured_ports(dir: &std::path::Path) -> Option<(u16, u16)> {
    let contents = fs::read_to_string(dir.join("docker-compose.yml")).ok()?;
    ports_from_block(&service_block(&contents)?)
}

/// Whether our container is the one running right now. This is the only place
/// the module spawns `docker` to *look*, and `resolve` only gets here once
/// something is already listening on the configured port.
pub fn container_running() -> bool {
    crate::modules::docker::list_containers()
        .map(|cs| cs.iter().any(|c| c.name == CONTAINER_NAME && c.state == "running"))
        .unwrap_or(false)
}

fn compose(dir: &std::path::Path, args: &[&str]) -> Result<(), String> {
    let file = compose_file(dir)?;
    let file_str = file.to_string_lossy().to_string();
    let mut full = vec!["-p", PROJECT_NAME, "-f", &file_str];
    full.extend_from_slice(args);
    let output = crate::modules::compose::build(&full)?
        .output()
        .map_err(|e| format!("failed to run docker compose: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

/// (Re)writes the service block with these ports and brings it up. Rewriting
/// rather than keeping the first ports lets the user change them while it's
/// off; `up -d` recreates the container only when the block actually changed.
pub fn start(dir: &std::path::Path, smtp_port: u16, http_port: u16) -> Result<(), String> {
    let detection = crate::modules::docker::detect();
    if !detection.found {
        return Err("Docker isn't installed. Use the built-in Mailpit instead, or install Docker and try again.".into());
    }
    if !detection.daemon_reachable {
        return Err(format!(
            "Docker isn't running ({}). Start Docker, or use the built-in Mailpit instead.",
            detection.error.unwrap_or_else(|| "the daemon did not answer".into())
        ));
    }

    let file = compose_file(dir)?;
    let existing = fs::read_to_string(&file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
    let mut updated = remove_service_block(&existing);
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&render_service_block(smtp_port, http_port));
    fs::write(&file, updated).map_err(|e| format!("could not write {}: {e}", file.display()))?;

    compose(dir, &["up", "-d", SERVICE_NAME])
        .map_err(|e| format!("wrote the compose file but starting the container failed: {e}"))
}

pub fn stop(dir: &std::path::Path) -> Result<(), String> {
    compose(dir, &["stop", SERVICE_NAME])
}

/// Deletes the container and forgets its ports. Any mail it caught goes with it.
pub fn remove(dir: &std::path::Path) -> Result<(), String> {
    let file = compose_file(dir)?;
    let existing = fs::read_to_string(&file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
    if service_block(&existing).is_none() {
        return Ok(());
    }
    compose(dir, &["rm", "-sf", SERVICE_NAME])?;
    fs::write(&file, remove_service_block(&existing)).map_err(|e| format!("could not write {}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_and_parse_roundtrip() {
        let block = render_service_block(1025, 8025);
        assert!(service_block(&format!("services:\n{block}")).is_some());
        assert_eq!(ports_from_block(&block), Some((1025, 8025)));
    }

    #[test]
    fn render_with_custom_ports() {
        let block = render_service_block(11025, 18025);
        assert_eq!(ports_from_block(&block), Some((11025, 18025)));
        assert!(block.contains("image: axllent/mailpit:latest"));
        assert!(block.contains("container_name: devspro_mailcatcher"));
    }

    #[test]
    fn container_never_restarts_on_its_own() {
        assert!(render_service_block(1025, 8025).contains("restart: \"no\""));
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
        assert_eq!(ports_from_block(block), Some((1025, 8025)));
    }

    #[test]
    fn port_mapping_ignores_non_port_colon_lines() {
        let block = "  mailcatcher:\n    image: axllent/mailpit:latest\n    ports:\n      - \"1025:1025\"\n";
        assert_eq!(port_mapping_for(block, 8025), None);
    }
}
