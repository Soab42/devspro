// Deliberately has zero dependency on the main app crate — this binary is the one thing
// that runs elevated, so it stays small and independently auditable. Its manifest schema
// is a plain copy of `elevate::Operation` in the main crate, kept in sync by hand.
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Operation {
    Ping,
    WriteFile { path: String, contents: String },
    RemoveFile { path: String },
    SyncMarkedBlock {
        file_path: String,
        marker_begin: String,
        marker_end: String,
        block_lines: Vec<String>,
    },
    RemoveMarkedBlock {
        file_path: String,
        marker_begin: String,
        marker_end: String,
    },
    EnsureNginxConfDInclude { conf_d_dir: String, sites_glob: String },
    RunNginxTest { binary_path: String },
    ReloadNginx { binary_path: String },
    InstallRootCa {
        cert_path: String,
        nss_db_paths: Vec<String>,
    },
    InstallPackage { manager: String, packages: Vec<String> },
    FixLaravelPermissions { project_path: String },
}

#[derive(Debug, Deserialize)]
struct Manifest {
    allowed_write_prefix: String,
    operations: Vec<Operation>,
}

#[derive(Debug, Serialize)]
struct OpResult {
    success: bool,
    message: String,
}

fn is_within_prefix(prefix: &str, path: &str) -> bool {
    if prefix.is_empty() || path.contains("..") {
        return false;
    }
    Path::new(path).starts_with(Path::new(prefix))
}

fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp_path = dir.join(format!(".devsuite-tmp-{}", std::process::id()));
    fs::write(&tmp_path, contents)?;
    if let Ok(meta) = fs::metadata(path) {
        let _ = fs::set_permissions(&tmp_path, meta.permissions());
    }
    fs::rename(&tmp_path, path)
}

fn write_file_checked(allowed_prefix: &str, path: &str, contents: &str) -> OpResult {
    if !is_within_prefix(allowed_prefix, path) {
        return OpResult {
            success: false,
            message: format!("{path} is outside the allowed prefix ({allowed_prefix}) — refusing to write"),
        };
    }
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return OpResult {
                success: false,
                message: format!("failed to create parent dir for {path}: {e}"),
            };
        }
    }
    match fs::write(p, contents) {
        Ok(()) => OpResult {
            success: true,
            message: format!("wrote {path}"),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to write {path}: {e}"),
        },
    }
}

fn remove_file_checked(allowed_prefix: &str, path: &str) -> OpResult {
    if !is_within_prefix(allowed_prefix, path) {
        return OpResult {
            success: false,
            message: format!("{path} is outside the allowed prefix ({allowed_prefix}) — refusing to remove"),
        };
    }
    match fs::remove_file(path) {
        Ok(()) => OpResult {
            success: true,
            message: format!("removed {path}"),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => OpResult {
            success: true,
            message: format!("{path} already absent"),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to remove {path}: {e}"),
        },
    }
}

fn sync_marked_block(file_path: &str, marker_begin: &str, marker_end: &str, block_lines: &[String]) -> OpResult {
    let path = Path::new(file_path);
    let original = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return OpResult {
                success: false,
                message: format!("could not read {file_path}: {e}"),
            }
        }
    };

    let mut new_block = String::new();
    new_block.push_str(marker_begin);
    new_block.push('\n');
    for line in block_lines {
        new_block.push_str(line);
        new_block.push('\n');
    }
    new_block.push_str(marker_end);
    new_block.push('\n');

    let lines: Vec<&str> = original.lines().collect();
    let begin_idx = lines.iter().position(|l| l.trim() == marker_begin.trim());
    let end_idx = lines.iter().position(|l| l.trim() == marker_end.trim());

    let new_content = match (begin_idx, end_idx) {
        (Some(b), Some(e)) if e >= b => {
            let mut out = String::new();
            for l in &lines[..b] {
                out.push_str(l);
                out.push('\n');
            }
            out.push_str(&new_block);
            for l in &lines[e + 1..] {
                out.push_str(l);
                out.push('\n');
            }
            out
        }
        _ => {
            let mut out = original.clone();
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
            out.push_str(&new_block);
            out
        }
    };

    match atomic_write(path, &new_content) {
        Ok(()) => OpResult {
            success: true,
            message: format!("synced marked block in {file_path}"),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to write {file_path}: {e}"),
        },
    }
}

fn remove_marked_block(file_path: &str, marker_begin: &str, marker_end: &str) -> OpResult {
    let path = Path::new(file_path);
    let original = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return OpResult {
                success: false,
                message: format!("could not read {file_path}: {e}"),
            }
        }
    };

    let lines: Vec<&str> = original.lines().collect();
    let begin_idx = lines.iter().position(|l| l.trim() == marker_begin.trim());
    let end_idx = lines.iter().position(|l| l.trim() == marker_end.trim());

    let (b, e) = match (begin_idx, end_idx) {
        (Some(b), Some(e)) if e >= b => (b, e),
        _ => {
            return OpResult {
                success: true,
                message: format!("no marked block found in {file_path}, nothing to remove"),
            }
        }
    };

    let mut out = String::new();
    for l in lines.iter().take(b) {
        out.push_str(l);
        out.push('\n');
    }
    for l in lines.iter().skip(e + 1) {
        out.push_str(l);
        out.push('\n');
    }

    match atomic_write(path, &out) {
        Ok(()) => OpResult {
            success: true,
            message: format!("removed marked block from {file_path}"),
        },
        Err(err) => OpResult {
            success: false,
            message: format!("failed to write {file_path}: {err}"),
        },
    }
}

fn ensure_nginx_confd_include(conf_d_dir: &str, sites_glob: &str) -> OpResult {
    let dir = Path::new(conf_d_dir);
    if !dir.is_dir() {
        return OpResult {
            success: false,
            message: format!("{conf_d_dir} is not a directory"),
        };
    }
    let dest = dir.join("devsuite.conf");
    let contents = format!("include {sites_glob};\n");
    match fs::write(&dest, contents) {
        Ok(()) => OpResult {
            success: true,
            message: format!("wrote {}", dest.display()),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to write {}: {e}", dest.display()),
        },
    }
}

fn run_nginx_test(binary_path: &str) -> OpResult {
    match Command::new(binary_path).arg("-t").output() {
        Ok(out) => {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            OpResult {
                success: out.status.success(),
                message: combined.trim().to_string(),
            }
        }
        Err(e) => OpResult {
            success: false,
            message: format!("failed to run `{binary_path} -t`: {e}"),
        },
    }
}

fn reload_nginx(binary_path: &str) -> OpResult {
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

fn install_root_ca(allowed_prefix: &str, cert_path: &str, nss_db_paths: &[String]) -> OpResult {
    if !is_within_prefix(allowed_prefix, cert_path) {
        return OpResult {
            success: false,
            message: format!("{cert_path} is outside the allowed prefix — refusing to install"),
        };
    }

    let dest = "/usr/local/share/ca-certificates/devsuite-root-ca.crt";
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
            .args(["-d", nss_db, "-A", "-t", "C,,", "-n", "DevSuite Local CA", "-i", cert_path])
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

fn install_package(manager: &str, packages: &[String]) -> OpResult {
    if packages.is_empty() {
        return OpResult {
            success: false,
            message: "no packages specified".to_string(),
        };
    }
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

fn fix_laravel_permissions(project_path: &str) -> OpResult {
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

fn apply(allowed_prefix: &str, op: Operation) -> OpResult {
    match op {
        Operation::Ping => OpResult {
            success: true,
            message: "pong from ldm-helper".to_string(),
        },
        Operation::WriteFile { path, contents } => write_file_checked(allowed_prefix, &path, &contents),
        Operation::RemoveFile { path } => remove_file_checked(allowed_prefix, &path),
        Operation::SyncMarkedBlock {
            file_path,
            marker_begin,
            marker_end,
            block_lines,
        } => sync_marked_block(&file_path, &marker_begin, &marker_end, &block_lines),
        Operation::RemoveMarkedBlock {
            file_path,
            marker_begin,
            marker_end,
        } => remove_marked_block(&file_path, &marker_begin, &marker_end),
        Operation::EnsureNginxConfDInclude { conf_d_dir, sites_glob } => {
            ensure_nginx_confd_include(&conf_d_dir, &sites_glob)
        }
        Operation::RunNginxTest { binary_path } => run_nginx_test(&binary_path),
        Operation::ReloadNginx { binary_path } => reload_nginx(&binary_path),
        Operation::InstallRootCa { cert_path, nss_db_paths } => {
            install_root_ca(allowed_prefix, &cert_path, &nss_db_paths)
        }
        Operation::InstallPackage { manager, packages } => install_package(&manager, &packages),
        Operation::FixLaravelPermissions { project_path } => fix_laravel_permissions(&project_path),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(manifest_path) = args.get(1) else {
        eprintln!("usage: ldm-helper <manifest-path>");
        std::process::exit(2);
    };

    let manifest_path = PathBuf::from(manifest_path);
    let result_path = manifest_path.with_extension("result.json");

    let results: Vec<OpResult> = match fs::read_to_string(&manifest_path) {
        Ok(raw) => match serde_json::from_str::<Manifest>(&raw) {
            Ok(manifest) => {
                // Abort-on-first-failure within one manifest: a later op (e.g.
                // ReloadNginx) never runs if an earlier one (e.g. RunNginxTest)
                // failed, so a broken config can't get reloaded. Already-applied
                // steps are not automatically rolled back in this version.
                let prefix = manifest.allowed_write_prefix.clone();
                let mut results = Vec::new();
                let mut aborted = false;
                for op in manifest.operations {
                    if aborted {
                        results.push(OpResult {
                            success: false,
                            message: "skipped: an earlier operation in this manifest failed".to_string(),
                        });
                        continue;
                    }
                    let r = apply(&prefix, op);
                    if !r.success {
                        aborted = true;
                    }
                    results.push(r);
                }
                results
            }
            Err(e) => vec![OpResult {
                success: false,
                message: format!("invalid manifest: {e}"),
            }],
        },
        Err(e) => vec![OpResult {
            success: false,
            message: format!("could not read manifest: {e}"),
        }],
    };

    let out = serde_json::to_string_pretty(&results).unwrap_or_default();
    if let Err(e) = fs::write(&result_path, out) {
        eprintln!("failed to write result file: {e}");
        std::process::exit(1);
    }
    // Exit 0 regardless of per-op outcomes — the caller reads success/failure per
    // operation from the result file. A non-zero exit here is reserved for the
    // helper itself failing to run (unreadable manifest, unwritable result file),
    // which platform::launch_helper treats as "the elevated call itself failed."
}
