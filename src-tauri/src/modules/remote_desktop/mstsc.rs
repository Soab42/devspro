//! Windows' built-in RDP client, Remote Desktop Connection (`mstsc.exe`).
//!
//! mstsc takes almost nothing on its command line — no username, no domain,
//! no password — so a launch writes the saved connection out as a `.rdp` file
//! and hands mstsc that. The file goes in a temp directory and is removed when
//! the client exits.
//!
//! A password can't go in the `.rdp` file either (only as a DPAPI blob tied to
//! the machine), so it is put in Credential Manager under `TERMSRV/<host>`,
//! the target mstsc looks up before it prompts. It is written for the logon
//! session only, and once mstsc exits it is deleted — or, if the user already
//! had a credential saved for that host, theirs is put back as it was.

use std::path::{Path, PathBuf};

use windows_sys::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_SESSION, CRED_TYPE_GENERIC,
};

use super::connections::RemoteDesktop;

/// Strips anything that would end an `.rdp` line early.
fn one_line(value: &str) -> String {
    value.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

/// `DOMAIN\user`, or just the user when there is no domain.
fn account(desktop: &RemoteDesktop) -> String {
    if desktop.domain.is_empty() {
        desktop.username.clone()
    } else {
        format!("{}\\{}", desktop.domain, desktop.username)
    }
}

/// The `.rdp` file for one connection. Pure, so the settings that decide how
/// the window behaves are pinned by tests.
pub fn rdp_file(desktop: &RemoteDesktop) -> String {
    let lines = [
        format!("full address:s:{}:{}", one_line(&desktop.host), desktop.port),
        format!("username:s:{}", one_line(&account(desktop))),
        // 2 is full screen, 1 a window.
        format!("screen mode id:i:{}", if desktop.fullscreen { 2 } else { 1 }),
        // Resize the remote desktop with the window, as FreeRDP's
        // /dynamic-resolution does on Linux.
        "dynamic resolution:i:1".to_string(),
        "redirectclipboard:i:1".to_string(),
        // Warn on an untrusted certificate rather than refuse: mstsc's own
        // dialog then lets the user accept it, the way FreeRDP's /cert:tofu does.
        "authentication level:i:2".to_string(),
        "prompt for credentials:i:0".to_string(),
    ];
    let mut out = lines.join("\r\n");
    out.push_str("\r\n");
    out
}

/// UTF-16LE with a BOM, the encoding mstsc itself saves `.rdp` files in, so a
/// non-ASCII username survives whatever the system code page is.
fn encode(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

pub fn credential_target(host: &str) -> String {
    format!("TERMSRV/{host}")
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// A generic credential, copied out of Credential Manager so it can be put
/// back after the launch overwrote it.
struct SavedCredential {
    user: String,
    blob: Vec<u8>,
    persist: u32,
}

fn read_credential(target: &str) -> Option<SavedCredential> {
    let target = wide(target);
    let mut found: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: `target` is NUL-terminated; on success `found` points at a
    // CREDENTIALW owned by the system until `CredFree`.
    unsafe {
        if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) == 0 || found.is_null() {
            return None;
        }
        let cred = &*found;
        let user = if cred.UserName.is_null() {
            String::new()
        } else {
            let len = (0..).take_while(|&i| *cred.UserName.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(cred.UserName, len))
        };
        let blob = if cred.CredentialBlob.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize).to_vec()
        };
        let saved = SavedCredential { user, blob, persist: cred.Persist };
        CredFree(found.cast());
        Some(saved)
    }
}

fn write_credential(target: &str, user: &str, blob: &[u8], persist: u32) -> Result<(), String> {
    let mut target_w = wide(target);
    let mut user_w = wide(user);
    let mut blob = blob.to_vec();
    // SAFETY: every pointer in the struct outlives the call; the rest is
    // zeroed, which CredWriteW reads as "not set".
    let ok = unsafe {
        let mut cred: CREDENTIALW = std::mem::zeroed();
        cred.Type = CRED_TYPE_GENERIC;
        cred.TargetName = target_w.as_mut_ptr();
        cred.UserName = user_w.as_mut_ptr();
        cred.CredentialBlobSize = blob.len() as u32;
        cred.CredentialBlob = blob.as_mut_ptr();
        cred.Persist = persist;
        CredWriteW(&cred, 0)
    };
    if ok == 0 {
        return Err(format!(
            "could not store the password in Credential Manager: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn delete_credential(target: &str) {
    let target = wide(target);
    // SAFETY: `target` is NUL-terminated.
    unsafe {
        CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0);
    }
}

/// What has to be undone once mstsc exits.
pub struct Prepared {
    pub args: Vec<String>,
    file: PathBuf,
    /// Set when a password was stored: the target, and what was there before.
    credential: Option<(String, Option<SavedCredential>)>,
}

impl Prepared {
    pub fn cleanup(self) {
        let _ = std::fs::remove_file(&self.file);
        if let Some(dir) = self.file.parent() {
            let _ = std::fs::remove_dir(dir);
        }
        if let Some((target, previous)) = self.credential {
            match previous {
                Some(old) => {
                    let _ = write_credential(&target, &old.user, &old.blob, old.persist);
                }
                None => delete_credential(&target),
            }
        }
    }
}

/// Writes the `.rdp` file and, when a password was given, stores it where
/// mstsc will find it. `dir` is where the file goes.
pub fn prepare(desktop: &RemoteDesktop, password: Option<&str>, dir: &Path) -> Result<Prepared, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let file = dir.join(format!("{}.rdp", uuid::Uuid::new_v4().simple()));
    std::fs::write(&file, encode(&rdp_file(desktop)))
        .map_err(|e| format!("could not write {}: {e}", file.display()))?;
    let mut prepared = Prepared { args: vec![file.to_string_lossy().into_owned()], file, credential: None };

    if let Some(password) = password.filter(|p| !p.is_empty()) {
        let target = credential_target(&desktop.host);
        let previous = read_credential(&target);
        // The blob is the password as UTF-16 without a terminator, which is
        // what mstsc's own "remember me" stores.
        let blob: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
        if let Err(e) = write_credential(&target, &account(desktop), &blob, CRED_PERSIST_SESSION) {
            prepared.cleanup();
            return Err(e);
        }
        prepared.credential = Some((target, previous));
    }
    Ok(prepared)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desktop(fullscreen: bool, domain: &str) -> RemoteDesktop {
        RemoteDesktop {
            id: "d".into(),
            name: "Office".into(),
            protocol: "rdp".into(),
            host: "pc.local".into(),
            port: 3390,
            username: "alice".into(),
            domain: domain.into(),
            view_only: false,
            scale: false,
            fullscreen,
            created_at: "now".into(),
        }
    }

    #[test]
    fn rdp_file_carries_address_account_and_window_mode() {
        let text = rdp_file(&desktop(false, "CORP"));
        let lines: Vec<&str> = text.split("\r\n").collect();
        assert!(lines.contains(&"full address:s:pc.local:3390"));
        assert!(lines.contains(&"username:s:CORP\\alice"));
        assert!(lines.contains(&"screen mode id:i:1"));
        assert!(text.ends_with("\r\n"));

        let full = rdp_file(&desktop(true, ""));
        assert!(full.contains("username:s:alice\r\n"));
        assert!(full.contains("screen mode id:i:2\r\n"));
    }

    #[test]
    fn rdp_file_cannot_be_given_extra_lines() {
        let mut d = desktop(false, "");
        d.username = "alice\r\nalternate shell:s:cmd".into();
        let text = rdp_file(&d);
        assert!(!text.lines().any(|l| l.starts_with("alternate shell")));
    }

    #[test]
    fn encoding_is_utf16le_with_bom() {
        assert_eq!(encode("aé"), vec![0xFF, 0xFE, b'a', 0, 0xE9, 0]);
    }

    #[test]
    fn prepare_writes_the_file_and_cleanup_removes_it() {
        let dir = std::env::temp_dir().join(format!("devspro-mstsc-{}", uuid::Uuid::new_v4().simple()));
        let prepared = prepare(&desktop(false, ""), None, &dir).unwrap();
        let file = PathBuf::from(&prepared.args[0]);
        assert_eq!(file.extension().and_then(|e| e.to_str()), Some("rdp"));
        assert_eq!(std::fs::read(&file).unwrap()[..2], [0xFF, 0xFE]);
        prepared.cleanup();
        assert!(!file.exists());
        assert!(!dir.exists());
    }

    /// Round-trips through the real Credential Manager under a target no RDP
    /// host will ever have, and checks a pre-existing entry is put back.
    #[test]
    fn a_stored_password_is_removed_or_restored_afterwards() {
        let mut d = desktop(false, "");
        d.host = format!("devspro-test-{}", uuid::Uuid::new_v4().simple());
        let target = credential_target(&d.host);
        let dir = std::env::temp_dir().join(format!("devspro-mstsc-{}", uuid::Uuid::new_v4().simple()));

        let prepared = prepare(&d, Some("pw"), &dir).unwrap();
        let stored = read_credential(&target).expect("password stored for mstsc");
        assert_eq!(stored.user, "alice");
        assert_eq!(stored.blob, vec![b'p', 0, b'w', 0]);
        prepared.cleanup();
        assert!(read_credential(&target).is_none(), "nothing was there before, so nothing is left");

        write_credential(&target, "bob", &[1, 2], CRED_PERSIST_SESSION).unwrap();
        let prepared = prepare(&d, Some("pw"), &dir).unwrap();
        assert_eq!(read_credential(&target).unwrap().user, "alice");
        prepared.cleanup();
        let restored = read_credential(&target).expect("the user's own entry is back");
        assert_eq!((restored.user.as_str(), restored.blob.as_slice()), ("bob", &[1u8, 2][..]));
        delete_credential(&target);
    }
}
