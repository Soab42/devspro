use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use time::{Duration, OffsetDateTime};

const LEAF_VALID_DAYS: i64 = 825; // under the ~2yr ceiling modern browsers accept

#[derive(Debug, Clone, Serialize)]
pub struct LeafCert {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub not_after: String,
}

fn cert_path(certs_dir: &Path, hostname: &str) -> PathBuf {
    certs_dir.join(format!("{hostname}.pem"))
}

fn key_path(certs_dir: &Path, hostname: &str) -> PathBuf {
    certs_dir.join(format!("{hostname}-key.pem"))
}

/// Issues a fresh leaf certificate for `hostname`, signed by the local root CA,
/// overwriting any existing one. Always re-signing rather than reusing a still-valid
/// file is deliberate: a leaf signed by an *earlier* root CA looks valid on disk but
/// chains to nothing, and re-issuing is how that gets repaired. Requires no elevation — only *installing the root CA* into a
/// trust store does; a leaf signed by a CA already trusted needs no further trust
/// action of its own.
pub fn issue(certs_dir: &Path, hostname: &str) -> Result<LeafCert, String> {
    let cert_file = cert_path(certs_dir, hostname);
    let key_file = key_path(certs_dir, hostname);

    let mut params =
        CertificateParams::new(vec![hostname.to_string()]).map_err(|e| format!("invalid hostname for cert: {e}"))?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, hostname);
    params.distinguished_name = dn;

    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(LEAF_VALID_DAYS);
    let not_after = params.not_after;

    let (issuer_cert, issuer_key) = super::load_issuer(certs_dir)?;
    let leaf_key = KeyPair::generate().map_err(|e| format!("failed to generate leaf key: {e}"))?;
    let cert = params
        .signed_by(&leaf_key, &issuer_cert, &issuer_key)
        .map_err(|e| format!("failed to sign leaf cert for {hostname}: {e}"))?;

    fs::write(&cert_file, cert.pem()).map_err(|e| format!("failed to write leaf cert: {e}"))?;
    write_private(&key_file, &leaf_key.serialize_pem())?;

    Ok(LeafCert {
        cert_path: cert_file,
        key_path: key_file,
        not_after: not_after.to_string(),
    })
}

#[cfg(unix)]
fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("failed to create {path:?}: {e}"))?;
    file.write_all(contents.as_bytes())
        .map_err(|e| format!("failed to write {path:?}: {e}"))
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|e| format!("failed to write {path:?}: {e}"))
}
