//! Docker Compose comes in two forms that take identical arguments but are
//! invoked differently: the modern `docker compose` CLI plugin, and the older
//! standalone `docker-compose` binary (Python-based, v1). This machine turned out
//! to have only the latter (`docker-compose` 1.29.2 — matching the label already
//! on the user's own `~/docker/databases` stack), so any module that shells out to
//! compose needs to detect which one is actually available rather than assuming
//! the plugin.

use std::process::Command;

enum Flavor {
    Plugin,
    Standalone(String),
}

fn detect() -> Result<Flavor, String> {
    let plugin_ok = Command::new("docker")
        .args(["compose", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if plugin_ok {
        return Ok(Flavor::Plugin);
    }

    if let Some(path) = crate::modules::tools::find("docker-compose") {
        return Ok(Flavor::Standalone(path));
    }

    Err("neither `docker compose` (v2 plugin) nor `docker-compose` (standalone) was found".to_string())
}

/// Builds a ready-to-`.output()` `Command` for whichever compose flavor this
/// machine has, e.g. `build(&["-p", "devspro", "-f", path, "up", "-d", "redis"])`.
pub fn build(args: &[&str]) -> Result<Command, String> {
    match detect()? {
        Flavor::Plugin => {
            let mut cmd = Command::new("docker");
            cmd.arg("compose");
            cmd.args(args);
            Ok(cmd)
        }
        Flavor::Standalone(path) => {
            let mut cmd = Command::new(path);
            cmd.args(args);
            Ok(cmd)
        }
    }
}
