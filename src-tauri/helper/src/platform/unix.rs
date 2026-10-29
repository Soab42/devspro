//! Linux (and macOS, for the parts that apply). Moved out of `main.rs` unchanged, so
//! the platform that already worked keeps behaving — and reporting — exactly as it did.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::OpResult;

pub fn reload_nginx(binary_path: &str, _prefix: Option<&str>) -> OpResult {
    if let Ok(out) = Command::new("systemctl").args(["reload", "nginx"]).output() {
        if out.status.success() {
            return OpResult {
                success: true,
                message: "reloaded via `systemctl reload nginx`".to_string(),
            };
        }
    }
    match Command::new(binary_path).args(["-s", "reload"]).output() {
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

pub fn install_root_ca(cert_path: &str, nss_db_paths: &[String]) -> OpResult {
    let dest = "/usr/local/share/ca-certificates/devspro-root-ca.crt";
    if let Err(e) = fs::copy(cert_path, dest) {
        return OpResult {
            success: false,
            message: format!("failed to copy CA cert to {dest}: {e}"),
        };
    }

    let (os_store_ok, mut message) = match Command::new("update-ca-certificates").output() {
        Ok(out) if out.status.success() => (true, "installed into OS trust store".to_string()),
        Ok(out) => (
            false,
            format!(
                "update-ca-certificates failed: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        ),
        Err(e) => (false, format!("failed to run update-ca-certificates: {e}")),
    };

    for nss_db in nss_db_paths {
        match Command::new("certutil")
            .args(["-d", nss_db, "-A", "-t", "C,,", "-n", "DevsPro Local CA", "-i", cert_path])
            .output()
        {
            Ok(out) if out.status.success() => message.push_str(&format!("; imported into {nss_db}")),
            Ok(out) => message.push_str(&format!(
                "; NSS import into {nss_db} failed: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )),
            Err(_) => message.push_str(
                "; certutil not found, skipped NSS import (install libnss3-tools for Firefox/Chrome trust)",
            ),
        }
    }

    OpResult {
        success: os_store_ok,
        message,
    }
}

pub fn install_package(manager: &str, packages: &[String]) -> OpResult {
    let mut command = match manager {
        "apt" => {
            let mut c = Command::new("apt-get");
            c.env("DEBIAN_FRONTEND", "noninteractive");
            c.arg("install").arg("-y");
            c
        }
        "dnf" => {
            let mut c = Command::new("dnf");
            c.arg("install").arg("-y");
            c
        }
        "pacman" => {
            let mut c = Command::new("pacman");
            c.arg("-S").arg("--noconfirm");
            c
        }
        other => {
            return OpResult {
                success: false,
                message: format!("unsupported package manager: {other}"),
            }
        }
    };
    command.args(packages);

    match command.output() {
        Ok(out) => {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let message = if out.status.success() {
                format!("installed {} via {manager}", packages.join(", "))
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
            message: format!("failed to run {manager}: {e}"),
        },
    }
}

pub fn fix_laravel_permissions(project_path: &str) -> OpResult {
    let targets = [
        format!("{project_path}/storage"),
        format!("{project_path}/bootstrap/cache"),
    ];
    let mut messages = Vec::new();
    let mut ok = true;

    for dir in &targets {
        if !Path::new(dir).is_dir() {
            messages.push(format!("{dir} does not exist, skipped"));
            continue;
        }
        let chgrp = Command::new("chgrp").args(["-R", "www-data", dir]).output();
        let chmod = Command::new("chmod").args(["-R", "775", dir]).output();
        let step_ok = matches!(&chgrp, Ok(o) if o.status.success()) && matches!(&chmod, Ok(o) if o.status.success());
        if step_ok {
            messages.push(format!("{dir}: group=www-data, mode=775"));
        } else {
            ok = false;
            let chgrp_err = match chgrp {
                Ok(o) if !o.status.success() => String::from_utf8_lossy(&o.stderr).trim().to_string(),
                Err(e) => e.to_string(),
                _ => String::new(),
            };
            let chmod_err = match chmod {
                Ok(o) if !o.status.success() => String::from_utf8_lossy(&o.stderr).trim().to_string(),
                Err(e) => e.to_string(),
                _ => String::new(),
            };
            messages.push(format!("{dir}: failed ({chgrp_err}{chmod_err})"));
        }
    }

    OpResult {
        success: ok,
        message: messages.join("; "),
    }
}

/// The four Windows-only cleanups of the Cleaner's System tab. The app never
/// sends them from Unix; a manifest that does gets a plain refusal rather than
/// a success for work that wasn't done.
fn windows_only(what: &str) -> OpResult {
    OpResult {
        success: false,
        message: format!("{what} exists only on Windows"),
    }
}

pub fn clear_windows_temp() -> OpResult {
    windows_only("the Windows temp folder")
}

pub fn clear_windows_update_cache() -> OpResult {
    windows_only("the Windows Update download cache")
}

pub fn clear_delivery_optimization_cache() -> OpResult {
    windows_only("the Delivery Optimization cache")
}

pub fn remove_system_crash_dumps() -> OpResult {
    windows_only("the Windows crash dump store")
}
