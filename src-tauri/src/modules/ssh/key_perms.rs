//! The permission check OpenSSH makes on a private key before using it, and
//! the `chmod 600` that satisfies it.
//!
//! The terminal runs the system `ssh`, which refuses a key the user owns if
//! group or others have any access to it ("UNPROTECTED PRIVATE KEY FILE"),
//! then fails with "Permission denied (publickey)". SFTP goes through `ssh2`,
//! which doesn't check, so the same host browses fine and only the terminal
//! fails. The frontend checks before starting `ssh` and offers the fix; the
//! fix itself only ever runs when the user has said yes to it.
//!
//! Windows has no mode bits. Its OpenSSH port makes the same refusal from the
//! key's ACL instead (`check_secure_file_permission` in Win32-OpenSSH's
//! w32-sshfileperm.c), so there the check reads the ACL and the fix replaces
//! it with one granting only you — what `icacls <key> /inheritance:r
//! /grant:r %USERNAME%:F` does, minus any other explicit grants icacls keeps.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeyPermissions {
    /// The permission bits, e.g. `0o664`. 0 off unix, where there are none.
    pub mode: u32,
    /// `ssh` will refuse the key as it stands.
    pub too_open: bool,
    /// Windows: the accounts besides you that the ACL lets at the key (an
    /// owner that isn't you is listed with " (owner)"), so the UI can name
    /// them where Unix shows the mode. Always empty on Unix, and left out of
    /// the JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<String>,
}

/// OpenSSH's rule (`sshkey_perm_ok` in authfile.c): a key owned by the user
/// must have no group or other bits. A key owned by someone else isn't
/// checked at all, so it is never "too open" here either.
#[cfg_attr(not(unix), allow(dead_code))]
pub fn too_open(mode: u32, owned: bool) -> bool {
    owned && mode & 0o077 != 0
}

/// Who an entry in a Windows key's ACL (or its owner) is, in the terms
/// Win32-OpenSSH judges it by.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, PartialEq)]
pub enum Party {
    /// The user ssh runs as.
    Me,
    /// SYSTEM, Administrators or TrustedInstaller: always allowed.
    Trusted,
    /// Anyone else, by account name.
    Other(String),
}

/// Win32-OpenSSH's rule: the owner must be you, SYSTEM or Administrators,
/// and every allow entry must be for one of those (or for the owner, which
/// `windows_acl::read` already leaves out). Returns the offenders; an empty
/// list means ssh will use the key. Unlike Unix, a key someone else owns is
/// refused outright ("Bad owner"), not skipped.
#[cfg(any(windows, test))]
pub fn acl_offenders(owner: &Party, grants: &[Party]) -> Vec<String> {
    let mut out = Vec::new();
    if let Party::Other(name) = owner {
        out.push(format!("{name} (owner)"));
    }
    for grant in grants {
        if let Party::Other(name) = grant {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
    }
    out
}

/// The file `ssh -i <path>` reads. The PTY starts ssh without a shell, so a
/// leading `~/` reaches ssh as typed and ssh expands it itself.
fn resolve(path: &str) -> Result<PathBuf, String> {
    // Windows ssh expands `~\` as well.
    let rest = path.strip_prefix("~/");
    #[cfg(windows)]
    let rest = rest.or_else(|| path.strip_prefix("~\\"));
    match rest {
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
    Ok(KeyPermissions { mode, too_open: too_open(mode, meta.uid() == current_uid()), others: Vec::new() })
}

#[cfg(windows)]
pub fn check(path: &Path) -> Result<KeyPermissions, String> {
    let acl = windows_acl::read(path)?;
    let others = acl_offenders(&acl.owner, &acl.grants);
    Ok(KeyPermissions { mode: 0, too_open: !others.is_empty(), others })
}

#[cfg(not(any(unix, windows)))]
pub fn check(_path: &Path) -> Result<KeyPermissions, String> {
    Ok(KeyPermissions { mode: 0, too_open: false, others: Vec::new() })
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

/// The ACL equivalent of `chmod 600`: inheritance off and a single entry
/// granting you full control. Like the Unix version it refuses a file owned
/// by another account; Administrators or SYSTEM as owner is fine, since
/// OpenSSH accepts those owners too.
#[cfg(windows)]
pub fn restrict(path: &Path) -> Result<(), String> {
    let acl = windows_acl::read(path)?;
    if let Party::Other(owner) = &acl.owner {
        return Err(format!(
            "{0} belongs to {owner}, so it was left as it is. To use a copy of your own, run: copy \"{0}\" \"%USERPROFILE%\\.ssh\\\" — then point the host at the copy.",
            path.display()
        ));
    }
    windows_acl::grant_only_me(path)
}

#[cfg(not(any(unix, windows)))]
pub fn restrict(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// The Win32 side: reading a file's owner and allow entries, and replacing
/// its DACL. Everything the security APIs allocate is freed before return.
#[cfg(windows)]
mod windows_acl {
    use super::Party;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, GetNamedSecurityInfoW, SetNamedSecurityInfoW, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        AddAccessAllowedAce, EqualSid, GetAce, GetLengthSid, GetTokenInformation, InitializeAcl, IsWellKnownSid,
        LookupAccountSidW, TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACE_HEADER,
        ACL, ACL_REVISION, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// `ACCESS_ALLOWED_ACE_TYPE`. Deny and audit entries don't matter to
    /// OpenSSH, only what is granted.
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    /// The TrustedInstaller service SID, which Win32-OpenSSH also accepts.
    const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

    pub struct Acl {
        pub owner: Party,
        /// Every allow entry except the owner's own.
        pub grants: Vec<Party>,
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    fn os_err(what: &str, path: &Path, code: u32) -> String {
        format!("{what} {}: {}", path.display(), std::io::Error::from_raw_os_error(code as i32))
    }

    /// The current user's `TOKEN_USER`, copied out of the process token.
    /// `u64`s so the buffer is aligned for the struct it holds.
    struct UserSid(Vec<u64>);

    impl UserSid {
        fn current() -> Result<Self, String> {
            let fail = || format!("cannot read your account: {}", std::io::Error::last_os_error());
            let mut token: HANDLE = std::ptr::null_mut();
            // SAFETY: `token` is a valid out-pointer, closed below.
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
                return Err(fail());
            }
            let mut len = 0u32;
            // SAFETY: a size query; a null buffer of length 0 is allowed.
            unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
            let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
            // SAFETY: `buf` holds at least `len` bytes.
            let ok = unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) };
            let err = (ok == 0).then(fail);
            // SAFETY: `token` came from OpenProcessToken and is closed once.
            unsafe { CloseHandle(token) };
            match err {
                Some(e) => Err(e),
                None => Ok(UserSid(buf)),
            }
        }

        fn sid(&self) -> PSID {
            // SAFETY: the buffer holds a TOKEN_USER whose SID points into the
            // same buffer, which lives as long as `self`.
            unsafe { (*(self.0.as_ptr() as *const TOKEN_USER)).User.Sid }
        }
    }

    /// `DOMAIN\name`, or the SID string for an account that no longer exists.
    fn name_of(sid: PSID) -> String {
        let (mut name_len, mut domain_len, mut kind) = (0u32, 0u32, 0);
        // SAFETY: a size query with null buffers.
        unsafe {
            LookupAccountSidW(
                std::ptr::null(),
                sid,
                std::ptr::null_mut(),
                &mut name_len,
                std::ptr::null_mut(),
                &mut domain_len,
                &mut kind,
            )
        };
        if name_len > 0 {
            let mut name = vec![0u16; name_len as usize];
            let mut domain = vec![0u16; domain_len.max(1) as usize];
            // SAFETY: both buffers are the sizes the query asked for.
            let ok = unsafe {
                LookupAccountSidW(
                    std::ptr::null(),
                    sid,
                    name.as_mut_ptr(),
                    &mut name_len,
                    domain.as_mut_ptr(),
                    &mut domain_len,
                    &mut kind,
                )
            };
            if ok != 0 {
                let name = String::from_utf16_lossy(&name[..name_len as usize]);
                let domain = String::from_utf16_lossy(&domain[..domain_len as usize]);
                return if domain.is_empty() { name } else { format!("{domain}\\{name}") };
            }
        }
        sid_string(sid).unwrap_or_else(|| "an unknown account".to_string())
    }

    fn sid_string(sid: PSID) -> Option<String> {
        let mut out: *mut u16 = std::ptr::null_mut();
        // SAFETY: on success `out` is a LocalAlloc'd, NUL-terminated string.
        if unsafe { ConvertSidToStringSidW(sid, &mut out) } == 0 || out.is_null() {
            return None;
        }
        let mut len = 0;
        // SAFETY: walks up to the terminating NUL.
        while unsafe { *out.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: `len` u16s were just walked; the string is freed exactly once.
        let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(out, len) });
        unsafe { LocalFree(out.cast()) };
        Some(text)
    }

    fn classify(sid: PSID, me: PSID) -> Party {
        // SAFETY: both are valid SIDs for the duration of the calls.
        unsafe {
            if EqualSid(sid, me) != 0 {
                return Party::Me;
            }
            if IsWellKnownSid(sid, WinLocalSystemSid) != 0 || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0 {
                return Party::Trusted;
            }
        }
        if sid_string(sid).as_deref() == Some(TRUSTED_INSTALLER) {
            return Party::Trusted;
        }
        Party::Other(name_of(sid))
    }

    pub fn read(path: &Path) -> Result<Acl, String> {
        // Same wording as the Unix side for a missing file.
        std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let me = UserSid::current()?;
        let mut owner: PSID = std::ptr::null_mut();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: the out-pointers are valid. `owner` and `dacl` point into
        // `sd`, which is freed once they are no longer used.
        let rc = unsafe {
            GetNamedSecurityInfoW(
                wide(path).as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut sd,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(os_err("cannot read the permissions of", path, rc));
        }
        let owner_party = classify(owner, me.sid());
        let mut grants = Vec::new();
        if dacl.is_null() {
            // A null DACL grants everyone everything.
            grants.push(Party::Other("Everyone".to_string()));
        } else {
            // SAFETY: `dacl` is valid until `sd` is freed, and GetAce yields
            // pointers into it that are only read inside this loop.
            let count = unsafe { (*dacl).AceCount } as u32;
            for i in 0..count {
                let mut ace: *mut c_void = std::ptr::null_mut();
                if unsafe { GetAce(dacl, i, &mut ace) } == 0 || ace.is_null() {
                    continue;
                }
                if unsafe { (*(ace as *const ACE_HEADER)).AceType } != ACCESS_ALLOWED_ACE_TYPE {
                    continue;
                }
                let sid = unsafe { std::ptr::addr_of!((*(ace as *const ACCESS_ALLOWED_ACE)).SidStart) } as PSID;
                // OpenSSH allows the owner's own entry whoever the owner is;
                // a bad owner is reported as the owner instead.
                if unsafe { EqualSid(sid, owner) } != 0 {
                    continue;
                }
                grants.push(classify(sid, me.sid()));
            }
        }
        // SAFETY: allocated by GetNamedSecurityInfoW; freed once, after the
        // last use of `owner` and `dacl`.
        unsafe { LocalFree(sd) };
        Ok(Acl { owner: owner_party, grants })
    }

    /// Replaces the DACL with a single entry — you, full control — marked
    /// protected so nothing is inherited from the folder.
    pub fn grant_only_me(path: &Path) -> Result<(), String> {
        let me = UserSid::current()?;
        // SAFETY: `me.sid()` is a valid SID.
        let sid_len = unsafe { GetLengthSid(me.sid()) } as usize;
        let size = std::mem::size_of::<ACL>() + std::mem::size_of::<ACCESS_ALLOWED_ACE>()
            - std::mem::size_of::<u32>()
            + sid_len;
        let mut buf = vec![0u64; size.div_ceil(8)];
        let acl = buf.as_mut_ptr() as *mut ACL;
        // SAFETY: `buf` is at least `size` bytes and aligned for an ACL.
        let built = unsafe {
            InitializeAcl(acl, size as u32, ACL_REVISION) != 0
                && AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, me.sid()) != 0
        };
        if !built {
            return Err(format!("failed to restrict {}: {}", path.display(), std::io::Error::last_os_error()));
        }
        // SAFETY: `acl` is a well-formed ACL that outlives the call.
        let rc = unsafe {
            SetNamedSecurityInfoW(
                wide(path).as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                acl,
                std::ptr::null(),
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(os_err("failed to restrict", path, rc));
        }
        Ok(())
    }
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
        assert_eq!(check(&path).unwrap(), KeyPermissions { mode: 0o664, too_open: true, others: vec![] });

        restrict(&path).unwrap();
        assert_eq!(check(&path).unwrap(), KeyPermissions { mode: 0o600, too_open: false, others: vec![] });

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_private_key_is_fine_as_it_is() {
        let path = temp_key(0o600);
        assert_eq!(check(&path).unwrap(), KeyPermissions { mode: 0o600, too_open: false, others: vec![] });
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

#[cfg(test)]
mod acl_tests {
    use super::*;

    #[test]
    fn only_you_system_and_administrators_may_hold_a_windows_key() {
        assert!(acl_offenders(&Party::Me, &[Party::Me, Party::Trusted, Party::Trusted]).is_empty());
        assert!(acl_offenders(&Party::Trusted, &[Party::Me]).is_empty());
        assert_eq!(
            acl_offenders(&Party::Me, &[Party::Me, Party::Other("BUILTIN\\Users".into()), Party::Other("BUILTIN\\Users".into())]),
            ["BUILTIN\\Users"]
        );
        // Unlike Unix, someone else owning the key is itself a refusal.
        assert_eq!(acl_offenders(&Party::Other("PC\\bob".into()), &[Party::Me]), ["PC\\bob (owner)"]);
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::os::windows::process::CommandExt;

    fn temp_key() -> PathBuf {
        let path = std::env::temp_dir().join(format!("devspro-key-perms-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, "-----BEGIN OPENSSH PRIVATE KEY-----\n").unwrap();
        path
    }

    /// What a %TEMP% file inherits varies (sandboxes add their own SIDs), so
    /// the test starts from a restricted file. Granting Everyone (by SID, so
    /// the test doesn't depend on the display language) makes it too open,
    /// and restrict puts it right again.
    #[test]
    fn check_reads_the_acl_and_restrict_fixes_it() {
        let path = temp_key();
        restrict(&path).unwrap();
        assert_eq!(check(&path).unwrap(), KeyPermissions { mode: 0, too_open: false, others: vec![] });

        let granted = std::process::Command::new("icacls")
            .arg(&path)
            .args(["/grant", "*S-1-1-0:(R)"])
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(granted.status.success());
        let open = check(&path).unwrap();
        assert!(open.too_open);
        assert_eq!(open.mode, 0);
        assert_eq!(open.others.len(), 1, "{open:?}");

        restrict(&path).unwrap();
        assert_eq!(check(&path).unwrap(), KeyPermissions { mode: 0, too_open: false, others: vec![] });
        // Still readable by its owner afterwards.
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("-----BEGIN"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_key_is_an_error() {
        let path = Path::new(r"C:\definitely\not\a\key");
        assert!(check(path).unwrap_err().starts_with("cannot read"));
        assert!(restrict(path).is_err());
    }

    #[test]
    fn a_leading_tilde_is_the_profile_directory() {
        let profile = PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
        assert_eq!(resolve("~/.ssh/k").unwrap(), profile.join(".ssh/k"));
    }
}
