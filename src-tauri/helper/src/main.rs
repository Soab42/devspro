// Deliberately has zero dependency on the main app crate — this binary is the one thing
// that runs elevated, so it stays small and independently auditable. Its manifest schema
// is a plain copy of `elevate::Operation` in the main crate, kept in sync by hand.
mod platform;

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
    EnsureIncludeInHttpBlock {
        config_path: String,
        marker_begin: String,
        marker_end: String,
        include_line: String,
    },
    RunNginxTest {
        binary_path: String,
        prefix: Option<String>,
    },
    ReloadNginx {
        binary_path: String,
        prefix: Option<String>,
    },
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
    ClearWindowsTemp,
    ClearWindowsUpdateCache,
    ClearDeliveryOptimizationCache,
    RemoveSystemCrashDumps,
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

/// Writes via a scratch file in the same directory and renames over the target, so a
/// reader never sees a half-written `/etc/hosts` or nginx.conf.
///
/// The scratch name is unique per *process*, not per call — which is sound only because a
/// manifest's operations are applied strictly one at a time (see `main`), and each helper
/// invocation is its own process. Two concurrent calls within one process would stage
/// through the same path and rename over each other; the tests below have to give every
/// case its own directory for exactly that reason.
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

#[cfg(not(windows))]
fn hosts_path() -> PathBuf {
    PathBuf::from("/etc/hosts")
}

/// `<system dir>\drivers\etc\hosts`. The system directory comes from
/// `GetSystemDirectoryW` rather than `%SystemRoot%`: this process runs elevated,
/// so where it writes must not depend on an environment the caller could shape.
#[cfg(windows)]
fn hosts_path() -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }

    let mut buf = [0u16; 260];
    // SAFETY: `buf` is a writable buffer of exactly the length passed in.
    let len = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let system_dir = if len > 0 && len < buf.len() {
        PathBuf::from(OsString::from_wide(&buf[..len]))
    } else {
        PathBuf::from(r"C:\Windows\System32")
    };
    system_dir.join(r"drivers\etc\hosts")
}

/// Whether any uncommented line maps something to `localhost`.
fn defines_localhost(contents: &str) -> bool {
    contents
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .any(|l| l.split_whitespace().skip(1).any(|h| h.eq_ignore_ascii_case("localhost")))
}

/// Contents without a `localhost` line are refused: nothing legitimate the app
/// does removes it, and losing it breaks far more than the app.
#[cfg(not(windows))]
fn check_localhost(_path: &Path, contents: &str) -> Result<(), String> {
    if defines_localhost(contents) {
        Ok(())
    } else {
        Err("new hosts contents have no localhost entry — refusing to write".to_string())
    }
}

/// Windows resolves localhost itself and ships a hosts file with the localhost
/// lines commented out, so an entry cannot be required. What is refused instead
/// is dropping one the file currently has.
#[cfg(windows)]
fn check_localhost(path: &Path, contents: &str) -> Result<(), String> {
    let current = fs::read_to_string(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    if defines_localhost(&current) && !defines_localhost(contents) {
        return Err("new hosts contents drop the existing localhost entry — refusing to write".to_string());
    }
    Ok(())
}

/// Windows-only obstacles that would otherwise surface as a bare "Access is denied".
#[cfg(windows)]
fn check_writable(path: &Path) -> Result<(), String> {
    match fs::metadata(path) {
        Ok(meta) if meta.permissions().readonly() => Err(format!(
            "{} is marked read-only (some tools do this to lock it) — clear \"Read-only\" in its Properties and try again",
            path.display()
        )),
        _ => Ok(()),
    }
}

#[cfg(not(windows))]
fn check_writable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Replaces the hosts file wholesale. The path is fixed here rather than taken
/// from the manifest, and the write is atomic (temp file + rename, mode
/// preserved) so a crash mid-write cannot leave a truncated hosts file. See
/// `check_localhost` for the per-OS localhost rule.
fn write_hosts_file(contents: &str) -> OpResult {
    let path = hosts_path();
    let shown = path.display();
    if !path.is_file() {
        return OpResult {
            success: false,
            message: format!("{shown} does not exist — refusing to create it"),
        };
    }
    if let Err(message) = check_localhost(&path, contents).and_then(|()| check_writable(&path)) {
        return OpResult { success: false, message };
    }
    match atomic_write(&path, contents) {
        Ok(()) => OpResult {
            success: true,
            message: format!("wrote {shown}"),
        },
        // ERROR_VIRUS_INFECTED: Microsoft Defender blocks hosts edits it deems a
        // hijack (e.g. redirecting a well-known domain).
        #[cfg(windows)]
        Err(e) if e.raw_os_error() == Some(225) => OpResult {
            success: false,
            message: format!(
                "Windows Security blocked the write to {shown} (it flags some hosts entries as a hijack) — allow it under Protection history and try again"
            ),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to write {shown}: {e}"),
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

/// The line ending to write marked-block edits with. Windows files (the hosts
/// file above all) are usually CRLF; `lines()` strips the `\r`, so without this
/// a sync would quietly convert the whole file to LF.
#[cfg(windows)]
fn line_ending(original: &str) -> &'static str {
    if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

#[cfg(not(windows))]
fn line_ending(_original: &str) -> &'static str {
    "\n"
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

    let nl = line_ending(&original);
    let mut new_block = String::new();
    new_block.push_str(marker_begin);
    new_block.push_str(nl);
    for line in block_lines {
        new_block.push_str(line);
        new_block.push_str(nl);
    }
    new_block.push_str(marker_end);
    new_block.push_str(nl);

    let lines: Vec<&str> = original.lines().collect();
    let begin_idx = lines.iter().position(|l| l.trim() == marker_begin.trim());
    let end_idx = lines.iter().position(|l| l.trim() == marker_end.trim());

    let new_content = match (begin_idx, end_idx) {
        (Some(b), Some(e)) if e >= b => {
            let mut out = String::new();
            for l in &lines[..b] {
                out.push_str(l);
                out.push_str(nl);
            }
            out.push_str(&new_block);
            for l in &lines[e + 1..] {
                out.push_str(l);
                out.push_str(nl);
            }
            out
        }
        _ => {
            let mut out = original.clone();
            if !out.ends_with('\n') {
                out.push_str(nl);
            }
            out.push_str(nl);
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

    let nl = line_ending(&original);
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
        out.push_str(nl);
    }
    for l in lines.iter().skip(e + 1) {
        out.push_str(l);
        out.push_str(nl);
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

/// Splices one marker-wrapped `include` line in immediately after the line that opens
/// `http{}`, replacing the block in place if it's already there.
///
/// Where it goes is the whole reason this exists rather than reusing `sync_marked_block`:
/// that one appends to the end of the file when the markers are absent, which for
/// nginx.conf means *after* `http{}` closes — where an `include` of `server{}` blocks is a
/// syntax error. `sites` on Linux avoids the problem entirely by dropping a file into
/// `conf.d`, but the stock Windows nginx has no such directory and includes nothing.
///
/// Finding `http{}` by scanning for the opening line is enough for a real nginx.conf: the
/// directive is `http` followed by `{`, at most one per file, and never inside a comment
/// in any distribution's shipped config. A file with no `http` block at all is reported as
/// an error rather than guessed at, since inserting anywhere else would produce a config
/// that fails `nginx -t` with a message pointing at our own line.
fn ensure_include_in_http_block(
    config_path: &str,
    marker_begin: &str,
    marker_end: &str,
    include_line: &str,
) -> OpResult {
    let path = Path::new(config_path);
    let original = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return OpResult {
                success: false,
                message: format!("could not read {config_path}: {e}"),
            }
        }
    };

    let block = format!("{marker_begin}\n{include_line}\n{marker_end}\n");
    let lines: Vec<&str> = original.lines().collect();
    let begin_idx = lines.iter().position(|l| l.trim() == marker_begin.trim());
    let end_idx = lines.iter().position(|l| l.trim() == marker_end.trim());

    let (before, after) = match (begin_idx, end_idx) {
        // Already present — replace exactly the marked span, leaving it where it is.
        (Some(b), Some(e)) if e >= b => (&lines[..b], &lines[e + 1..]),
        _ => match lines.iter().position(is_http_block_opening) {
            Some(http) => (&lines[..=http], &lines[http + 1..]),
            None => {
                return OpResult {
                    success: false,
                    message: format!(
                        "{config_path} has no `http {{` block to add the include to — this nginx \
                         configuration doesn't use the standard layout, so automatic site setup \
                         isn't supported for it"
                    ),
                }
            }
        },
    };

    let mut out = String::new();
    for line in before {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&block);
    for line in after {
        out.push_str(line);
        out.push('\n');
    }

    match atomic_write(path, &out) {
        Ok(()) => OpResult {
            success: true,
            message: format!("ensured DevsPro include inside http{{}} in {config_path}"),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to write {config_path}: {e}"),
        },
    }
}

/// The line that opens nginx's `http` block: the bare directive, then `{` either on the
/// same line or (legally, if unusually) alone on the next one. A commented-out `http {`
/// must not count, or the include lands inside a comment and silently does nothing.
fn is_http_block_opening(line: &&str) -> bool {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return false;
    }
    let without_brace = trimmed.strip_suffix('{').unwrap_or(trimmed).trim_end();
    without_brace == "http"
}

/// `prefix` is nginx's `-p`, and is `None` on Unix — see `Operation::RunNginxTest` in the
/// main crate for why Windows cannot leave it out.
fn run_nginx_test(binary_path: &str, prefix: Option<&str>) -> OpResult {
    let mut command = Command::new(binary_path);
    if let Some(prefix) = prefix {
        command.args(["-p", prefix]);
    }
    match command.arg("-t").output() {
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
    #[cfg(windows)]
    {
        platform::service_control(action, unit)
    }
    #[cfg(not(windows))]
    {
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
}

// Windows has its own; see `platform::windows_memory`.
#[cfg(windows)]
use platform::{drop_caches, reclaim_swap};

/// `drop_caches` only frees clean pages, so `sync` first writes the dirty ones
/// out. A failed sync is reported but doesn't stop the drop.
#[cfg(not(windows))]
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
#[cfg_attr(windows, allow(dead_code))]
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
#[cfg_attr(windows, allow(dead_code))]
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
#[cfg_attr(windows, allow(dead_code))]
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

#[cfg(not(windows))]
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
        Operation::EnsureIncludeInHttpBlock {
            config_path,
            marker_begin,
            marker_end,
            include_line,
        } => ensure_include_in_http_block(&config_path, &marker_begin, &marker_end, &include_line),
        Operation::RunNginxTest { binary_path, prefix } => {
            run_nginx_test(&binary_path, prefix.as_deref())
        }
        Operation::ReloadNginx { binary_path, prefix } => {
            platform::reload_nginx(&binary_path, prefix.as_deref())
        }
        // The prefix check stays here, not in `platform`: it is the security boundary this
        // binary exists to enforce, and it is the same on every OS. Only what "install into
        // the trust store" means is platform-specific.
        Operation::InstallRootCa { cert_path, nss_db_paths } => {
            if !is_within_prefix(allowed_prefix, &cert_path) {
                OpResult {
                    success: false,
                    message: format!("{cert_path} is outside the allowed prefix — refusing to install"),
                }
            } else {
                platform::install_root_ca(&cert_path, &nss_db_paths)
            }
        }
        Operation::InstallPackage { manager, packages } => {
            if packages.is_empty() {
                OpResult {
                    success: false,
                    message: "no packages specified".to_string(),
                }
            } else {
                platform::install_package(&manager, &packages)
            }
        }
        Operation::FixLaravelPermissions { project_path } => {
            platform::fix_laravel_permissions(&project_path)
        }
        Operation::ServiceControl { action, unit } => service_control(&action, &unit),
        Operation::DropCaches => drop_caches(),
        Operation::ReclaimSwap => reclaim_swap(),
        Operation::AptClean => apt_clean(),
        Operation::VacuumJournal { max_bytes } => vacuum_journal(max_bytes),
        Operation::RemoveSnapRevision { name, revision } => remove_snap_revision(&name, &revision),
        // No arguments: every path these touch is fixed inside `platform`.
        Operation::ClearWindowsTemp => platform::clear_windows_temp(),
        Operation::ClearWindowsUpdateCache => platform::clear_windows_update_cache(),
        Operation::ClearDeliveryOptimizationCache => platform::clear_delivery_optimization_cache(),
        Operation::RemoveSystemCrashDumps => platform::remove_system_crash_dumps(),
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

/// Everything covered here is the part of this binary that edits files while running as
/// root. It had no tests at all before the Windows port needed a third block-editing
/// function alongside the two that already existed — which is exactly the wrong number of
/// similar-looking functions to leave unguarded in the one component that can overwrite
/// `/etc/hosts` and the user's real nginx.conf.
#[cfg(test)]
mod tests {
    use super::*;

    /// A seeded file in a directory of its own.
    ///
    /// The directory matters: `atomic_write` names its scratch file after the process id
    /// alone, so every test in this binary would otherwise stage through the same
    /// `.devspro-tmp-<pid>` path and rename it over each other's target — tests failing
    /// with another test's contents. That is safe in the helper itself, which applies a
    /// manifest's operations strictly one at a time in a process of its own, but it is not
    /// safe for a parallel test runner.
    struct TempDirFile {
        dir: PathBuf,
        file: PathBuf,
    }

    impl TempDirFile {
        fn with(contents: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("ldm-helper-test-{}-{seq}", std::process::id()));
            fs::create_dir_all(&dir).expect("failed to create test dir");
            let file = dir.join("target.conf");
            fs::write(&file, contents).expect("failed to seed test file");
            TempDirFile { dir, file }
        }

        fn path(&self) -> &str {
            self.file.to_str().expect("temp path is not valid UTF-8")
        }

        fn read(&self) -> String {
            fs::read_to_string(&self.file).expect("failed to read test file")
        }
    }

    impl Drop for TempDirFile {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    const BEGIN: &str = "# >>> devspro >>>";
    const END: &str = "# <<< devspro <<<";

    // --- the allowed-prefix guard -------------------------------------------------

    #[test]
    fn prefix_guard_admits_only_paths_under_the_declared_prefix() {
        assert!(is_within_prefix("/home/me/.local/share/DevsPro", "/home/me/.local/share/DevsPro/nginx/sites/a.conf"));
        assert!(!is_within_prefix("/home/me/.local/share/DevsPro", "/etc/passwd"));
        // A neighbouring directory that merely shares a name prefix is still outside.
        assert!(!is_within_prefix("/home/me/DevsPro", "/home/me/DevsPro-evil/x.conf"));
    }

    #[test]
    fn prefix_guard_rejects_traversal_and_an_empty_prefix() {
        // Traversal is refused on the raw string, before any canonicalisation could be
        // fooled by a symlink under the prefix.
        assert!(!is_within_prefix("/home/me/DevsPro", "/home/me/DevsPro/../../../etc/passwd"));
        // An empty prefix would otherwise match every path — `Ping` and
        // `FixLaravelPermissions` legitimately send one, so this must never be
        // interpreted as "anywhere".
        assert!(!is_within_prefix("", "/anything"));
    }

    // --- SyncMarkedBlock ---------------------------------------------------------

    #[test]
    fn sync_replaces_an_existing_block_in_place_and_leaves_the_rest_alone() {
        let file = TempDirFile::with(&format!(
            "127.0.0.1 localhost\n{BEGIN}\n127.0.0.1 old.test\n{END}\n# trailing comment\n"
        ));
        let result = sync_marked_block(
            file.path(),
            BEGIN,
            END,
            &["127.0.0.1 new.test".to_string()],
        );
        assert!(result.success, "{}", result.message);
        assert_eq!(
            file.read(),
            format!("127.0.0.1 localhost\n{BEGIN}\n127.0.0.1 new.test\n{END}\n# trailing comment\n")
        );
    }

    #[test]
    fn sync_appends_the_block_when_the_markers_are_absent() {
        let file = TempDirFile::with("127.0.0.1 localhost\n");
        let result = sync_marked_block(file.path(), BEGIN, END, &["127.0.0.1 a.test".to_string()]);
        assert!(result.success, "{}", result.message);
        assert_eq!(
            file.read(),
            format!("127.0.0.1 localhost\n\n{BEGIN}\n127.0.0.1 a.test\n{END}\n")
        );
    }

    /// An empty set of hostnames is the normal state after removing the last site, and it
    /// has to leave an empty block behind rather than deleting the markers — the next add
    /// finds and replaces them instead of appending a second copy.
    #[test]
    fn sync_with_no_lines_keeps_an_empty_marked_block() {
        let file = TempDirFile::with(&format!("127.0.0.1 localhost\n{BEGIN}\n127.0.0.1 a.test\n{END}\n"));
        let result = sync_marked_block(file.path(), BEGIN, END, &[]);
        assert!(result.success, "{}", result.message);
        assert_eq!(file.read(), format!("127.0.0.1 localhost\n{BEGIN}\n{END}\n"));
    }

    // --- RemoveMarkedBlock -------------------------------------------------------

    #[test]
    fn remove_takes_the_markers_with_it() {
        let file = TempDirFile::with(&format!("before\n{BEGIN}\ninclude x;\n{END}\nafter\n"));
        let result = remove_marked_block(file.path(), BEGIN, END);
        assert!(result.success, "{}", result.message);
        assert_eq!(file.read(), "before\nafter\n");
    }

    /// Called unconditionally on every add and remove, so the no-block case is the common
    /// one and must be a successful no-op rather than an error that aborts the manifest.
    #[test]
    fn remove_is_a_successful_no_op_when_there_is_no_block() {
        let file = TempDirFile::with("http {\n}\n");
        let result = remove_marked_block(file.path(), BEGIN, END);
        assert!(result.success, "{}", result.message);
        assert_eq!(file.read(), "http {\n}\n");
    }

    // --- EnsureIncludeInHttpBlock ------------------------------------------------

    #[test]
    fn include_goes_immediately_inside_the_http_block() {
        let file = TempDirFile::with("events {\n}\n\nhttp {\n    include mime.types;\n}\n");
        let result = ensure_include_in_http_block(file.path(), BEGIN, END, "include sites/*.conf;");
        assert!(result.success, "{}", result.message);
        assert_eq!(
            file.read(),
            format!("events {{\n}}\n\nhttp {{\n{BEGIN}\ninclude sites/*.conf;\n{END}\n    include mime.types;\n}}\n")
        );
    }

    /// Re-applied on every add, enable and remove, so running it repeatedly has to be
    /// indistinguishable from running it once.
    #[test]
    fn include_is_idempotent_and_updates_in_place() {
        let file = TempDirFile::with("http {\n    include mime.types;\n}\n");
        for line in ["include first/*.conf;", "include first/*.conf;", "include second/*.conf;"] {
            let result = ensure_include_in_http_block(file.path(), BEGIN, END, line);
            assert!(result.success, "{}", result.message);
        }
        assert_eq!(
            file.read(),
            format!("http {{\n{BEGIN}\ninclude second/*.conf;\n{END}\n    include mime.types;\n}}\n")
        );
        assert_eq!(file.read().matches(BEGIN).count(), 1, "exactly one block, never stacked");
    }

    /// The failure this whole operation exists to avoid: appended at the end of the file
    /// the include lands after `http{}` closes, where nginx rejects a `server{}` block. So
    /// a config with no `http{}` must be refused, not appended to.
    #[test]
    fn include_refuses_a_config_with_no_http_block() {
        let file = TempDirFile::with("events {\n    worker_connections 1024;\n}\n");
        let result = ensure_include_in_http_block(file.path(), BEGIN, END, "include sites/*.conf;");
        assert!(!result.success);
        assert!(result.message.contains("no `http {` block"), "{}", result.message);
        assert_eq!(file.read(), "events {\n    worker_connections 1024;\n}\n", "left untouched");
    }

    #[test]
    fn http_block_opening_ignores_comments_and_lookalikes() {
        assert!(is_http_block_opening(&"http {"));
        assert!(is_http_block_opening(&"  http{"));
        // `{` on its own line is legal nginx, and the marker still belongs after `http`.
        assert!(is_http_block_opening(&"http"));
        // A commented-out block would swallow the include into the comment.
        assert!(!is_http_block_opening(&"# http {"));
        // Other blocks and directives that merely start with the same letters.
        assert!(!is_http_block_opening(&"https {"));
        assert!(!is_http_block_opening(&"http2 on;"));
        assert!(!is_http_block_opening(&"    server {"));
    }
}

#[cfg(test)]
mod ops_tests {
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
    fn localhost_only_counts_on_uncommented_lines() {
        assert!(defines_localhost("127.0.0.1\tlocalhost\n"));
        assert!(defines_localhost("::1 ip6-localhost LocalHost # v6\r\n"));
        // The stock Windows hosts file mentions localhost only in comments.
        let windows = "# localhost name resolution is handled within DNS itself.\r\n#\t127.0.0.1       localhost\r\n";
        assert!(!defines_localhost(windows));
        assert!(!defines_localhost("127.0.0.1 my-localhost\n"));
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

    #[test]
    fn windows_cleanup_operations_deserialize_and_carry_no_paths() {
        let raw = r#"{"allowed_write_prefix":"","operations":[
            {"type":"ClearWindowsTemp"},
            {"type":"ClearWindowsUpdateCache"},
            {"type":"ClearDeliveryOptimizationCache"},
            {"type":"RemoveSystemCrashDumps"}
        ]}"#;
        let manifest: Manifest = serde_json::from_str(raw).unwrap();
        assert!(matches!(manifest.operations[0], Operation::ClearWindowsTemp));
        assert!(matches!(manifest.operations[1], Operation::ClearWindowsUpdateCache));
        assert!(matches!(manifest.operations[2], Operation::ClearDeliveryOptimizationCache));
        assert!(matches!(manifest.operations[3], Operation::RemoveSystemCrashDumps));
    }
}
