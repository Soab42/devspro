//! Windows. There is no PHP-FPM here — no pool manager, no Unix sockets, nothing under
//! `/run/php`. PHP ships `php-cgi.exe`, which speaks the same FastCGI protocol nginx
//! expects but over a TCP port, and *nothing starts it for you*: no service, no packaging
//! convention, no equivalent of `systemctl start php8.3-fpm`.
//!
//! That difference shapes the whole backend:
//!
//! - **"Available" means installed, not running.** The Unix backend only lists versions
//!   with a live socket, because there a dead pool is somebody else's problem to start.
//!   Applying that rule here would leave the dropdown permanently empty on a machine with
//!   four PHP versions sitting on disk. So discovery lists what is installed, and the
//!   listener is started at the point a site actually needs it.
//! - **The listener is started on demand and left running.** `target_for_version` brings
//!   one up if the port isn't already answering, which also means an existing listener —
//!   one started earlier in this session, by a previous run of DevsPro, or by Laragon —
//!   is adopted rather than duplicated. Nothing is killed on exit, matching how `workers`
//!   treats the processes it starts: the site keeps serving, and the next launch finds the
//!   port already answering.
//! - **Ports are derived from the version, not allocated.** A site's rendered vhost holds
//!   the port literally, so it has to be the same port next week; `9000 + major*100 +
//!   minor` gives 8.3 the same 9803 every time, on every machine.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::os::windows::process::CommandExt;
use std::time::{Duration, Instant};

use super::super::PhpFpmVersion;
use crate::modules::nginx_ssl::ports;

/// Without this, starting a PHP listener flashes up a console window that then sits in the
/// taskbar for as long as the listener runs.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Directories holding one versioned subdirectory per install — Laragon's
/// `php-8.3.31-Win32-vs16-x64`, Chocolatey's `php-8.3.31`.
const VERSIONED_ROOTS: &[&str] = &[r"C:\laragon\bin\php", r"C:\tools"];

/// Directories holding `php-cgi.exe` directly.
const DIRECT_ROOTS: &[&str] = &[r"C:\php", r"C:\Program Files\PHP", r"C:\xampp\php"];

/// How long to wait for a freshly started listener to begin accepting. php-cgi binds
/// almost immediately; this is generous enough for a cold start off a slow disk while
/// still failing rather than hanging if the binary refuses to run at all.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);

/// One installed PHP, before its listener is known to be up.
struct Install {
    version: String,
    cgi_binary: PathBuf,
}

pub fn list_available_versions() -> Vec<PhpFpmVersion> {
    let mut installs = discover_installs();
    // Highest version first, matching the Unix backend's ordering.
    installs.sort_by_key(|install| std::cmp::Reverse(version_key(&install.version)));
    installs
        .into_iter()
        .map(|install| PhpFpmVersion {
            fastcgi_pass: address_for(&install.version),
            version: install.version,
        })
        .collect()
}

/// Highest installed version wins, the same rule the Unix backend applies to running pools.
pub fn detect_default() -> Option<PhpFpmVersion> {
    list_available_versions().into_iter().next()
}

/// The address nginx should talk to for `version`, starting a listener if one isn't already
/// answering. `None` means that version isn't installed, or its `php-cgi.exe` wouldn't come
/// up — never "installed but idle", which is a state this backend resolves rather than
/// reports.
pub fn target_for_version(version: &str) -> Option<String> {
    let install = discover_installs()
        .into_iter()
        .find(|install| install.version == version)?;
    ensure_listener(&install)
}

pub fn install_hint() -> String {
    "no PHP with php-cgi.exe was found — install PHP (or a bundle like Laragon or XAMPP), \
     then reopen this dialog"
        .to_string()
}

/// Why a version the user pinned isn't usable. Unlike Unix, "not running" is not among the
/// reasons — DevsPro starts the listener itself — so this is only ever about the install
/// being gone or unusable.
pub fn missing_version_hint(version: &str) -> String {
    format!(
        "PHP {version} could not be started — its php-cgi.exe is no longer installed where DevsPro found it, or it failed to start on 127.0.0.1:{}. Reinstall that version, or leave the version unset to use the highest installed one",
        port_for(version)
    )
}

/// The PHP command-line binary to run artisan with. There are no `php8.3`-style names
/// here; each install is a folder holding both `php-cgi.exe` and `php.exe`, so the CLI for
/// a pinned version is the `php.exe` beside the `php-cgi.exe` serving it. Unpinned, `php`
/// on PATH is what the user would get in a terminal; failing that, the newest install —
/// Laragon and the zip downloads don't touch PATH, and a bare `php` would just fail.
pub fn cli_binary(version: Option<&str>) -> String {
    let mut installs = discover_installs();
    let cli_of = |install: &Install| {
        let php = install.cgi_binary.with_file_name("php.exe");
        php.is_file().then(|| php.to_string_lossy().into_owned())
    };
    if let Some(version) = version.map(str::trim) {
        if let Some(php) = installs.iter().find(|i| i.version == version).and_then(cli_of) {
            return php;
        }
    }
    if let Some(php) = crate::modules::tools::program("php") {
        return php;
    }
    installs.sort_by_key(|install| std::cmp::Reverse(version_key(&install.version)));
    installs.iter().find_map(cli_of).unwrap_or_else(|| "php".to_string())
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

fn discover_installs() -> Vec<Install> {
    let mut binaries: Vec<PathBuf> = Vec::new();

    for root in DIRECT_ROOTS {
        let candidate = Path::new(root).join("php-cgi.exe");
        if candidate.is_file() {
            binaries.push(candidate);
        }
    }
    for root in VERSIONED_ROOTS {
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let candidate = entry.path().join("php-cgi.exe");
                if candidate.is_file() {
                    binaries.push(candidate);
                }
            }
        }
    }
    if let Some(on_path) = php_cgi_on_path() {
        binaries.push(on_path);
    }

    let mut installs: Vec<Install> = Vec::new();
    for binary in binaries {
        // The version comes from the binary rather than from its directory name: the
        // folder is named by whoever unpacked it, and only `php-cgi -v` is authoritative.
        let Some(version) = query_version(&binary) else {
            continue;
        };
        // Two paths can be the same install reached two ways — a versioned directory that
        // is also on PATH. Keyed on version, since that is what a site pins to and what
        // decides the port.
        if installs.iter().any(|install| install.version == version) {
            continue;
        }
        installs.push(Install {
            version,
            cgi_binary: binary,
        });
    }
    installs
}

fn php_cgi_on_path() -> Option<PathBuf> {
    let output = Command::new("where")
        .arg("php-cgi")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next()?.trim();
    (!first.is_empty()).then(|| PathBuf::from(first))
}

/// `php-cgi -v` prints `PHP 8.3.31 (cgi-fcgi) (built: ...)`; only major.minor is kept,
/// because that is the granularity a site pins to and that PHP's own packaging uses.
fn query_version(cgi_binary: &Path) -> Option<String> {
    let output = Command::new(cgi_binary)
        .arg("-v")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_version(&String::from_utf8_lossy(&output.stdout))
}

fn parse_version(version_output: &str) -> Option<String> {
    let first_line = version_output.lines().next()?;
    let full = first_line.strip_prefix("PHP ")?.split_whitespace().next()?;
    let mut parts = full.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    if major.is_empty() || !major.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if minor.is_empty() || !minor.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("{major}.{minor}"))
}

fn version_key(version: &str) -> (u32, u32) {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor)
}

// ---------------------------------------------------------------------------
// Listeners
// ---------------------------------------------------------------------------

/// Derived, not allocated: the rendered vhost holds this port literally, so picking the
/// next free one would silently point yesterday's config at a listener that has moved.
fn port_for(version: &str) -> u16 {
    let (major, minor) = version_key(version);
    9000 + (major as u16 * 100) + minor as u16
}

fn address_for(version: &str) -> String {
    format!("127.0.0.1:{}", port_for(version))
}

fn ensure_listener(install: &Install) -> Option<String> {
    let port = port_for(&install.version);

    // Already answering: either a listener from earlier in this session, one left behind by
    // a previous run, or Laragon's own. Any of them speaks the same FastCGI on the same
    // port, so starting a second is pure duplication.
    if ports::is_backend_reachable(port) {
        return Some(address_for(&install.version));
    }

    Command::new(&install.cgi_binary)
        .arg("-b")
        .arg(address_for(&install.version))
        // php-cgi exits after 500 requests by default and would take the site down with
        // it — the FastCGI process manager that normally restarts it doesn't exist here.
        .env("PHP_FCGI_MAX_REQUESTS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;

    // The handle is deliberately dropped rather than kept: on Windows that does not kill
    // the process, and the port check above is the only state worth trusting — a listener
    // that died is noticed on the next render and replaced, with no bookkeeping to go stale.
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        if ports::is_backend_reachable(port) {
            return Some(address_for(&install.version));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_major_and_minor_from_php_cgi_version_output() {
        assert_eq!(
            parse_version("PHP 8.3.31 (cgi-fcgi) (built: Nov 19 2024 18:32:20)\n").as_deref(),
            Some("8.3")
        );
        // Patch level is dropped: a site pins to 8.3, not to 8.3.31, so a PHP update
        // must not orphan its configuration.
        assert_eq!(parse_version("PHP 8.1.10 (cgi-fcgi)").as_deref(), Some("8.1"));
        assert_eq!(parse_version("PHP 8.5.7 (cgi-fcgi)").as_deref(), Some("8.5"));
    }

    #[test]
    fn rejects_output_that_is_not_a_php_version_banner() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("'php-cgi' is not recognized"), None);
        assert_eq!(parse_version("PHP"), None);
        assert_eq!(parse_version("PHP 8"), None);
        assert_eq!(parse_version("PHP x.y.z"), None);
    }

    /// These numbers end up written into every Laravel vhost, so they have to be stable
    /// across runs and distinct per version — a collision would route one version's
    /// requests to another's interpreter.
    #[test]
    fn ports_are_stable_per_version_and_never_collide() {
        assert_eq!(port_for("8.3"), 9803);
        assert_eq!(port_for("8.1"), 9801);
        assert_eq!(port_for("7.4"), 9704);
        // Two-digit minors stay inside their major's block.
        assert_eq!(port_for("8.10"), 9810);

        let versions = ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.10"];
        let mut ports: Vec<u16> = versions.iter().map(|v| port_for(v)).collect();
        let count = ports.len();
        ports.sort_unstable();
        ports.dedup();
        assert_eq!(ports.len(), count, "every version gets its own port");
    }

    #[test]
    fn addresses_are_loopback_only() {
        // A dev machine's PHP interpreter has no business being reachable off-box.
        assert_eq!(address_for("8.3"), "127.0.0.1:9803");
    }

    #[test]
    fn newest_version_is_listed_first() {
        // Ordering is by (major, minor) numerically, so 8.10 must outrank 8.9 — which
        // string ordering would get backwards.
        let mut keys = [version_key("8.9"), version_key("8.10"), version_key("7.4")];
        keys.sort_by_key(|k| std::cmp::Reverse(*k));
        assert_eq!(keys, [(8, 10), (8, 9), (7, 4)]);
    }
}
