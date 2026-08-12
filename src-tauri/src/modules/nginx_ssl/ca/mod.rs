//! Local trusted CA, mkcert-style — but generated in-process with `rcgen` rather than
//! shelling out to the `mkcert` binary (see the plan: this machine's own state, certs
//! on disk with no `mkcert` binary installed, is exactly the failure mode that
//! justifies the choice). Root CA key generation needs no elevation; only installing
//! the resulting cert into the OS/browser trust stores does (see `crate::elevate`).

pub mod leaf;

use rcgen::{BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use time::{Duration, OffsetDateTime};

const CA_COMMON_NAME: &str = "DevsPro Local Development CA";
const CA_VALID_YEARS: i64 = 10;

fn root_cert_path(certs_dir: &Path) -> PathBuf {
    certs_dir.join("rootCA.pem")
}

fn root_key_path(certs_dir: &Path) -> PathBuf {
    certs_dir.join("rootCA-key.pem")
}

/// Sidecar file recording the root CA's expiry as RFC3339, written once at creation
/// time. Avoids pulling in an X.509 parsing crate just to read a date back out of a
/// PEM we already know the expiry of when we create it.
fn root_expiry_path(certs_dir: &Path) -> PathBuf {
    certs_dir.join("rootCA-expiry.txt")
}

#[derive(Debug, Serialize)]
pub struct CaStatus {
    pub exists: bool,
    pub not_after: Option<String>,
}

pub fn status(certs_dir: &Path) -> CaStatus {
    if !root_cert_path(certs_dir).exists() {
        return CaStatus {
            exists: false,
            not_after: None,
        };
    }
    let not_after = fs::read_to_string(root_expiry_path(certs_dir)).ok();
    CaStatus {
        exists: true,
        not_after,
    }
}

/// Creates the root CA if it doesn't already exist. Idempotent — safe to call on
/// every app launch.
pub fn ensure_root_ca(certs_dir: &Path) -> Result<(), String> {
    let cert_path = root_cert_path(certs_dir);
    let key_path = root_key_path(certs_dir);
    if cert_path.exists() && key_path.exists() {
        return Ok(());
    }

    fs::create_dir_all(certs_dir).map_err(|e| format!("failed to create certs dir: {e}"))?;

    let key_pair = KeyPair::generate().map_err(|e| format!("failed to generate CA key: {e}"))?;

    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, CA_COMMON_NAME);
    dn.push(DnType::OrganizationName, "DevsPro (local, do not trust off this machine)");
    params.distinguished_name = dn;
    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(365 * CA_VALID_YEARS);
    let not_after = params.not_after;

    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| format!("failed to self-sign root CA: {e}"))?;

    write_private(&key_path, &key_pair.serialize_pem())?;
    fs::write(&cert_path, cert.pem()).map_err(|e| format!("failed to write root CA cert: {e}"))?;
    fs::write(root_expiry_path(certs_dir), not_after.to_string())
        .map_err(|e| format!("failed to write root CA expiry sidecar: {e}"))?;

    Ok(())
}

/// rcgen 0.13's `signed_by` wants an in-memory `Certificate` + `KeyPair` for the
/// issuer, not just a PEM on disk (it has no "load an existing cert as issuer"
/// path). We reconstruct an equivalent `Certificate` here from the persisted key and
/// the same fixed DN used at creation time, then self-sign it in memory purely to
/// get a handle `signed_by` accepts — this reconstruction is never written to disk;
/// the real root CA file on disk is untouched. `signed_by` only reads the issuer's
/// DN/key-identifier/key-usage fields, none of which depend on the reconstruction's
/// own (unused) serial number or validity dates, so this is safe even though it
/// doesn't byte-for-byte match the original self-signed cert.
pub(super) fn load_issuer(certs_dir: &Path) -> Result<(Certificate, KeyPair), String> {
    let key_pem =
        fs::read_to_string(root_key_path(certs_dir)).map_err(|e| format!("root CA key missing: {e}"))?;
    let key_pair = KeyPair::from_pem(&key_pem).map_err(|e| format!("invalid root CA key: {e}"))?;

    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, CA_COMMON_NAME);
    dn.push(DnType::OrganizationName, "DevsPro (local, do not trust off this machine)");
    params.distinguished_name = dn;

    let issuer_cert = params
        .self_signed(&key_pair)
        .map_err(|e| format!("failed to reconstruct CA issuer: {e}"))?;
    Ok((issuer_cert, key_pair))
}

pub(super) fn root_cert_file(certs_dir: &Path) -> PathBuf {
    root_cert_path(certs_dir)
}

#[cfg(unix)]
fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("failed to create {path:?}: {e}"))?;
    use std::io::Write;
    file.write_all(contents.as_bytes())
        .map_err(|e| format!("failed to write {path:?}: {e}"))
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|e| format!("failed to write {path:?}: {e}"))
}

#[tauri::command]
pub fn nginx_ssl_ca_status(app: tauri::AppHandle) -> Result<CaStatus, String> {
    let certs_dir = super::certs_dir(&app)?;
    Ok(status(&certs_dir))
}

#[tauri::command]
pub fn nginx_ssl_ca_ensure(app: tauri::AppHandle) -> Result<CaStatus, String> {
    let certs_dir = super::certs_dir(&app)?;
    ensure_root_ca(&certs_dir)?;
    Ok(status(&certs_dir))
}

/// Every NSS database that might need this CA imported, computed here
/// (unprivileged, so `$HOME` is trustworthy) rather than inside the elevated
/// helper — pkexec does not reliably preserve the calling user's `$HOME` into the
/// elevated process. `~/.pki/nssdb` is the shared database Chrome/Chromium reads;
/// Firefox instead keeps a separate `cert9.db` per profile, so it needs its own
/// entry even after Chrome's import succeeds.
fn nss_db_paths_for_current_user() -> Vec<String> {
    let Ok(home) = std::env::var("HOME") else {
        return Vec::new();
    };
    let mut paths = vec![format!("sql:{home}/.pki/nssdb")];

    let firefox_dir = std::path::Path::new(&home).join(".mozilla/firefox");
    if let Ok(entries) = std::fs::read_dir(&firefox_dir) {
        for entry in entries.flatten() {
            let profile_dir = entry.path();
            if profile_dir.join("cert9.db").is_file() {
                paths.push(format!("sql:{}", profile_dir.to_string_lossy()));
            }
        }
    }

    // Flatpak browsers (Chrome, Chromium, Brave, Edge, ...) are sandboxed with
    // their own isolated data dir and don't see ~/.pki/nssdb at all — each keeps
    // its own NSS database at this same conventional path inside its own sandbox
    // data directory. Scanned generically rather than hardcoding app IDs.
    let flatpak_apps_dir = std::path::Path::new(&home).join(".var/app");
    if let Ok(entries) = std::fs::read_dir(&flatpak_apps_dir) {
        for entry in entries.flatten() {
            let db_dir = entry.path().join("data/pki/nssdb");
            if db_dir.join("cert9.db").is_file() {
                paths.push(format!("sql:{}", db_dir.to_string_lossy()));
            }
        }
    }

    paths
}

fn certutil_available() -> bool {
    std::process::Command::new("which")
        .arg("certutil")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Debian/Ubuntu, Fedora, and Arch each name the NSS command-line tools package
/// differently.
fn nss_tools_package(manager: &str) -> &'static str {
    match manager {
        "dnf" => "nss-tools",
        "pacman" => "nss",
        _ => "libnss3-tools",
    }
}

#[tauri::command]
pub fn nginx_ssl_ca_install(app: tauri::AppHandle) -> Result<String, String> {
    let certs_dir = super::certs_dir(&app)?;
    ensure_root_ca(&certs_dir)?;
    let cert_path = root_cert_file(&certs_dir).to_string_lossy().to_string();
    let allowed_write_prefix = super::app_root_dir(&app)?.to_string_lossy().to_string();
    let nss_db_paths = nss_db_paths_for_current_user();

    let mut operations = Vec::new();
    // Firefox and Chrome-on-Linux read the NSS database, not the OS trust store —
    // `certutil` (from libnss3-tools/nss-tools/nss) is what writes to it, and it's
    // often not installed by default, silently skipping that half of "install the
    // CA" otherwise.
    if !nss_db_paths.is_empty() && !certutil_available() {
        if let Some(manager) = crate::modules::nginx_ssl::nginx::linux_package_manager() {
            let package = nss_tools_package(&manager).to_string();
            operations.push(crate::elevate::Operation::InstallPackage {
                manager,
                packages: vec![package],
            });
        }
    }
    operations.push(crate::elevate::Operation::InstallRootCa { cert_path, nss_db_paths });

    let manifest = crate::elevate::OperationManifest {
        allowed_write_prefix,
        operations,
    };

    let results = crate::elevate::run(&app, manifest)?;
    if results.iter().any(|r| !r.success) {
        return Err(results.iter().map(|r| r.message.as_str()).collect::<Vec<_>>().join("; "));
    }
    Ok(results.iter().map(|r| r.message.as_str()).collect::<Vec<_>>().join("; "))
}
