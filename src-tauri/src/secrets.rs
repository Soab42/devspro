//! Secrets at rest: AES-256-GCM with a random 32-byte key kept in its own
//! file under the app data dir, created on first use with mode 0600.
//!
//! Each feature that stores secrets (browser logins, SSH passwords) has its
//! own key file, so replacing or deleting one only loses that feature's
//! secrets. The threat model is Chrome on Linux without a keyring: a casual
//! `sqlite3 app.db` shows no plaintext, but someone with the user's files can
//! still decrypt them. The app's profile password isn't used as the key
//! because the secrets are needed while the vault is unlocked, long after
//! that password has left memory.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use std::path::Path;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

pub fn load_or_create_key(path: &Path) -> Result<[u8; 32], String> {
    if let Ok(bytes) = std::fs::read(path) {
        if bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
        return Err(format!("the key file {} is damaged", path.display()));
    }
    let key: [u8; 32] = Aes256Gcm::generate_key(OsRng).into();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, key).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

/// The cipher for the key file at `path`, creating the key if there is none.
pub fn cipher_at(path: &Path) -> Result<Aes256Gcm, String> {
    let key = load_or_create_key(path)?;
    Ok(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key)))
}

/// base64(nonce || ciphertext).
pub fn seal(cipher: &Aes256Gcm, plaintext: &str) -> Result<String, String> {
    let nonce = Aes256Gcm::generate_nonce(OsRng);
    let ct = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| "could not encrypt the password".to_string())?;
    let mut out = nonce.to_vec();
    out.extend(ct);
    Ok(B64.encode(out))
}

pub fn open(cipher: &Aes256Gcm, sealed: &str) -> Result<String, String> {
    let bytes = B64.decode(sealed).map_err(|_| "stored password is not valid base64".to_string())?;
    if bytes.len() < 12 {
        return Err("stored password is truncated".into());
    }
    let (nonce, ct) = bytes.split_at(12);
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| "could not decrypt the password (was the key file replaced?)".to_string())?;
    String::from_utf8(pt).map_err(|_| "stored password is not text".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_and_open_round_trip_and_differ_per_call() {
        let key: [u8; 32] = Aes256Gcm::generate_key(OsRng).into();
        let c = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let a = seal(&c, "hunter2").unwrap();
        let b = seal(&c, "hunter2").unwrap();
        assert_ne!(a, b, "fresh nonce every time");
        assert_eq!(open(&c, &a).unwrap(), "hunter2");
        assert_eq!(open(&c, &b).unwrap(), "hunter2");
    }

    #[test]
    fn wrong_key_or_damaged_blob_is_an_error_not_garbage() {
        let k1: [u8; 32] = Aes256Gcm::generate_key(OsRng).into();
        let k2: [u8; 32] = Aes256Gcm::generate_key(OsRng).into();
        let c1 = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&k1));
        let c2 = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&k2));
        let sealed = seal(&c1, "secret").unwrap();
        assert!(open(&c2, &sealed).is_err());
        assert!(open(&c1, "not base64!").is_err());
        assert!(open(&c1, "AAAA").is_err());
    }

    #[test]
    fn key_file_is_created_once_and_reused() {
        let dir = std::env::temp_dir().join(format!("devspro-key-{}", uuid::Uuid::new_v4()));
        let path = dir.join("browser").join("credentials.key");
        let a = load_or_create_key(&path).unwrap();
        let b = load_or_create_key(&path).unwrap();
        assert_eq!(a, b);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
