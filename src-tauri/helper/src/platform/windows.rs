//! Windows. Built on the tools that ship with the OS, deliberately adding no dependency
//! to this crate — it is the one binary here that runs elevated, and staying auditable at
//! a glance is worth more than the convenience of a Win32 crate.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::OpResult;

/// nginx writes this under its prefix while running, and removes it on exit — the same
/// file `nginx -s reload` itself looks for to find the process to signal.
fn pid_file(prefix: Option<&str>) -> Option<PathBuf> {
    Some(Path::new(prefix?).join("logs").join("nginx.pid"))
}

/// There is no service manager to go through here, so this is `nginx -s reload` and
/// nothing else — but only when nginx is actually running.
///
/// When it isn't, this reports **success** with an explanation rather than failing.
/// Failing would abort the rest of the manifest and, in `sites`, discard the site
/// entirely — even though its config file, its certificate and its hosts entry were all
/// written correctly and will be picked up the moment nginx starts. "Nothing to reload"
/// is not the same as "the change didn't work", and conflating them would throw away work
/// that succeeded.
///
/// Note what this deliberately does *not* do: start nginx. nginx on Windows runs in the
/// foreground rather than daemonising, so a one-shot elevated helper launching it would
/// leave an elevated orphan process behind with nothing supervising it. Telling the user
/// to start nginx is the honest option until DevsPro manages that process properly.
pub fn reload_nginx(binary_path: &str, prefix: Option<&str>) -> OpResult {
    if let Some(pid_file) = pid_file(prefix) {
        if !pid_file.exists() {
            return OpResult {
                success: true,
                message: format!(
                    "configuration is in place, but nginx is not running (no {}) — start nginx to serve it",
                    pid_file.display()
                ),
            };
        }
    }

    let mut command = Command::new(binary_path);
    if let Some(prefix) = prefix {
        command.args(["-p", prefix]);
    }
    match command.args(["-s", "reload"]).output() {
        Ok(out) => {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let message = if combined.trim().is_empty() {
                format!("reloaded via `{binary_path} -s reload`")
            } else {
                combined.trim().to_string()
            };
            OpResult {
                success: out.status.success(),
                message,
            }
        }
        Err(e) => OpResult {
            success: false,
            message: format!("failed to reload nginx: {e}"),
        },
    }
}

/// Installs the CA into the machine-wide `Root` store, which is what Chrome, Edge and
/// anything else using the OS trust store read.
///
/// Firefox is the documented exception: it ships its own NSS trust store and ignores the
/// Windows one unless `security.enterprise_roots.enabled` is turned on. The Linux backend
/// covers that case with NSS's `certutil`, but the `certutil` on PATH here is Microsoft's
/// entirely different tool of the same name — so rather than shell out to something that
/// would take the flags and do something else, this says plainly what is and isn't
/// trusted. `nss_db_paths` is always empty from the Windows caller.
pub fn install_root_ca(cert_path: &str, nss_db_paths: &[String]) -> OpResult {
    let output = Command::new("certutil")
        .args(["-addstore", "-f", "Root", cert_path])
        .output();

    let mut result = match output {
        Ok(out) if out.status.success() => OpResult {
            success: true,
            message: "installed into the Windows Root store (Chrome and Edge trust it now; \
                      Firefox uses its own store — enable security.enterprise_roots.enabled \
                      in about:config, or import the certificate there by hand)"
                .to_string(),
        },
        Ok(out) => OpResult {
            success: false,
            message: format!(
                "certutil -addstore failed: {}{}",
                String::from_utf8_lossy(&out.stdout).trim(),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to run certutil: {e}"),
        },
    };

    if !nss_db_paths.is_empty() {
        result.message.push_str(
            "; NSS databases were listed but no NSS certutil exists on Windows, so they were skipped",
        );
    }
    result
}

/// Installs each package with `winget`.
///
/// One call per package, unlike apt: `winget install` takes a single id, so a batch is a
/// loop rather than one command line. The property that matters to the user is preserved
/// either way — the whole batch still costs exactly one UAC prompt, because it all happens
/// inside this one elevated process.
///
/// Every result is reported, not just the first failure, so a batch where one id is wrong
/// still tells the user which one. The operation counts as successful only if every package
/// installed, matching how the apt path treats a partial failure.
pub fn install_package(manager: &str, packages: &[String]) -> OpResult {
    if manager != "winget" {
        return OpResult {
            success: false,
            message: format!("unsupported package manager on Windows: {manager}"),
        };
    }

    let mut messages = Vec::new();
    let mut ok = true;

    for package in packages {
        match install_one(package) {
            Ok(message) => messages.push(message),
            Err(message) => {
                ok = false;
                messages.push(message);
            }
        }
    }

    OpResult {
        success: ok,
        message: messages.join("; "),
    }
}

fn install_one(package: &str) -> Result<String, String> {
    let output = Command::new("winget")
        .args([
            "install",
            "--id",
            package,
            // Without --exact, winget resolves the id as a search term and can pick a
            // different package that merely matches.
            "--exact",
            // Restricted to the community repository: `msstore` needs a user context this
            // elevated process may not have, and would otherwise be consulted first.
            "--source",
            "winget",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            // No prompts at all: there is no console for the user to answer one on.
            "--disable-interactivity",
        ])
        .output()
        .map_err(|e| {
            format!(
                "{package}: could not run winget ({e}) — it is installed per-user as an app \
                 execution alias, which an elevated process does not always inherit"
            )
        })?;

    if output.status.success() {
        return Ok(format!("installed {package}"));
    }

    let detail = [&output.stdout, &output.stderr]
        .into_iter()
        .map(|stream| String::from_utf8_lossy(stream).trim().to_string())
        .find(|text| !text.is_empty())
        .unwrap_or_else(|| format!("exited with {}", output.status));
    Err(format!("{package}: {detail}"))
}

/// Nothing to do. The Unix version exists because PHP-FPM's pool runs as `www-data` while
/// the project folder belongs to the desktop user; on Windows `php-cgi.exe` is started by
/// DevsPro as that same user, who already owns the folder. Reported as a no-op rather
/// than silently claiming to have changed permissions.
pub fn fix_laravel_permissions(project_path: &str) -> OpResult {
    OpResult {
        success: true,
        message: format!(
            "nothing to change: on Windows php-cgi runs as you, and you already own {project_path}"
        ),
    }
}
