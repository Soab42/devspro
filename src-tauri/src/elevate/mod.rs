//! Shared privilege-elevation contract. Any module that needs to touch a system file,
//! reload a system service, or write to an OS trust store builds an `OperationManifest`
//! and calls `run()` — the Rust core itself never runs elevated. `run()` writes the
//! manifest to a temp file, launches the platform-specific elevation prompt pointed at
//! the `ldm-helper` binary, and reads back the helper's result.

mod platform;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Operation {
    Ping,
    /// Full overwrite of one file. Helper rejects any path outside the manifest's
    /// `allowed_write_prefix` — the app's own nginx/sites directory, never anywhere
    /// arbitrary.
    WriteFile { path: String, contents: String },
    RemoveFile { path: String },
    /// Ensures a marker-delimited block exists verbatim inside `file_path` — inserts
    /// it if the markers aren't present, replaces the block's contents if they are.
    /// Used for both the one `include` line in nginx.conf and the whole set of
    /// devsuite-owned /etc/hosts entries (recomputed and passed in full each time,
    /// so this stays idempotent rather than needing incremental add/remove tracking).
    SyncMarkedBlock {
        file_path: String,
        marker_begin: String,
        marker_end: String,
        block_lines: Vec<String>,
    },
    /// Reverses a previously-applied `SyncMarkedBlock` — removes the marker lines
    /// and everything between them entirely (not just emptied). No-op if absent.
    RemoveMarkedBlock {
        file_path: String,
        marker_begin: String,
        marker_end: String,
    },
    /// Writes exactly one file, `devsuite.conf`, into nginx's `conf.d` directory
    /// (already `include`d from inside `http{}` by every standard nginx package —
    /// Debian/Ubuntu, RHEL/Fedora). This is what actually wires our sites in,
    /// *without* ever touching the user's real nginx.conf: appending our own
    /// `include` line at the end of nginx.conf (the earlier approach) lands after
    /// `http{}`'s closing brace, where a `server{}` block is a syntax error —
    /// that's the exact bug this replaces. The destination filename is fixed by
    /// the helper itself, not caller-controlled.
    EnsureNginxConfDInclude { conf_d_dir: String, sites_glob: String },
    RunNginxTest { binary_path: String },
    ReloadNginx { binary_path: String },
    /// Linux only: `manager` is one of "apt", "dnf", "pacman" — whichever
    /// `nginx::linux_package_manager()` detected on this machine. macOS/Windows
    /// never build this op (brew must not run as root; Windows has no such step).
    InstallPackage { manager: String, packages: Vec<String> },
    /// PHP-FPM's pool runs as `www-data` by default on Debian/RHEL, but a project
    /// folder picked from the user's home dir is owned by the desktop user — so
    /// `storage/` and `bootstrap/cache/` need group-write for www-data before
    /// Laravel can log/cache anything. Scoped to exactly those two subdirectories
    /// of a path the user picked via the native folder dialog, not touched
    /// otherwise.
    FixLaravelPermissions { project_path: String },
    /// Installs the root CA into the OS trust store and, if `nss_db_path` is given,
    /// also imports it into that NSS database (Firefox/Chrome-on-Linux don't read
    /// the OS store).
    InstallRootCa {
        cert_path: String,
        /// One `sql:<dir>` entry per NSS database to import into — the shared
        /// `~/.pki/nssdb` (Chrome/Chromium) and each Firefox profile's own
        /// `cert9.db` are separate databases, so Firefox trusting the CA needs
        /// its own import even after Chrome's succeeds.
        nss_db_paths: Vec<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OperationManifest {
    /// Absolute path prefix `WriteFile`/`RemoveFile` are allowed to touch — computed
    /// by the unprivileged core (it knows the real app data dir), never widened by
    /// the helper itself.
    pub allowed_write_prefix: String,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OpResult {
    pub success: bool,
    pub message: String,
}

pub fn run(manifest: OperationManifest) -> Result<Vec<OpResult>, String> {
    let tmp_dir = std::env::temp_dir();
    let stem = uuid::Uuid::new_v4().to_string();
    let manifest_path = tmp_dir.join(format!("devsuite-manifest-{stem}.json"));
    let result_path = manifest_path.with_extension("result.json");

    let manifest_json =
        serde_json::to_string(&manifest).map_err(|e| format!("failed to serialize manifest: {e}"))?;
    fs::write(&manifest_path, manifest_json)
        .map_err(|e| format!("failed to write manifest: {e}"))?;

    let helper_path = helper_binary_path()?;
    let launch_result = platform::launch_helper(&helper_path, &manifest_path);

    let read_results = || -> Result<Vec<OpResult>, String> {
        let raw = fs::read_to_string(&result_path)
            .map_err(|e| format!("helper produced no result file: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("could not parse helper result: {e}"))
    };
    let outcome = launch_result.and_then(|_| read_results());

    let _ = fs::remove_file(&manifest_path);
    let _ = fs::remove_file(&result_path);

    outcome
}

/// Round-trips a single `Ping` op through the real elevation flow. Used by the
/// Settings module to verify the helper is reachable on this OS.
pub fn ping() -> Result<String, String> {
    let results = run(OperationManifest {
        allowed_write_prefix: String::new(),
        operations: vec![Operation::Ping],
    })?;
    match results.first() {
        Some(r) if r.success => Ok(r.message.clone()),
        Some(r) => Err(r.message.clone()),
        None => Err("helper returned no results".to_string()),
    }
}

/// Debug-build lookup only: resolves the helper binary inside the shared workspace
/// target directory. Production packaging should ship this as a Tauri sidecar
/// (`bundle.externalBin`) and resolve it via the app's resource directory instead —
/// deferred until this ships in a released build.
fn helper_binary_path() -> Result<PathBuf, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let bin_name = if cfg!(windows) { "ldm-helper.exe" } else { "ldm-helper" };
    let path = manifest_dir.join("target").join(profile).join(bin_name);
    if path.exists() {
        Ok(path)
    } else {
        Err(format!(
            "ldm-helper binary not found at {path:?} — run `cargo build` from src-tauri first"
        ))
    }
}
