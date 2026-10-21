//! The Windows side of the File Manager: the shell's own answers where the
//! Linux build asks the desktop. Known folders come from the shell (so a
//! Desktop or Documents that OneDrive moved is found where it really is),
//! deleting goes to the Recycle Bin, "Open" is `ShellExecuteW`, and the
//! devices list is the drive letters, as Explorer's "This PC" shows them.

use super::devices::Device;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::core::{GUID, PWSTR};
use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_ACCESS_DENIED, ERROR_OPEN_FILES, ERROR_SHARING_VIOLATION, GENERIC_READ, GENERIC_WRITE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::NetworkManagement::WNet::{WNetCancelConnection2W, WNetGetConnectionW};
use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{LookupAccountSidW, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};
use windows_sys::Win32::System::Diagnostics::Debug::{SetThreadErrorMode, SEM_FAILCRITICALERRORS};
use windows_sys::Win32::System::Ioctl::{
    FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, IOCTL_STORAGE_EJECT_MEDIA, IOCTL_STORAGE_MEDIA_REMOVAL,
    PREVENT_MEDIA_REMOVAL,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::WindowsProgramming::{DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE};
use windows_sys::Win32::UI::Shell::{
    SHFileOperationW, SHGetKnownFolderPath, ShellExecuteW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
    FOF_WANTNUKEWARNING, FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures,
    FOLDERID_Videos, FO_DELETE, SHFILEOPSTRUCTW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// NUL-terminated UTF-16, for every `W` call.
fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// UTF-16 up to the first NUL.
fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..end]).to_string_lossy().into_owned()
}

/// `\\?\C:\x` -> `C:\x`. `canonicalize` hands back the verbatim form, which
/// the shell's APIs and the frontend's paths both spell without the prefix.
pub fn plain(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// The user folders the sidebar lists, in its order, with the place kind each
/// is drawn as.
pub const KNOWN_PLACES: &[(GUID, &str)] = &[
    (FOLDERID_Desktop, "desktop"),
    (FOLDERID_Documents, "documents"),
    (FOLDERID_Downloads, "downloads"),
    (FOLDERID_Pictures, "pictures"),
    (FOLDERID_Music, "music"),
    (FOLDERID_Videos, "videos"),
];

/// Where the shell keeps one of the user's folders.
pub fn known_folder(id: &GUID) -> Option<PathBuf> {
    let mut raw: PWSTR = null_mut();
    // SAFETY: `raw` receives a CoTaskMemAlloc'd string, freed below whether
    // or not the call succeeded, as the API requires.
    let hr = unsafe { SHGetKnownFolderPath(id, 0, null_mut(), &mut raw) };
    let path = if hr >= 0 && !raw.is_null() {
        // SAFETY: on success `raw` is a valid NUL-terminated string.
        let len = unsafe { (0..).take_while(|&i| *raw.add(i) != 0).count() };
        let slice = unsafe { std::slice::from_raw_parts(raw, len) };
        Some(PathBuf::from(OsString::from_wide(slice)))
    } else {
        None
    };
    // SAFETY: null is allowed; otherwise this is the buffer the shell gave us.
    unsafe { CoTaskMemFree(raw as *const _) };
    path
}

/// The account that owns a file, as Explorer's Security tab names it
/// (`Administrators`, `SYSTEM`, or the user's own name). None when the
/// security descriptor cannot be read, e.g. on a FAT stick, which has none.
pub fn owner_of(path: &Path) -> Option<String> {
    let name = wide(path.as_os_str());
    let mut owner: PSID = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: out-pointers are valid; `owner` points into `descriptor`, which
    // is freed with LocalFree once the name has been looked up.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if rc != 0 || owner.is_null() {
        return None;
    }
    let mut account = [0u16; 256];
    let mut domain = [0u16; 256];
    let (mut account_len, mut domain_len, mut kind) = (account.len() as u32, domain.len() as u32, 0);
    // SAFETY: both buffers are as long as the lengths passed in.
    let ok = unsafe {
        LookupAccountSidW(
            null(),
            owner,
            account.as_mut_ptr(),
            &mut account_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut kind,
        )
    };
    // SAFETY: the descriptor GetNamedSecurityInfoW allocated.
    unsafe { LocalFree(descriptor) };
    (ok != 0).then(|| from_wide(&account)).filter(|n| !n.is_empty())
}

/// The drive a path is on, `C:\` or `\\server\share\`, as the drive-type API
/// wants it.
fn volume_root(path: &Path) -> PathBuf {
    path.ancestors().last().map(Path::to_path_buf).unwrap_or_else(|| path.to_path_buf())
}

/// Moves one path to the Recycle Bin, the way Explorer's Delete does.
///
/// Only a fixed drive has a Recycle Bin: Windows deletes from a USB stick, a
/// memory card or a network share outright. Those are refused here rather
/// than destroyed, since every caller (the Cleaner included) promises the
/// user that a delete can be undone.
pub fn recycle(path: &Path) -> Result<(), String> {
    let full = plain(&std::path::absolute(path).map_err(|e| format!("could not resolve {}: {e}", path.display()))?);
    let root = volume_root(&full);
    // SAFETY: a valid NUL-terminated string.
    let kind = unsafe { GetDriveTypeW(wide(root.as_os_str()).as_ptr()) };
    if kind != DRIVE_FIXED {
        return Err(format!(
            "{} has no Recycle Bin, so nothing was deleted — Windows would remove it permanently",
            root.display()
        ));
    }
    // pFrom is a list: each name NUL-terminated, the list ended by one more.
    let mut from = wide(full.as_os_str());
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: null(),
        // FOF_WANTNUKEWARNING: should the item somehow not fit in the bin,
        // Windows asks before deleting it for good instead of doing so quietly.
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOF_WANTNUKEWARNING) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: null_mut(),
        lpszProgressTitle: null(),
    };
    // SAFETY: `op` and the buffer it points at live across the call.
    let rc = unsafe { SHFileOperationW(&mut op) };
    if op.fAnyOperationsAborted != 0 {
        return Err(format!("{} was not deleted", full.display()));
    }
    match rc as u32 {
        0 => Ok(()),
        ERROR_SHARING_VIOLATION => Err(format!("{} is open in another program", full.display())),
        ERROR_ACCESS_DENIED => Err(format!("permission denied: {}", full.display())),
        code => Err(format!("could not move {} to the Recycle Bin (error {code:#x})", full.display())),
    }
}

/// Opens a file, folder or web address with whatever Windows has
/// associated with it, as a double-click in Explorer does.
pub fn shell_open(target: &str) -> Result<(), String> {
    let verb = wide(OsStr::new("open"));
    let file = wide(OsStr::new(target));
    // Some handlers are shell extensions that need COM on the calling thread.
    // SAFETY: balanced by CoUninitialize below when it succeeded.
    let com = unsafe { CoInitializeEx(null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32) };
    // SAFETY: valid NUL-terminated strings; the rest may be null.
    let result = unsafe { ShellExecuteW(null_mut(), verb.as_ptr(), file.as_ptr(), null(), null(), SW_SHOWNORMAL) };
    if com >= 0 {
        // SAFETY: pairs with the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
    // Anything above 32 is success; the rest are the old error codes.
    match result as usize {
        n if n > 32 => Ok(()),
        2 | 3 => Err(format!("{target} does not exist")),
        5 => Err(format!("permission denied: {target}")),
        31 => Err(format!("no app is set to open {target}")),
        n => Err(format!("could not open {target} (error {n})")),
    }
}

/// `Z:` -> `\\nas\share`, from the session's own record of the mapping, so
/// listing a share whose server is down does not wait on the network.
fn mapped_share(letter: char) -> Option<String> {
    let local = wide(OsStr::new(&format!("{letter}:")));
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` is `len` characters long.
    let rc = unsafe { WNetGetConnectionW(local.as_ptr(), buf.as_mut_ptr(), &mut len) };
    (rc == 0).then(|| from_wide(&buf))
}

/// Every drive letter with something on it: local disks, USB sticks, discs
/// and mapped shares. An empty card reader or DVD drive is left out, as
/// there is nothing to open.
pub fn drives() -> Vec<Device> {
    // Without this, asking an empty drive for its label can raise the old
    // "There is no disk in the drive" dialog.
    let mut previous = 0;
    // SAFETY: plain flag set; restored before returning.
    unsafe { SetThreadErrorMode(SEM_FAILCRITICALERRORS, &mut previous) };
    // SAFETY: no arguments.
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let root_w = wide(OsStr::new(&root));
        // SAFETY: a valid NUL-terminated string.
        let kind = match unsafe { GetDriveTypeW(root_w.as_ptr()) } {
            DRIVE_FIXED | DRIVE_RAMDISK => "drive",
            DRIVE_REMOVABLE => "usb",
            DRIVE_CDROM => "disc",
            DRIVE_REMOTE => "network",
            _ => continue,
        };
        let (label, free) = if kind == "network" {
            // Never touch the share itself: a mapped drive whose server is
            // offline would hold every poll of this list up for seconds.
            let share = mapped_share(letter);
            let label = share
                .as_deref()
                .map(|s| {
                    let mut parts = s.trim_start_matches('\\').splitn(2, '\\');
                    let host = parts.next().unwrap_or("");
                    match parts.next() {
                        Some(share) => format!("{share} on {host}"),
                        None => host.to_string(),
                    }
                })
                .unwrap_or_else(|| "Network Drive".to_string());
            (label, None)
        } else {
            let mut name = [0u16; 261];
            // SAFETY: `name` is as long as the size passed; the rest may be null.
            let ok = unsafe {
                GetVolumeInformationW(
                    root_w.as_ptr(),
                    name.as_mut_ptr(),
                    name.len() as u32,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    0,
                )
            };
            if ok == 0 {
                continue;
            }
            let label = from_wide(&name);
            let label = if label.is_empty() {
                match kind {
                    "usb" => "USB Drive".to_string(),
                    "disc" => "CD Drive".to_string(),
                    _ => "Local Disk".to_string(),
                }
            } else {
                label
            };
            let mut available = 0u64;
            // SAFETY: valid out-pointer; the other two may be null.
            let ok = unsafe { GetDiskFreeSpaceExW(root_w.as_ptr(), &mut available, null_mut(), null_mut()) };
            (label, (ok != 0).then_some(available))
        };
        out.push(Device {
            id: format!("{letter}:"),
            label: format!("{label} ({letter}:)"),
            kind: kind.to_string(),
            path: Some(root),
            mount: None,
            unmount: (kind == "network").then(|| format!("net:{letter}")),
            eject: matches!(kind, "usb" | "disc").then(|| format!("drive:{letter}")),
            free,
        });
    }
    // SAFETY: restores the mode saved above.
    unsafe { SetThreadErrorMode(previous, null_mut()) };
    out
}

/// One `DeviceIoControl` with no buffers.
fn ioctl(handle: windows_sys::Win32::Foundation::HANDLE, code: u32, input: Option<&PREVENT_MEDIA_REMOVAL>) -> bool {
    let mut returned = 0u32;
    let (ptr, len) = match input {
        Some(p) => (p as *const PREVENT_MEDIA_REMOVAL as *const _, std::mem::size_of::<PREVENT_MEDIA_REMOVAL>() as u32),
        None => (null(), 0),
    };
    // SAFETY: `handle` is open; the input buffer, when there is one, lives across the call.
    unsafe { DeviceIoControl(handle, code, ptr, len, null_mut(), 0, &mut returned, null_mut()) != 0 }
}

/// Makes a USB stick, memory card or disc safe to remove: the volume is
/// locked (which fails while anything has a file open on it), dismounted so
/// every write is flushed, and the medium ejected — what "Eject" in
/// Explorer does for a removable drive.
pub fn eject(letter: char) -> Result<(), String> {
    let volume = wide(OsStr::new(&format!(r"\\.\{letter}:")));
    // SAFETY: a valid NUL-terminated string; no security attributes or template.
    let handle = unsafe {
        CreateFileW(
            volume.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(format!("could not open {letter}: — {}", std::io::Error::last_os_error()));
    }
    let result = (|| {
        // Something may be finishing a write; give it a moment before giving up.
        let locked = (0..10).any(|attempt| {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            ioctl(handle, FSCTL_LOCK_VOLUME, None)
        });
        if !locked {
            return Err(format!("{letter}: is in use — close any files or windows open on it and try again"));
        }
        if !ioctl(handle, FSCTL_DISMOUNT_VOLUME, None) {
            return Err(format!("could not dismount {letter}: — {}", std::io::Error::last_os_error()));
        }
        let allow = PREVENT_MEDIA_REMOVAL { PreventMediaRemoval: 0 };
        ioctl(handle, IOCTL_STORAGE_MEDIA_REMOVAL, Some(&allow));
        if !ioctl(handle, IOCTL_STORAGE_EJECT_MEDIA, None) {
            return Err(format!("could not eject {letter}: — {}", std::io::Error::last_os_error()));
        }
        Ok(())
    })();
    // SAFETY: the handle opened above.
    unsafe { CloseHandle(handle) };
    result
}

/// Disconnects a mapped network drive for this session. Not forced: a share
/// with files still open on it stays connected and says so.
pub fn disconnect(letter: char) -> Result<(), String> {
    let local = wide(OsStr::new(&format!("{letter}:")));
    // SAFETY: a valid NUL-terminated string.
    match unsafe { WNetCancelConnection2W(local.as_ptr(), 0, 0) } {
        0 => Ok(()),
        ERROR_OPEN_FILES => Err(format!("{letter}: has files open — close them and try again")),
        code => Err(format!("could not disconnect {letter}: — {}", std::io::Error::from_raw_os_error(code as i32))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_dropped() {
        assert_eq!(plain(Path::new(r"\\?\C:\Users\me")), PathBuf::from(r"C:\Users\me"));
        assert_eq!(plain(Path::new(r"\\?\UNC\nas\share\x")), PathBuf::from(r"\\nas\share\x"));
        assert_eq!(plain(Path::new(r"C:\x")), PathBuf::from(r"C:\x"));
    }

    #[test]
    fn the_system_drive_and_the_profile_folders_are_found() {
        let list = drives();
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let c = list.iter().find(|d| d.id.eq_ignore_ascii_case(&system)).expect("system drive listed");
        assert_eq!(c.kind, "drive");
        assert!(c.label.ends_with(&format!("({})", system.to_ascii_uppercase())), "{}", c.label);
        assert!(c.free.is_some());
        assert_eq!(c.eject, None);
        assert!(known_folder(&FOLDERID_Documents).is_some_and(|p| p.is_absolute()));
    }

    #[test]
    fn owner_is_read_from_the_security_descriptor() {
        let dir = std::env::temp_dir().join(format!("devspro-owner-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(owner_of(&dir).is_some_and(|o| !o.is_empty()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Ignored by default: it leaves a file in the real Recycle Bin, which
    /// the Linux trash tests take care never to do. `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn the_recycle_bin_takes_files_off_a_fixed_drive() {
        let dir = std::env::temp_dir().join(format!("devspro-recycle-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("doomed.txt");
        std::fs::write(&file, "x").unwrap();
        recycle(&file).unwrap();
        assert!(!file.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
