//! The permission check OpenSSH makes on a private key before using it, and
//! the `chmod 600` that satisfies it.
//!
//! The terminal runs the system `ssh`, which refuses a key the user owns if
//! group or others have any access to it ("UNPROTECTED PRIVATE KEY FILE"),
//! then fails with "Permission denied (publickey)". SFTP goes through `ssh2`,
//! which doesn't check, so the same host browses fine and only the terminal
//! fails. The frontend checks before starting `ssh` and offers the fix; the
//! fix itself only ever runs when the user has said yes to it.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeyPermissions {
    /// The permission bits, e.g. `0o664`. 0 off unix, where there are none.
    pub mode: u32,
    /// `ssh` will refuse the key as it stands.
    pub too_open: bool,
}

/// OpenSSH's rule (`sshkey_perm_ok` in authfile.c): a key owned by the user
/// must have no group or other bits. A key owned by someone else isn't
/// checked at all, so it is never "too open" here either.
pub fn too_open(mode: u32, owned: bool) -> bool {
    owned && mode & 0o077 != 0
}

/// The file `ssh -i <path>` reads. The PTY starts ssh without a shell, so a
/// leading `~/` reaches ssh as typed and ssh expands it itself.
fn resolve(path: &str) -> Result<PathBuf, String> {
    match path.strip_prefix("~/") {
        Some(rest) => Ok(super::home_dir()?.join(rest)),
        None => Ok(PathBuf::from(path)),
    }
}

/// The real uid, which is what OpenSSH compares the key's owner with.
#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: getuid takes no arguments, touches no memory and cannot fail.
    unsafe { libc::getuid() }
}

#[cfg(unix)]
pub fn check(path: &Path) -> Result<KeyPermissions, String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let meta = std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mode = meta.permissions().mode() & 0o777;
    Ok(KeyPermissions { mode, too_open: too_open(mode, meta.uid() == current_uid()) })
}

#[cfg(not(unix))]
pub fn check(_path: &Path) -> Result<KeyPermissions, String> {
    Ok(KeyPermissions { mode: 0, too_open: false })
}

/// `chmod 600`, on a file the user owns and nothing else. Someone else's key
/// is left alone; the error says how to get a copy ssh will accept.
#[cfg(unix)]
pub fn restrict(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let meta = std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if meta.uid() != current_uid() {
        return Err(format!(
            "{0} belongs to another user, so it was left as it is. To use a copy of your own, run: install -m 600 '{0}' ~/.ssh/ — then point the host at the copy.",
            path.display()
        ));
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("failed to restrict {}: {e}", path.display()))
}

#[cfg(not(unix))]
pub fn restrict(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[tauri::command(async)]
pub fn ssh_key_permissions(path: String) -> Result<KeyPermissions, String> {
    check(&resolve(&path)?)
}

#[tauri::command(async)]
pub fn ssh_key_restrict(path: String) -> Result<(), String> {
    restrict(&resolve(&path)?)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_key(mode: u32) -> PathBuf {
        let path = std::env::temp_dir().join(format!("devspro-key-perms-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, "-----BEGIN OPENSSH PRIVATE KEY-----\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn group_or_other_bits_make_an_owned_key_too_open() {
        assert!(too_open(0o664, true));
        assert!(too_open(0o640, true));
        assert!(too_open(0o604, true));
        assert!(!too_open(0o600, true));
        assert!(!too_open(0o400, true));
        // ssh doesn't check a key someone else owns.
        assert!(!too_open(0o644, false));
    }

    #[test]
    fn check_reads_the_mode_and_restrict_fixes_it() {
        let path = temp_key(0o664);
        assert_eq!(check(&path).unwrap(), KeyPermissions { mode: 0o664, too_open: true });

        restrict(&path).unwrap();
        assert_eq!(check(&path).unwrap(), KeyPermissions { mode: 0o600, too_open: false });

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_private_key_is_fine_as_it_is() {
        let path = temp_key(0o600);
        assert_eq!(check(&path).unwrap(), KeyPermissions { mode: 0o600, too_open: false });
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_key_is_an_error() {
        assert!(check(Path::new("/definitely/not/a/key")).is_err());
        assert!(restrict(Path::new("/definitely/not/a/key")).is_err());
    }

    #[test]
    fn someone_elses_file_is_never_changed() {
        // Root's /etc/passwd: world-readable, but ssh wouldn't object, and
        // restrict must refuse it with what to run instead.
        if current_uid() == 0 {
            return;
        }
        let passwd = Path::new("/etc/passwd");
        let before = check(passwd).unwrap();
        assert!(!before.too_open);
        let err = restrict(passwd).unwrap_err();
        assert!(err.contains("belongs to another user"), "{err}");
        assert!(err.contains("install -m 600"), "{err}");
        assert_eq!(check(passwd).unwrap(), before);
    }

    #[test]
    fn a_leading_tilde_is_the_home_directory() {
        assert_eq!(resolve("/abs/key").unwrap(), PathBuf::from("/abs/key"));
        assert_eq!(resolve("~/.ssh/k").unwrap(), crate::modules::ssh::home_dir().unwrap().join(".ssh/k"));
    }
}
