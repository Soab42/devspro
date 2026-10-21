//! The Recycle Bin, Windows' Trash. Each fixed drive keeps one folder per user
//! at `X:\$Recycle.Bin\<SID>`; those are what the scan sizes. Emptying goes
//! through the shell rather than deleting the folders' contents, so Explorer's
//! own bookkeeping and the desktop icon stay in step.

use std::path::PathBuf;
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::System::WindowsProgramming::DRIVE_FIXED;
use windows_sys::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND};

/// What SHEmptyRecycleBinW returns when there was nothing to empty.
const E_UNEXPECTED: i32 = 0x8000_FFFFu32 as i32;

/// This user's Recycle Bin folder on every fixed drive, whether or not it exists yet.
pub fn bins() -> Vec<PathBuf> {
    let Some(sid) = user_sid() else { return Vec::new() };
    // SAFETY: no arguments.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|root| {
            let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
            // SAFETY: `wide` is NUL-terminated and outlives the call.
            unsafe { GetDriveTypeW(wide.as_ptr()) == DRIVE_FIXED }
        })
        .map(|root| PathBuf::from(root).join("$Recycle.Bin").join(&sid))
        .collect()
}

/// Empties the Recycle Bin on every drive, without asking or animating.
pub fn empty() -> Result<(), String> {
    // SAFETY: a null window and a null root (meaning every drive) are both allowed.
    let hr = unsafe {
        SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND)
    };
    match hr {
        0 | E_UNEXPECTED => Ok(()),
        _ => Err(format!("could not empty the Recycle Bin (HRESULT {:#010x})", hr as u32)),
    }
}

/// The current user's SID as text ("S-1-5-21-…"), which names their folder in each bin.
fn user_sid() -> Option<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo-handle needs no closing; `token` is closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    let sid = (|| {
        let mut needed = 0u32;
        // SAFETY: a zero-length query only reports the size it needs.
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
        if needed == 0 {
            return None;
        }
        // u64s, so the TOKEN_USER at the front is properly aligned.
        let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: `buf` is writable for `needed` bytes.
        if unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), needed, &mut needed) } == 0 {
            return None;
        }
        // SAFETY: on success `buf` begins with a TOKEN_USER whose SID points into `buf`.
        let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
        let mut text: *mut u16 = std::ptr::null_mut();
        // SAFETY: the SID is valid while `buf` lives; `text` is freed with LocalFree below.
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 || text.is_null() {
            return None;
        }
        // SAFETY: `text` is a NUL-terminated wide string from the call above.
        let s = unsafe {
            let len = (0..).take_while(|&n| *text.add(n) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
        };
        // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc.
        unsafe { LocalFree(text.cast()) };
        Some(s)
    })();
    // SAFETY: the token handle is ours and closed exactly once.
    unsafe { CloseHandle(token) };
    sid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_drive_has_a_bin_named_by_our_sid() {
        let sid = user_sid().expect("no SID for the current user");
        assert!(sid.starts_with("S-1-"), "{sid}");
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let expected = PathBuf::from(format!("{system}\\")).join("$Recycle.Bin").join(&sid);
        assert!(bins().contains(&expected), "{:?}", bins());
    }
}
