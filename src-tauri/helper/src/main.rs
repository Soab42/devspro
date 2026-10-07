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
    WriteHostsFile { contents: String },
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
    ServiceControl { action: String, unit: String },
    DropCaches,
    ReclaimSwap,
    AptClean,
    VacuumJournal { max_bytes: u64 },
    RemoveSnapRevision { name: String, revision: String },
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
    let tmp_path = dir.join(format!(".devspro-tmp-{}", std::process::id()));
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

const HOSTS_FILE: &str = "/etc/hosts";

/// Replaces /etc/hosts wholesale. The path is fixed here rather than taken from
/// the manifest, and the write is atomic (temp file + rename, mode preserved) so
/// a crash mid-write cannot leave a truncated hosts file. Contents without a
/// `localhost` line are refused: nothing legitimate the app does removes it,
/// and losing it breaks far more than the app.
fn write_hosts_file(contents: &str) -> OpResult {
    let path = Path::new(HOSTS_FILE);
    if !path.is_file() {
        return OpResult {
            success: false,
            message: format!("{HOSTS_FILE} does not exist — refusing to create it"),
        };
    }
    let has_localhost = contents
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .any(|l| l.split_whitespace().skip(1).any(|h| h.eq_ignore_ascii_case("localhost")));
    if !has_localhost {
        return OpResult {
            success: false,
            message: "new hosts contents have no localhost entry — refusing to write".to_string(),
        };
    }
    match atomic_write(path, contents) {
        Ok(()) => OpResult {
            success: true,
            message: format!("wrote {HOSTS_FILE}"),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to write {HOSTS_FILE}: {e}"),
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
    let dest = dir.join("devspro.conf");
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

/// `^[A-Za-z0-9@._:-]{1,128}$` without a regex crate, plus no leading `-`: a
/// unit can never be read as a `systemctl` option, and `-.mount` (the root
/// filesystem) is out of reach.
fn is_valid_unit_name(unit: &str) -> bool {
    !unit.is_empty()
        && unit.len() <= 128
        && !unit.starts_with('-')
        && unit.chars().all(|c| c.is_ascii_alphanumeric() || "@._:-".contains(c))
}

fn service_control(action: &str, unit: &str) -> OpResult {
    if action != "start" && action != "stop" {
        return OpResult {
            success: false,
            message: format!("unsupported service action: {action}"),
        };
    }
    if !is_valid_unit_name(unit) {
        return OpResult {
            success: false,
            message: format!("{unit:?} is not a valid unit name — refusing"),
        };
    }
    match Command::new("systemctl").args([action, "--", unit]).output() {
        Ok(out) if out.status.success() => OpResult {
            success: true,
            message: format!("{} {unit}", if action == "start" { "started" } else { "stopped" }),
        },
        Ok(out) => OpResult {
            success: false,
            message: format!(
                "systemctl {action} {unit} failed: {}{}",
                String::from_utf8_lossy(&out.stdout).trim(),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to run systemctl: {e}"),
        },
    }
}

/// `drop_caches` only frees clean pages, so `sync` first writes the dirty ones
/// out. A failed sync is reported but doesn't stop the drop.
fn drop_caches() -> OpResult {
    let synced = matches!(Command::new("sync").status(), Ok(s) if s.success());
    match fs::write("/proc/sys/vm/drop_caches", "3") {
        Ok(()) => OpResult {
            success: true,
            message: if synced {
                "dropped page cache, dentries and inodes".to_string()
            } else {
                "dropped caches (sync failed, so dirty pages stayed)".to_string()
            },
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to write /proc/sys/vm/drop_caches: {e}"),
        },
    }
}

/// The `/proc/meminfo` figures the swap guard needs, in kB:
/// (swap in use, MemAvailable).
fn swap_figures(meminfo: &str) -> (u64, u64) {
    let field = |name: &str| {
        meminfo
            .lines()
            .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
            .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
            .unwrap_or(0)
    };
    (field("SwapTotal").saturating_sub(field("SwapFree")), field("MemAvailable"))
}

#[derive(Debug, PartialEq)]
enum SwapPlan {
    /// Nothing in swap.
    Nothing,
    /// Safe to run swapoff/swapon.
    Reclaim,
    /// Too much in swap for the free RAM; the message says so.
    Refuse(String),
}

/// `swapoff` has to fit everything in swap into RAM, and the kernel will OOM-kill
/// to make room rather than give up — so refuse when what's in swap is more
/// than 80 % of MemAvailable.
fn plan_swap_reclaim(swap_used_kb: u64, mem_available_kb: u64) -> SwapPlan {
    if swap_used_kb == 0 {
        SwapPlan::Nothing
    } else if swap_used_kb.saturating_mul(100) > mem_available_kb.saturating_mul(80) {
        SwapPlan::Refuse(format!(
            "{} MB is in swap but only {} MB of RAM is free — close some apps first; reclaiming now could run out of memory",
            swap_used_kb / 1024,
            mem_available_kb / 1024
        ))
    } else {
        SwapPlan::Reclaim
    }
}

fn reclaim_swap() -> OpResult {
    let meminfo = match fs::read_to_string("/proc/meminfo") {
        Ok(s) => s,
        Err(e) => {
            return OpResult {
                success: false,
                message: format!("could not read /proc/meminfo: {e}"),
            }
        }
    };
    let (swap_used_kb, mem_available_kb) = swap_figures(&meminfo);
    match plan_swap_reclaim(swap_used_kb, mem_available_kb) {
        SwapPlan::Nothing => OpResult {
            success: true,
            message: "no swap in use".to_string(),
        },
        SwapPlan::Refuse(message) => OpResult {
            success: false,
            message,
        },
        SwapPlan::Reclaim => {
            let off = Command::new("swapoff").arg("-a").output();
            // Always turn swap back on, even after a failed swapoff — a partial
            // one may have disabled some devices already.
            let on = Command::new("swapon").arg("-a").output();
            let failure = |name: &str, r: &io::Result<std::process::Output>| match r {
                Ok(o) if o.status.success() => None,
                Ok(o) => Some(format!("{name} -a failed: {}", String::from_utf8_lossy(&o.stderr).trim())),
                Err(e) => Some(format!("failed to run {name}: {e}")),
            };
            match (failure("swapoff", &off), failure("swapon", &on)) {
                (None, None) => OpResult {
                    success: true,
                    message: format!("moved {} MB from swap back into RAM", swap_used_kb / 1024),
                },
                (a, b) => OpResult {
                    success: false,
                    message: [a, b].into_iter().flatten().collect::<Vec<_>>().join("; "),
                },
            }
        }
    }
}

/// Smallest size `journalctl --vacuum-size` may be asked for: a typo or a bad
/// caller can't wipe the journal down to nothing.
const MIN_JOURNAL_BYTES: u64 = 50 * 1024 * 1024;

/// `^[a-z0-9-]+$`, at most 40 characters, no leading `-` so it can never be
/// read as a `snap` option.
fn is_valid_snap_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && !name.starts_with('-')
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Digits only, no leading zero, at most 9 of them.
fn is_valid_snap_revision(rev: &str) -> bool {
    !rev.is_empty()
        && rev.len() <= 9
        && !rev.starts_with('0')
        && rev.chars().all(|c| c.is_ascii_digit())
}

/// Whether `snap list --all` output marks `name` at `revision` as disabled
/// (the Notes column, the last one, holds a comma-separated list).
fn snap_revision_is_disabled(listing: &str, name: &str, revision: &str) -> bool {
    listing.lines().skip(1).any(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        cols.len() >= 6
            && cols[0] == name
            && cols[2] == revision
            && cols.last().is_some_and(|notes| notes.split(',').any(|n| n == "disabled"))
    })
}

fn failed(message: String) -> OpResult {
    OpResult { success: false, message }
}

fn run_fixed(program: &str, args: &[&str], done: String) -> OpResult {
    match Command::new(program).args(args).output() {
        Ok(out) if out.status.success() => OpResult { success: true, message: done },
        Ok(out) => failed(format!(
            "{program} {} failed: {}{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => failed(format!("failed to run {program}: {e}")),
    }
}

fn apt_clean() -> OpResult {
    run_fixed("apt-get", &["clean"], "cleaned the apt package cache".to_string())
}

fn vacuum_journal(max_bytes: u64) -> OpResult {
    if max_bytes < MIN_JOURNAL_BYTES {
        return failed(format!(
            "{max_bytes} bytes is below the {} MiB minimum for a journal vacuum — refusing",
            MIN_JOURNAL_BYTES / (1024 * 1024)
        ));
    }
    let arg = format!("--vacuum-size={max_bytes}");
    run_fixed(
        "journalctl",
        &[&arg],
        format!("vacuumed the journal down to {} MiB", max_bytes / (1024 * 1024)),
    )
}

fn remove_snap_revision(name: &str, revision: &str) -> OpResult {
    if !is_valid_snap_name(name) {
        return failed(format!("{name:?} is not a valid snap name — refusing"));
    }
    if !is_valid_snap_revision(revision) {
        return failed(format!("{revision:?} is not a valid snap revision — refusing"));
    }
    let listing = match Command::new("snap").args(["list", "--all"]).output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        Ok(out) => return failed(format!("snap list failed: {}", String::from_utf8_lossy(&out.stderr).trim())),
        Err(e) => return failed(format!("failed to run snap: {e}")),
    };
    if !snap_revision_is_disabled(&listing, name, revision) {
        return failed(format!("{name} revision {revision} is not a disabled revision — refusing"));
    }
    let rev_arg = format!("--revision={revision}");
    run_fixed(
        "snap",
        &["remove", name, &rev_arg],
        format!("removed {name} revision {revision}"),
    )
}

fn apply(allowed_prefix: &str, op: Operation) -> OpResult {
    match op {
        Operation::Ping => OpResult {
            success: true,
            message: "pong from ldm-helper".to_string(),
        },
        Operation::WriteFile { path, contents } => write_file_checked(allowed_prefix, &path, &contents),
        Operation::RemoveFile { path } => remove_file_checked(allowed_prefix, &path),
        Operation::WriteHostsFile { contents } => write_hosts_file(&contents),
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
        Operation::ServiceControl { action, unit } => service_control(&action, &unit),
        Operation::DropCaches => drop_caches(),
        Operation::ReclaimSwap => reclaim_swap(),
        Operation::AptClean => apt_clean(),
        Operation::VacuumJournal { max_bytes } => vacuum_journal(max_bytes),
        Operation::RemoveSnapRevision { name, revision } => remove_snap_revision(&name, &revision),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_names_are_checked_before_systemctl_runs() {
        for ok in ["nginx", "php8.3-fpm", "postgresql@16-main", "docker.service", "a:b", &"x".repeat(128)] {
            assert!(is_valid_unit_name(ok), "{ok}");
        }
        for bad in ["", "-.mount", "--now", "nginx; reboot", "a b", "$(id)", "a/b", &"x".repeat(129), "ünit"] {
            assert!(!is_valid_unit_name(bad), "{bad}");
        }
    }

    #[test]
    fn service_control_refuses_other_actions_and_bad_units_without_running_anything() {
        for action in ["restart", "mask", "disable", "", "stop --now"] {
            let r = service_control(action, "nginx");
            assert!(!r.success);
            assert!(r.message.contains("unsupported"), "{}", r.message);
        }
        let r = service_control("stop", "--force");
        assert!(!r.success);
        assert!(r.message.contains("not a valid unit name"), "{}", r.message);
    }

    #[test]
    fn swap_guard() {
        assert_eq!(plan_swap_reclaim(0, 0), SwapPlan::Nothing);
        assert_eq!(plan_swap_reclaim(0, 4_000_000), SwapPlan::Nothing);
        // Exactly 80 % of available is still allowed; a kB more is not.
        assert_eq!(plan_swap_reclaim(800_000, 1_000_000), SwapPlan::Reclaim);
        assert!(matches!(plan_swap_reclaim(800_001, 1_000_000), SwapPlan::Refuse(_)));
        // This machine as the spec saw it: 6.6 GB in swap, 5 GB available.
        match plan_swap_reclaim(6_600 * 1024, 5_000 * 1024) {
            SwapPlan::Refuse(m) => assert_eq!(
                m,
                "6600 MB is in swap but only 5000 MB of RAM is free — close some apps first; reclaiming now could run out of memory"
            ),
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(matches!(plan_swap_reclaim(1, 0), SwapPlan::Refuse(_)));
    }

    #[test]
    fn swap_figures_come_from_meminfo() {
        let meminfo = "MemTotal:       16066572 kB\nMemAvailable:    5620872 kB\nSwapCached:         7220 kB\nSwapTotal:      20260340 kB\nSwapFree:       12845860 kB\n";
        assert_eq!(swap_figures(meminfo), (7_414_480, 5_620_872));
        assert_eq!(swap_figures(""), (0, 0));
    }

    #[test]
    fn new_operations_deserialize_from_the_apps_manifest_shape() {
        let raw = r#"{"allowed_write_prefix":"","operations":[
            {"type":"ServiceControl","action":"stop","unit":"nginx"},
            {"type":"DropCaches"},
            {"type":"ReclaimSwap"}
        ]}"#;
        let manifest: Manifest = serde_json::from_str(raw).unwrap();
        assert_eq!(manifest.operations.len(), 3);
        assert!(matches!(&manifest.operations[0], Operation::ServiceControl { action, unit } if action == "stop" && unit == "nginx"));
        assert!(matches!(manifest.operations[1], Operation::DropCaches));
        assert!(matches!(manifest.operations[2], Operation::ReclaimSwap));
    }

    #[test]
    fn snap_names_and_revisions_are_checked() {
        for ok in ["core20", "gnome-42-2204", "firefox", "a"] {
            assert!(is_valid_snap_name(ok), "{ok}");
        }
        for bad in ["", "-x", "--revision=1", "Core", "a b", "a/b", "a_b", "x;id", "../x", &"a".repeat(41)] {
            assert!(!is_valid_snap_name(bad), "{bad}");
        }
        for ok in ["1", "2182", "123456789"] {
            assert!(is_valid_snap_revision(ok), "{ok}");
        }
        for bad in ["", "x1", "12a", "-1", "1 2", "0", "012", "1234567890", "x123", "1.5"] {
            assert!(!is_valid_snap_revision(bad), "{bad}");
        }
    }

    #[test]
    fn snap_removal_is_refused_before_anything_runs() {
        let r = remove_snap_revision("Bad Name", "12");
        assert!(!r.success && r.message.contains("not a valid snap name"), "{}", r.message);
        let r = remove_snap_revision("core20", "12x");
        assert!(!r.success && r.message.contains("not a valid snap revision"), "{}", r.message);
    }

    #[test]
    fn journal_vacuum_has_a_floor() {
        for n in [0, 1, MIN_JOURNAL_BYTES - 1] {
            let r = vacuum_journal(n);
            assert!(!r.success && r.message.contains("minimum"), "{n}: {}", r.message);
        }
    }

    #[test]
    fn only_disabled_snap_revisions_pass_the_recheck() {
        let listing = "Name    Version  Rev   Tracking       Publisher   Notes\n\
core20  20240111 2182  latest/stable  canonical** base,disabled\n\
core20  20240227 2264  latest/stable  canonical** base\n\
firefox 123.0    3836  latest/stable  mozilla**   -\n";
        assert!(snap_revision_is_disabled(listing, "core20", "2182"));
        assert!(!snap_revision_is_disabled(listing, "core20", "2264"));
        assert!(!snap_revision_is_disabled(listing, "firefox", "3836"));
        assert!(!snap_revision_is_disabled(listing, "firefox", "2182"));
        assert!(!snap_revision_is_disabled("", "core20", "2182"));
    }

    #[test]
    fn system_cleanup_operations_deserialize_from_the_apps_manifest_shape() {
        let raw = r#"{"allowed_write_prefix":"","operations":[
            {"type":"AptClean"},
            {"type":"VacuumJournal","max_bytes":209715200},
            {"type":"RemoveSnapRevision","name":"core20","revision":"2182"}
        ]}"#;
        let manifest: Manifest = serde_json::from_str(raw).unwrap();
        assert_eq!(manifest.operations.len(), 3);
        assert!(matches!(manifest.operations[0], Operation::AptClean));
        assert!(matches!(manifest.operations[1], Operation::VacuumJournal { max_bytes: 209_715_200 }));
        assert!(matches!(&manifest.operations[2], Operation::RemoveSnapRevision { name, revision } if name == "core20" && revision == "2182"));
    }
}
