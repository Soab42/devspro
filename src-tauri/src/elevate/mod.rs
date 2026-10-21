//! Shared privilege-elevation contract. Any module that needs to touch a system file,
//! reload a system service, or write to an OS trust store builds an `OperationManifest`
//! and calls `run()` — the Rust core itself never runs elevated. `run()` writes the
//! manifest to a temp file, launches the platform-specific elevation prompt pointed at
//! the `ldm-helper` binary, and reads back the helper's result.

mod platform;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Operation {
    Ping,
    /// Full overwrite of one file. Helper rejects any path outside the manifest's
    /// `allowed_write_prefix` — the app's own nginx/sites directory, never anywhere
    /// arbitrary.
    WriteFile { path: String, contents: String },
    RemoveFile { path: String },
    /// Full overwrite of exactly the system hosts file (`/etc/hosts`, or
    /// `<system dir>\drivers\etc\hosts` on Windows) — the destination is fixed
    /// inside the helper, so it deliberately sits outside the `allowed_write_prefix`
    /// gate rather than widening that prefix to `/etc`. The helper writes
    /// atomically and refuses contents with no `localhost` line (on Windows, which
    /// ships without one: contents that drop an existing one), so a bug in the
    /// caller cannot leave the machine without its loopback name.
    WriteHostsFile { contents: String },
    /// Ensures a marker-delimited block exists verbatim inside `file_path` — inserts
    /// it if the markers aren't present, replaces the block's contents if they are.
    /// Used for both the one `include` line in nginx.conf and the whole set of
    /// devspro-owned hosts-file entries (recomputed and passed in full each time,
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
    /// Writes exactly one file, `devspro.conf`, into nginx's `conf.d` directory
    /// (already `include`d from inside `http{}` by every standard nginx package —
    /// Debian/Ubuntu, RHEL/Fedora). This is what actually wires our sites in,
    /// *without* ever touching the user's real nginx.conf: appending our own
    /// `include` line at the end of nginx.conf (the earlier approach) lands after
    /// `http{}`'s closing brace, where a `server{}` block is a syntax error —
    /// that's the exact bug this replaces. The destination filename is fixed by
    /// the helper itself, not caller-controlled.
    EnsureNginxConfDInclude { conf_d_dir: String, sites_glob: String },
    /// Windows counterpart of `EnsureNginxConfDInclude`. The stock Windows nginx has no
    /// `conf.d` convention at all — nothing in its shipped nginx.conf includes such a
    /// directory, and none exists — so there is no drop-in location to write into.
    /// Instead one marker-wrapped `include` line goes *inside* `http{}`. Inserting
    /// inside the block is the entire point: appended at the end of the file it would
    /// land after `http{}`'s closing brace, where an include of `server{}` blocks is a
    /// syntax error — precisely the bug `cleanup_legacy_include_block_op` exists to undo
    /// on Linux. The helper decides nothing here beyond where to splice the block in.
    EnsureIncludeInHttpBlock {
        config_path: String,
        marker_begin: String,
        marker_end: String,
        include_line: String,
    },
    /// `prefix` is nginx's `-p`: `None` everywhere it can be left to nginx itself, and
    /// set on Windows, where it is not optional. The Windows builds are configured with
    /// an empty `--prefix=` and a *relative* `--conf-path=conf/nginx.conf`, which nginx
    /// then resolves against the current working directory rather than against its own
    /// executable — so `nginx -t` run from anywhere but the install directory looks for
    /// the config under the wrong root and reports the whole configuration broken.
    RunNginxTest {
        binary_path: String,
        prefix: Option<String>,
    },
    ReloadNginx {
        binary_path: String,
        prefix: Option<String>,
    },
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
    ///
    /// Nothing to do on Windows, where `php-cgi.exe` runs as the desktop user who
    /// already owns the project folder — the helper reports that rather than inventing
    /// an ACL change nobody asked for.
    FixLaravelPermissions { project_path: String },
    /// Installs the root CA into the OS trust store and, if `nss_db_path` is given,
    /// also imports it into that NSS database (Firefox/Chrome-on-Linux don't read
    /// the OS store). `nss_db_paths` is always empty on Windows: the OS store there is
    /// what Chrome and Edge read, and the `certutil` on PATH is Microsoft's own tool of
    /// that name, not NSS's.
    InstallRootCa {
        cert_path: String,
        /// One `sql:<dir>` entry per NSS database to import into — the shared
        /// `~/.pki/nssdb` (Chrome/Chromium) and each Firefox profile's own
        /// `cert9.db` are separate databases, so Firefox trusting the CA needs
        /// its own import even after Chrome's succeeds.
        nss_db_paths: Vec<String>,
    },
    /// Memory module: `systemctl <action> -- <unit>`. The helper accepts only
    /// "start" or "stop", and only a unit name matching
    /// `^[A-Za-z0-9@._:-]{1,128}$` that doesn't start with `-`.
    ServiceControl { action: String, unit: String },
    /// Memory module: `sync`, then `3` into /proc/sys/vm/drop_caches (page
    /// cache, dentries and inodes). Only clean pages are dropped, hence the sync.
    DropCaches,
    /// Memory module: `swapoff -a && swapon -a`, pulling everything in swap
    /// back into RAM. Refused when what's in swap is more than 80 % of
    /// MemAvailable, since swapoff would then push the machine into the OOM
    /// killer; succeeds without doing anything when swap is empty. Put it last
    /// in a manifest: the helper skips everything after a failed operation.
    ReclaimSwap,
    /// Cleaner, System tab: `apt-get clean`.
    AptClean,
    /// Cleaner, System tab: `journalctl --vacuum-size=<max_bytes>`. The helper
    /// refuses anything under 50 MiB.
    VacuumJournal { max_bytes: u64 },
    /// Cleaner, System tab: `snap remove <name> --revision=<revision>`. `name`
    /// must match `[a-z0-9-]+`, `revision` is digits only, and the helper
    /// re-checks with `snap list --all` that the revision is disabled.
    RemoveSnapRevision { name: String, revision: String },
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

pub fn run(app: &AppHandle, manifest: OperationManifest) -> Result<Vec<OpResult>, String> {
    let tmp_dir = std::env::temp_dir();
    let stem = uuid::Uuid::new_v4().to_string();
    let manifest_path = tmp_dir.join(format!("devspro-manifest-{stem}.json"));
    let result_path = manifest_path.with_extension("result.json");

    let manifest_json =
        serde_json::to_string(&manifest).map_err(|e| format!("failed to serialize manifest: {e}"))?;
    fs::write(&manifest_path, manifest_json)
        .map_err(|e| format!("failed to write manifest: {e}"))?;

    let helper_path = helper_binary_path(app)?;
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
pub fn ping(app: &AppHandle) -> Result<String, String> {
    let results = run(
        app,
        OperationManifest {
            allowed_write_prefix: String::new(),
            operations: vec![Operation::Ping],
        },
    )?;
    match results.first() {
        Some(r) if r.success => Ok(r.message.clone()),
        Some(r) => Err(r.message.clone()),
        None => Err("helper returned no results".to_string()),
    }
}

/// In debug builds, resolves the helper inside the shared workspace `target/`
/// directory (what `cargo tauri dev` runs against, with no bundle around it). In
/// a packaged build the helper instead ships as a Tauri sidecar (`bundle.externalBin`
/// in tauri.conf.json) — the bundler copies `binaries/ldm-helper-<target-triple>`
/// into the bundle next to the main executable, stripped of its triple suffix, so
/// at runtime it sits beside whatever `tauri::process::current_binary` resolves to.
/// This mirrors `tauri_plugin_shell`'s own sidecar path resolution, without pulling
/// in that plugin just to compute a path.
fn helper_binary_path(app: &AppHandle) -> Result<PathBuf, String> {
    let bin_name = if cfg!(windows) { "ldm-helper.exe" } else { "ldm-helper" };

    let path = if cfg!(debug_assertions) {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir.join("target").join("debug").join(bin_name)
    } else {
        let current_exe = tauri::process::current_binary(&app.env())
            .map_err(|e| format!("could not resolve current executable path: {e}"))?;
        let exe_dir = current_exe
            .parent()
            .ok_or_else(|| "current executable path has no parent directory".to_string())?;
        exe_dir.join(bin_name)
    };

    if path.exists() {
        Ok(path)
    } else if cfg!(debug_assertions) {
        Err(format!(
            "ldm-helper binary not found at {path:?} — run `cargo build` from src-tauri first"
        ))
    } else {
        Err(format!(
            "ldm-helper sidecar not found at {path:?} — check that bundle.externalBin in tauri.conf.json points at a built binaries/ldm-helper-<target-triple>"
        ))
    }
}
