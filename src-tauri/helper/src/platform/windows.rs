//! Windows. Built on the tools that ship with the OS, deliberately adding no dependency
//! to this crate — it is the one binary here that runs elevated, and staying auditable at
//! a glance is worth more than the convenience of a Win32 crate.

use std::fs;
use std::path::{Component, Path, PathBuf, Prefix};
use std::process::Command;
use std::time::{Duration, SystemTime};

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
///
/// The pid file is not trusted just for existing. Laragon leaves it empty for a moment
/// when it switches from Apache to nginx, and a crash leaves a stale one; either way
/// `nginx -s reload` fails with `invalid PID number ""` and would fail the whole manifest.
/// So the pid is checked against the running processes, and when it doesn't name a live
/// nginx the master is looked up directly and written back before reloading.
pub fn reload_nginx(binary_path: &str, prefix: Option<&str>) -> OpResult {
    if let Some(pid_file) = pid_file(prefix) {
        let not_running = || OpResult {
            success: true,
            message: format!(
                "configuration is in place, but nginx is not running (no {}) — start nginx to serve it",
                pid_file.display()
            ),
        };
        if !pid_file.exists() {
            return not_running();
        }
        let processes = nginx_processes(binary_path);
        let recorded = fs::read_to_string(&pid_file)
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok());
        if !recorded.is_some_and(|pid| processes.iter().any(|p| p.pid == pid)) {
            let Some(master) = master_pid(&processes) else {
                return not_running();
            };
            if let Err(e) = fs::write(&pid_file, format!("{master}\r\n")) {
                return OpResult {
                    success: false,
                    message: format!(
                        "nginx is running (PID {master}) but {} does not say so, and correcting it failed: {e}",
                        pid_file.display()
                    ),
                };
            }
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

#[derive(Debug, PartialEq)]
struct NginxProcess {
    pid: u32,
    parent: u32,
}

/// Every running `nginx.exe` started from `binary_path` — another install's nginx is not
/// the one this config belongs to. Read through CIM, which is what reports a process's
/// parent and image path without adding a Win32 dependency to this crate.
fn nginx_processes(binary_path: &str) -> Vec<NginxProcess> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-CimInstance Win32_Process -Filter \"Name='nginx.exe'\" | ForEach-Object { \"$($_.ProcessId)|$($_.ParentProcessId)|$($_.ExecutablePath)\" }",
        ])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    parse_nginx_processes(&String::from_utf8_lossy(&output.stdout), binary_path)
}

fn parse_nginx_processes(text: &str, binary_path: &str) -> Vec<NginxProcess> {
    let wanted = binary_path.replace('/', "\\").to_ascii_lowercase();
    text.lines()
        .filter_map(|line| {
            let mut parts = line.trim().splitn(3, '|');
            let pid = parts.next()?.parse().ok()?;
            let parent = parts.next()?.parse().ok()?;
            let path = parts.next()?.replace('/', "\\").to_ascii_lowercase();
            (path == wanted).then_some(NginxProcess { pid, parent })
        })
        .collect()
}

/// The master is the one nginx whose parent is not itself an nginx — its workers' parent.
fn master_pid(processes: &[NginxProcess]) -> Option<u32> {
    processes
        .iter()
        .find(|p| !processes.iter().any(|other| other.pid == p.parent))
        .map(|p| p.pid)
}

/// Never sent on Windows. `httpd -k restart` there only reaches an Apache installed as a
/// service, and the usual dev installs (Laragon, XAMPP) run it as a plain process — which
/// the app restarts itself, unelevated, so the new process is not an elevated orphan that
/// Laragon's own Stop button can no longer end.
pub fn reload_apache(_binary_path: &str, _pid_file: Option<&str>) -> OpResult {
    OpResult {
        success: false,
        message: "Apache is restarted by DevsPro itself on Windows, not through the helper".to_string(),
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

// ---- Cleaner, System tab ------------------------------------------------------------
//
// Four fixed cleanups, none taking a path from the caller. The folders they empty are
// found from `SystemRoot` and `ProgramData`, and only accepted in their standard shape —
// a drive root and one folder, `X:\Windows` / `X:\ProgramData` — so an odd environment
// can never point this elevated process at anything else.
//
// Deleting runs as an administrator inside folders a standard user can write to
// (`C:\Windows\Temp` above all), so a planted link must never be followed: a top-level
// link or junction is removed as itself, and `fs::remove_dir_all` does not follow links
// inside a tree (std removes them relative to the open parent handle).

/// Temp entries this recently touched are left: something may still be using them.
const MIN_TEMP_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// A temp folder this deep is too big to date; it is left as if fresh.
const MAX_AGE_WALK: usize = 20_000;

/// `path` is exactly `<drive>:\<name>`.
fn is_drive_folder(path: &Path, name: &str) -> bool {
    let parts: Vec<Component> = path.components().collect();
    matches!(
        parts.as_slice(),
        [Component::Prefix(p), Component::RootDir, Component::Normal(n)]
            if matches!(p.kind(), Prefix::Disk(_)) && n.eq_ignore_ascii_case(name)
    )
}

fn standard_folder(var: &str, name: &str) -> Result<PathBuf, String> {
    let path = std::env::var_os(var)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{var} is not set — refusing"))?;
    if !is_drive_folder(&path, name) {
        return Err(format!("{var} is {}, not a standard {name} folder — refusing", path.display()));
    }
    Ok(path)
}

fn windows_dir() -> Result<PathBuf, String> {
    standard_folder("SystemRoot", "Windows")
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// The newest modification time anywhere under `path`, not following links. `None`
/// when it cannot be read in full, which counts as "in use".
fn newest_mtime(path: &Path) -> Option<SystemTime> {
    let mut newest = fs::symlink_metadata(path).ok()?.modified().ok()?;
    let mut stack = vec![path.to_path_buf()];
    let mut seen = 0usize;
    while let Some(dir) = stack.pop() {
        if !fs::symlink_metadata(&dir).ok()?.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&dir).ok()?.flatten() {
            seen += 1;
            if seen > MAX_AGE_WALK {
                return None;
            }
            let meta = fs::symlink_metadata(entry.path()).ok()?;
            newest = newest.max(meta.modified().ok()?);
            if meta.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Some(newest)
}

fn stale_entries(dir: &Path) -> Vec<PathBuf> {
    let Some(cutoff) = SystemTime::now().checked_sub(MIN_TEMP_AGE) else {
        return Vec::new();
    };
    entries(dir)
        .into_iter()
        .filter(|p| newest_mtime(p).is_some_and(|t| t < cutoff))
        .collect()
}

/// (removed, left). One already gone counts as neither.
fn remove_entries(paths: &[PathBuf]) -> (usize, usize) {
    let (mut removed, mut left) = (0, 0);
    for path in paths {
        let Ok(meta) = fs::symlink_metadata(path) else {
            continue;
        };
        let result = if meta.file_type().is_symlink() {
            fs::remove_file(path).or_else(|_| fs::remove_dir(path))
        } else if meta.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };
        if result.is_ok() {
            removed += 1;
        } else {
            left += 1;
        }
    }
    (removed, left)
}

/// Files in use are normal in these folders, so leaving some is still a success;
/// removing nothing at all when something was there is not.
fn removal_result(what: &str, (removed, left): (usize, usize)) -> OpResult {
    match (removed, left) {
        (0, 0) => OpResult { success: true, message: format!("{what}: nothing to remove") },
        (0, l) => OpResult { success: false, message: format!("{what}: removed nothing, {l} in use") },
        (r, 0) => OpResult { success: true, message: format!("{what}: removed {r}") },
        (r, l) => OpResult { success: true, message: format!("{what}: removed {r}, {l} in use were left") },
    }
}

pub fn clear_windows_temp() -> OpResult {
    match windows_dir() {
        Ok(dir) => removal_result("Windows temp folder", remove_entries(&stale_entries(&dir.join("Temp")))),
        Err(message) => OpResult { success: false, message },
    }
}

/// Whether `sc query` reports the service as RUNNING (state 4; the number, unlike the
/// word next to it, is the same in every language).
fn service_running(name: &str) -> bool {
    let Ok(out) = Command::new("sc").args(["query", name]).output() else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout).lines().any(|line| {
        line.trim_start().starts_with("STATE")
            && line.split(':').nth(1).and_then(|v| v.split_whitespace().next()) == Some("4")
    })
}

/// Windows Update holds files in its download folder while it runs, so it is stopped
/// for the delete and started again afterwards if it had been running.
pub fn clear_windows_update_cache() -> OpResult {
    let dir = match windows_dir() {
        Ok(dir) => dir.join("SoftwareDistribution").join("Download"),
        Err(message) => return OpResult { success: false, message },
    };
    let was_running = service_running("wuauserv");
    if was_running {
        let _ = Command::new("net").args(["stop", "wuauserv", "/y"]).output();
    }
    let mut result = removal_result("Windows Update downloads", remove_entries(&entries(&dir)));
    if was_running {
        match Command::new("net").args(["start", "wuauserv"]).output() {
            Ok(out) if out.status.success() => {}
            _ => result.message.push_str("; Windows Update did not start again — it starts by itself when next needed"),
        }
    }
    result
}

pub fn clear_delivery_optimization_cache() -> OpResult {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Delete-DeliveryOptimizationCache -Force",
        ])
        .output();
    match output {
        Ok(out) if out.status.success() => OpResult {
            success: true,
            message: "emptied the Delivery Optimization cache".to_string(),
        },
        Ok(out) => OpResult {
            success: false,
            message: format!(
                "Delete-DeliveryOptimizationCache failed: {}{}",
                String::from_utf8_lossy(&out.stdout).trim(),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        },
        Err(e) => OpResult {
            success: false,
            message: format!("failed to run powershell: {e}"),
        },
    }
}

pub fn remove_system_crash_dumps() -> OpResult {
    let windows = match windows_dir() {
        Ok(dir) => dir,
        Err(message) => return OpResult { success: false, message },
    };
    let mut targets = Vec::new();
    let memory_dump = windows.join("MEMORY.DMP");
    if fs::symlink_metadata(&memory_dump).is_ok_and(|m| m.is_file()) {
        targets.push(memory_dump);
    }
    targets.extend(entries(&windows.join("Minidump")));
    // ProgramData is optional: without it the dumps under Windows are still cleaned.
    if let Ok(data) = standard_folder("ProgramData", "ProgramData") {
        let wer = data.join("Microsoft").join("Windows").join("WER");
        targets.extend(entries(&wer.join("ReportArchive")));
        targets.extend(entries(&wer.join("ReportQueue")));
    }
    removal_result("crash dumps and reports", remove_entries(&targets))
}

#[cfg(test)]
mod nginx_pid_tests {
    use super::*;

    const BIN: &str = r"C:\laragon\bin\nginx\nginx-1.22.0\nginx.exe";

    #[test]
    fn keeps_only_this_installs_processes() {
        let text = "25316|6988|C:\\laragon\\bin\\nginx\\nginx-1.22.0\\nginx.exe\r\n\
                    28668|25316|C:\\laragon\\bin\\nginx\\nginx-1.22.0\\nginx.exe\r\n\
                    999|1|C:\\nginx\\nginx.exe\r\n";
        assert_eq!(
            parse_nginx_processes(text, BIN),
            vec![NginxProcess { pid: 25316, parent: 6988 }, NginxProcess { pid: 28668, parent: 25316 }]
        );
    }

    #[test]
    fn the_master_is_the_workers_parent() {
        // Exactly the pair Laragon's nginx runs as: master 25316, worker 28668.
        let processes = vec![NginxProcess { pid: 28668, parent: 25316 }, NginxProcess { pid: 25316, parent: 6988 }];
        assert_eq!(master_pid(&processes), Some(25316));
        assert_eq!(master_pid(&[]), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_standard_drive_folder_is_accepted() {
        assert!(is_drive_folder(Path::new(r"C:\Windows"), "Windows"));
        assert!(is_drive_folder(Path::new(r"d:\WINDOWS"), "Windows"));
        for bad in [
            r"C:\",
            r"C:\Windows\System32",
            r"C:\Users\x\Windows",
            r"Windows",
            r"\\server\share\Windows",
            r"\\?\C:\Windows",
            r"C:\Win",
        ] {
            assert!(!is_drive_folder(Path::new(bad), "Windows"), "{bad}");
        }
    }

    #[test]
    fn removal_results() {
        assert!(removal_result("x", (0, 0)).success);
        assert!(removal_result("x", (2, 1)).success);
        assert!(!removal_result("x", (0, 3)).success);
    }

    #[test]
    fn stale_entries_skip_anything_touched_recently_and_links_go_alone() {
        let d = std::env::temp_dir().join(format!("ldm-helper-stale-{}", std::process::id()));
        let target = std::env::temp_dir().join(format!("ldm-helper-stale-target-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&target);
        fs::create_dir_all(d.join("fresh")).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep"), "x").unwrap();
        fs::write(d.join("old"), "x").unwrap();
        let old = SystemTime::now() - Duration::from_secs(3 * 86_400);
        fs::File::options().write(true).open(d.join("old")).unwrap().set_modified(old).unwrap();
        assert_eq!(stale_entries(&d), vec![d.join("old")]);

        let link = d.join("link");
        let made = Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(&target).output().unwrap();
        assert!(made.status.success());
        assert_eq!(remove_entries(&[link.clone(), d.join("old")]), (2, 0));
        assert!(target.join("keep").exists(), "a junction's target must survive");
        fs::remove_dir_all(&d).unwrap();
        fs::remove_dir_all(&target).unwrap();
    }
}
