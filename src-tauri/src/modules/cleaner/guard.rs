//! The one gate every removal passes. Scans decide what to offer; this decides
//! whether a path may be removed at all, so a bad scan, a stale id or a path
//! built from user input cannot reach outside the roots the user allowed.
//!
//! The rules are the same on every OS; what counts as a system or credential
//! folder is not, so each OS has its own protected set below. Windows paths
//! compare case-insensitively, and a Windows path is checked twice: as given,
//! and again as the filesystem resolves it, so an 8.3 short name
//! (`C:\PROGRA~1`) or a drive letter mapped onto an admin share cannot reach a
//! protected folder under another spelling.

#[cfg(not(windows))]
use super::super::file_manager;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Never removable, and everything below them is not either.
#[cfg(not(windows))]
const SYSTEM_TREES: &[&str] = &[
    "/bin", "/boot", "/dev", "/etc", "/lib", "/lib32", "/lib64", "/proc", "/sbin", "/sys", "/usr",
];

/// Never removable themselves; what is inside is another matter.
#[cfg(not(windows))]
const SYSTEM_DIRS: &[&str] = &[
    "/", "/home", "/media", "/mnt", "/opt", "/root", "/run", "/snap", "/srv", "/tmp", "/var",
];

/// Under $HOME: credentials, never a cleaning target.
#[cfg(not(windows))]
const HOME_PROTECTED: &[&str] = &[".ssh", ".gnupg"];

/// Ok when `path` may be removed: absolute, free of `..`, strictly inside one
/// of `allowed_roots`, reached through no symlink (the path itself included),
/// not $HOME, `/` or a system directory, and not under an excluded prefix.
pub fn check_removable<R, E>(path: &Path, allowed_roots: &[R], exclusions: &[E]) -> Result<(), String>
where
    R: AsRef<Path>,
    E: AsRef<Path>,
{
    let home = super::home_dir();
    check_with_home(path, allowed_roots, exclusions, home.as_deref())
}

fn check_with_home<R, E>(
    path: &Path,
    allowed_roots: &[R],
    exclusions: &[E],
    home: Option<&Path>,
) -> Result<(), String>
where
    R: AsRef<Path>,
    E: AsRef<Path>,
{
    let shown = path.display();
    if !path.is_absolute() {
        return Err(format!("{shown} is not an absolute path"));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err(format!("{shown} contains . or .. components"));
    }
    if path.parent().is_none() {
        return Err(if cfg!(windows) { format!("refusing to remove the drive {shown}") } else { "refusing to remove /".to_string() });
    }
    if home.is_some_and(|h| same(path, h)) {
        return Err("refusing to remove the home directory".to_string());
    }
    platform::check_protected(path, home)?;
    // An ancestor of an exclusion would remove the excluded thing with it.
    if exclusions.iter().any(|e| within(path, e.as_ref()) || within(e.as_ref(), path)) {
        return Err(format!("{shown} is excluded in the Cleaner settings"));
    }

    let root = allowed_roots
        .iter()
        .map(|r| r.as_ref())
        .filter(|r| r.is_absolute() && within(path, r) && !same(path, r))
        .max_by_key(|r| r.components().count())
        .ok_or_else(|| format!("{shown} is not inside a folder the Cleaner may clean"))?;

    // The root itself is the user's choice (it may live under a symlinked
    // /home); everything from it down to the target must be a real entry.
    // On Windows a junction counts as a link here too.
    let mut cursor = root.to_path_buf();
    for component in path.components().skip(root.components().count()) {
        cursor.push(component);
        let meta = fs::symlink_metadata(&cursor).map_err(|e| format!("{}: {e}", cursor.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!("{} is a symbolic link", cursor.display()));
        }
    }
    platform::check_resolved(path, home)
}

/// Removes `path`. `permanent` deletes outright without following symlinks;
/// otherwise it goes to the trash (the Recycle Bin on Windows). Call
/// `check_removable` first.
pub fn remove_path(path: &Path, permanent: bool) -> Result<(), String> {
    remove_path_with(path, permanent, platform::move_to_trash)
}

fn remove_path_with(
    path: &Path,
    permanent: bool,
    trash: impl Fn(&Path) -> Result<(), String>,
) -> Result<(), String> {
    if !permanent {
        return trash(path);
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let result = if meta.is_dir() {
        fs::remove_dir_all(path)
    } else if meta.file_type().is_symlink() {
        // A Windows directory link or junction is removed as a directory;
        // either way only the link goes, never what it points at.
        fs::remove_file(path).or_else(|_| fs::remove_dir(path))
    } else {
        fs::remove_file(path)
    };
    result.map_err(|e| format!("could not delete {}: {e}", path.display()))
}

/// Whether `path` is a system or credential location this OS protects, the
/// part of `check_removable` that needs no roots and touches no disk.
#[cfg(windows)]
pub fn is_protected(path: &Path) -> bool {
    platform::check_protected(path, super::home_dir().as_deref()).is_err()
}

/// `path` is `base` or below it. Case-insensitive on Windows.
pub fn within(path: &Path, base: &Path) -> bool {
    platform::within(path, base)
}

/// The same path. Case-insensitive on Windows.
pub fn same(a: &Path, b: &Path) -> bool {
    platform::same(a, b)
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn within(path: &Path, base: &Path) -> bool {
        path.starts_with(base)
    }

    pub fn same(a: &Path, b: &Path) -> bool {
        a == b
    }

    pub fn check_protected(path: &Path, home: Option<&Path>) -> Result<(), String> {
        let shown = path.display();
        if SYSTEM_DIRS.iter().any(|d| path == Path::new(d)) || SYSTEM_TREES.iter().any(|d| path.starts_with(d)) {
            return Err(format!("{shown} is a system directory"));
        }
        if let Some(h) = home {
            if HOME_PROTECTED.iter().any(|p| path.starts_with(h.join(p))) {
                return Err(format!("{shown} holds credentials"));
            }
        }
        Ok(())
    }

    /// Nothing more to check: the symlink walk already proved the path is
    /// what it says.
    pub fn check_resolved(_path: &Path, _home: Option<&Path>) -> Result<(), String> {
        Ok(())
    }

    pub fn move_to_trash(path: &Path) -> Result<(), String> {
        file_manager::move_to_trash(path)
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::OsString;
    use std::path::Prefix;

    /// Top-level folders of any drive: never removable, nor anything below.
    /// `windows` and the Program Files pair are listed for every drive, not
    /// just the system one, which is stricter than needed and cheap.
    const DRIVE_TREES: &[&str] = &[
        "$recycle.bin",
        "$windows.~bt",
        "$windows.~ws",
        "$winreagent",
        "boot",
        "config.msi",
        "documents and settings",
        "efi",
        "perflogs",
        "program files",
        "program files (x86)",
        "programdata",
        "recovery",
        "system volume information",
        "windows",
    ];

    /// Files at the root of any drive the OS itself owns.
    const DRIVE_FILES: &[&str] = &[
        "bootmgr",
        "bootnxt",
        "dumpstack.log",
        "dumpstack.log.tmp",
        "hiberfil.sys",
        "pagefile.sys",
        "swapfile.sys",
    ];

    /// The system folders the environment names, in case Windows lives
    /// somewhere other than the defaults above.
    const ENV_TREES: &[&str] = &["SystemRoot", "windir", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData"];

    /// Under the profile: never removable themselves.
    const HOME_DIRS: &[&str] = &["AppData", "AppData/Local", "AppData/LocalLow", "AppData/Roaming"];

    /// Under the profile: credentials and key stores, never a cleaning target.
    /// `Protect` holds the DPAPI master keys every saved browser password and
    /// Wi-Fi key is encrypted with; losing it loses those for good.
    const HOME_PROTECTED: &[&str] = &[
        ".ssh",
        ".gnupg",
        "AppData/Roaming/gnupg",
        "AppData/Local/Microsoft/Credentials",
        "AppData/Local/Microsoft/Vault",
        "AppData/Roaming/Microsoft/Credentials",
        "AppData/Roaming/Microsoft/Crypto",
        "AppData/Roaming/Microsoft/Protect",
        "AppData/Roaming/Microsoft/SystemCertificates",
    ];

    /// One lowercase string per component. Drive prefixes are normalised so
    /// `C:`, `c:` and `\\?\C:` all read `c:`.
    fn key(path: &Path) -> Vec<String> {
        path.components()
            .filter(|c| !matches!(c, Component::RootDir))
            .map(|c| match c {
                Component::Prefix(p) => match p.kind() {
                    Prefix::Disk(d) | Prefix::VerbatimDisk(d) => format!("{}:", (d as char).to_ascii_lowercase()),
                    _ => p.as_os_str().to_string_lossy().to_lowercase(),
                },
                other => other.as_os_str().to_string_lossy().to_lowercase(),
            })
            .collect()
    }

    pub fn within(path: &Path, base: &Path) -> bool {
        let (p, b) = (key(path), key(base));
        p.len() >= b.len() && p[..b.len()] == b[..]
    }

    pub fn same(a: &Path, b: &Path) -> bool {
        key(a) == key(b)
    }

    fn env_trees(env: &dyn Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
        ENV_TREES
            .iter()
            .filter_map(|v| env(v))
            .map(PathBuf::from)
            .filter(|p| p.is_absolute() && p.parent().is_some())
            .collect()
    }

    pub fn check_protected(path: &Path, home: Option<&Path>) -> Result<(), String> {
        check_protected_with(path, home, &|v| std::env::var_os(v))
    }

    pub(super) fn check_protected_with(
        path: &Path,
        home: Option<&Path>,
        env: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<(), String> {
        let shown = path.display();
        match path.components().next() {
            Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_)) => {}
            _ => return Err(format!("{shown} is not on a drive letter; only those can be cleaned")),
        }
        // `name:stream` addresses an alternate data stream, not a file.
        if path.components().skip(1).any(|c| c.as_os_str().to_string_lossy().contains(':')) {
            return Err(format!("{shown} names a data stream, not a file"));
        }
        let parts = key(path);
        let system = Err(format!("{shown} is a system folder"));
        // parts[0] is the drive; parts[1] the top-level entry on it.
        if let Some(top) = parts.get(1) {
            if DRIVE_TREES.contains(&top.as_str()) || (parts.len() == 2 && DRIVE_FILES.contains(&top.as_str())) {
                return system;
            }
            if top == "users" {
                // The Users folder, every profile folder in it, and the
                // template profiles new accounts are copied from.
                let special = parts.get(2).is_some_and(|p| matches!(p.as_str(), "default" | "default user" | "all users"));
                if parts.len() <= 3 || special {
                    return system;
                }
            }
        }
        if env_trees(env).iter().any(|t| within(path, t)) {
            return system;
        }
        if let Some(h) = home {
            if HOME_DIRS.iter().any(|d| same(path, &h.join(d))) {
                return system;
            }
            if HOME_PROTECTED.iter().any(|p| within(path, &h.join(p))) {
                return Err(format!("{shown} holds credentials"));
            }
            // The registry hive and its logs: NTUSER.DAT, ntuser.dat.LOG1, ...
            let at_top = path.parent().is_some_and(|p| same(p, h));
            let hive = path.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with("ntuser."));
            if at_top && hive {
                return Err(format!("{shown} is your registry hive"));
            }
        }
        Ok(())
    }

    /// The same checks on the path as the filesystem resolves it. An admin
    /// share (`\\host\C$`) reached through a mapped drive is refused outright.
    pub fn check_resolved(path: &Path, home: Option<&Path>) -> Result<(), String> {
        let real = fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(Component::Prefix(p)) = real.components().next() {
            match p.kind() {
                Prefix::VerbatimDisk(_) | Prefix::Disk(_) => {}
                Prefix::VerbatimUNC(_, share) | Prefix::UNC(_, share) if !share.to_string_lossy().ends_with('$') => {
                    return Ok(());
                }
                _ => return Err(format!("{} resolves to {}, which the Cleaner does not touch", path.display(), real.display())),
            }
        }
        // `\\?\C:\x` back to `C:\x`, so the checks read the usual spelling.
        let plain = real.to_string_lossy().strip_prefix(r"\\?\").map(PathBuf::from).unwrap_or(real);
        if home.is_some_and(|h| same(&plain, h)) {
            return Err("refusing to remove the home directory".to_string());
        }
        check_protected(&plain, home)
    }

    pub fn move_to_trash(path: &Path) -> Result<(), String> {
        recycle(path)
    }

    /// Sends `path` to the Recycle Bin of its drive. Refused on drives that
    /// have none (network, removable), where the shell would delete it for
    /// good instead without saying so.
    fn recycle(path: &Path) -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
        use windows_sys::Win32::UI::Shell::{
            SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW,
        };
        const DRIVE_FIXED: u32 = 3;

        let drive = match path.components().next() {
            Some(Component::Prefix(p)) => match p.kind() {
                Prefix::Disk(d) => format!("{}:\\", d as char),
                _ => return Err(format!("{} has no Recycle Bin; delete it permanently instead", path.display())),
            },
            _ => return Err(format!("{} is not an absolute path", path.display())),
        };
        let wide_drive: Vec<u16> = std::ffi::OsStr::new(&drive).encode_wide().chain(Some(0)).collect();
        // SAFETY: `wide_drive` is NUL-terminated and outlives the call.
        if unsafe { GetDriveTypeW(wide_drive.as_ptr()) } != DRIVE_FIXED {
            return Err(format!("{drive} has no Recycle Bin; delete it permanently instead"));
        }

        // pFrom is a list: each path NUL-terminated, the list ending in a second NUL.
        let from: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
        let mut op = SHFILEOPSTRUCTW {
            hwnd: std::ptr::null_mut(),
            wFunc: FO_DELETE,
            pFrom: from.as_ptr(),
            pTo: std::ptr::null(),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
            fAnyOperationsAborted: 0,
            hNameMappings: std::ptr::null_mut(),
            lpszProgressTitle: std::ptr::null(),
        };
        // SAFETY: every pointer in `op` is valid or null as the API allows,
        // and `from` outlives the call.
        let code = unsafe { SHFileOperationW(&mut op) };
        if code != 0 || op.fAnyOperationsAborted != 0 {
            return Err(format!("could not move {} to the Recycle Bin (shell error {code:#x})", path.display()));
        }
        if fs::symlink_metadata(path).is_ok() {
            return Err(format!("{} is still there; it may be open in another program", path.display()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("devspro-guard-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn check(path: &Path, roots: &[&Path], excl: &[&Path], home: &Path) -> Result<(), String> {
        check_with_home(path, roots, excl, Some(home))
    }

    #[test]
    fn accepts_a_real_entry_inside_a_root() {
        let home = scratch("ok");
        fs::create_dir_all(home.join(".cache/x")).unwrap();
        assert!(check(&home.join(".cache/x"), &[&home], &[], &home).is_ok());
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn rejects_relative_and_dotdot() {
        let home = scratch("rel");
        assert!(check(Path::new("a/b"), &[&home], &[], &home).is_err());
        assert!(check(&home.join("a/../b"), &[&home], &[], &home).is_err());
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn rejects_root_home_and_the_allowed_root_itself() {
        let home = scratch("home");
        let fs_root = if cfg!(windows) { Path::new("C:\\") } else { Path::new("/") };
        assert!(check(fs_root, &[fs_root], &[], &home).is_err());
        assert!(check(&home, &[&home], &[], &home).is_err());
        let other = scratch("other-root");
        assert!(check(&other, &[&other], &[], &home).is_err());
        // $HOME is refused even when a root above it would contain it.
        assert!(check(&home, &[home.parent().unwrap()], &[], &home).is_err());
        fs::remove_dir_all(&home).unwrap();
        fs::remove_dir_all(&other).unwrap();
    }

    #[test]
    fn rejects_outside_every_root() {
        let home = scratch("outside");
        let elsewhere = scratch("outside-else");
        fs::write(elsewhere.join("f"), "x").unwrap();
        assert!(check(&elsewhere.join("f"), &[&home], &[], &home).is_err());
        // A sibling that merely shares a name prefix is not inside.
        let mut sibling = home.clone().into_os_string();
        sibling.push("-x");
        assert!(check(Path::new(&sibling), &[&home], &[], &home).is_err());
        fs::remove_dir_all(&home).unwrap();
        fs::remove_dir_all(&elsewhere).unwrap();
    }

    #[cfg(not(windows))]
    #[test]
    fn rejects_system_directories() {
        let home = Path::new("/home/someone");
        for p in ["/usr/lib/x", "/etc/passwd", "/var", "/proc/1", "/boot/vmlinuz"] {
            assert!(check(Path::new(p), &[Path::new("/")], &[], home).is_err(), "{p}");
        }
    }

    #[test]
    fn rejects_credential_dirs_under_home() {
        let home = scratch("creds");
        fs::create_dir_all(home.join(".ssh")).unwrap();
        assert!(check(&home.join(".ssh/id_ed25519"), &[&home], &[], &home).is_err());
        fs::remove_dir_all(&home).unwrap();
    }

    #[cfg(not(windows))]
    #[test]
    fn rejects_symlinks_in_the_path_and_as_the_target() {
        let home = scratch("links");
        let outside = scratch("links-outside");
        fs::create_dir_all(outside.join("inner")).unwrap();
        std::os::unix::fs::symlink(&outside, home.join("via")).unwrap();
        assert!(check(&home.join("via"), &[&home], &[], &home).is_err());
        assert!(check(&home.join("via/inner"), &[&home], &[], &home).is_err());
        fs::remove_dir_all(&home).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn rejects_excluded_prefixes() {
        let home = scratch("excl");
        fs::create_dir_all(home.join("keep/deep")).unwrap();
        let keep = home.join("keep");
        assert!(check(&keep.join("deep"), &[&home], &[&keep], &home).is_err());
        assert!(check(&keep, &[&home], &[&keep], &home).is_err());
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn rejects_an_ancestor_of_an_exclusion() {
        let home = scratch("anc");
        fs::create_dir_all(home.join("a/keep")).unwrap();
        assert!(check(&home.join("a"), &[&home], &[&home.join("a/keep")], &home).is_err());
        assert!(check(&home.join("a"), &[&home], &[&home.join("other")], &home).is_ok());
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn non_permanent_removal_goes_to_the_trash() {
        let home = scratch("trash-home");
        fs::create_dir_all(home.join("proj")).unwrap();
        fs::write(home.join("proj/f"), "x").unwrap();
        // A scratch HOME, so the real trash is never touched.
        remove_path_with(&home.join("proj"), false, |p| {
            crate::modules::file_manager::trash_manually(p, &home)
        })
        .unwrap();
        assert!(!home.join("proj").exists());
        assert!(home.join(".local/share/Trash/files/proj/f").exists());
        assert!(home.join(".local/share/Trash/info/proj.trashinfo").exists());
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn rejects_a_missing_path() {
        let home = scratch("missing");
        assert!(check(&home.join("nope"), &[&home], &[], &home).is_err());
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn permanent_removal_deletes_files_and_trees() {
        let d = scratch("rm");
        fs::create_dir_all(d.join("t/u")).unwrap();
        fs::write(d.join("t/u/f"), "x").unwrap();
        fs::write(d.join("lone"), "x").unwrap();
        remove_path(&d.join("t"), true).unwrap();
        remove_path(&d.join("lone"), true).unwrap();
        assert!(!d.join("t").exists() && !d.join("lone").exists());
        assert!(remove_path(&d.join("gone"), true).is_err());
        fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(not(windows))]
    #[test]
    fn permanent_removal_of_a_symlink_leaves_its_target() {
        let d = scratch("rm-link");
        let target = scratch("rm-link-target");
        fs::write(target.join("keep"), "x").unwrap();
        std::os::unix::fs::symlink(&target, d.join("l")).unwrap();
        remove_path(&d.join("l"), true).unwrap();
        assert!(!d.join("l").exists());
        assert!(target.join("keep").exists());
        fs::remove_dir_all(&d).unwrap();
        fs::remove_dir_all(&target).unwrap();
    }

    #[cfg(windows)]
    mod windows {
        use super::*;
        use std::ffi::OsString;

        /// A junction from `link` to `target`; needs no admin rights, unlike a
        /// symlink.
        fn junction(link: &Path, target: &Path) {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap();
            assert!(status.status.success(), "mklink /J failed: {status:?}");
        }

        fn default_env(var: &str) -> Option<OsString> {
            let v = match var {
                "SystemRoot" | "windir" => r"C:\WINDOWS",
                "ProgramFiles" | "ProgramW6432" => r"C:\Program Files",
                "ProgramFiles(x86)" => r"C:\Program Files (x86)",
                "ProgramData" => r"C:\ProgramData",
                _ => return None,
            };
            Some(v.into())
        }

        fn protected(path: &str) -> bool {
            let home = Path::new(r"C:\Users\someone");
            platform::check_protected_with(Path::new(path), Some(home), &default_env).is_err()
        }

        #[test]
        fn rejects_windows_system_folders_in_any_spelling() {
            for p in [
                r"C:\Windows",
                r"C:\Windows\System32\drivers\etc\hosts",
                r"c:\windows\temp\x",
                r"C:/Windows/System32",
                r"C:\Program Files\App\app.exe",
                r"C:\Program Files (x86)\App",
                r"C:\ProgramData\Microsoft\x",
                r"C:\$Recycle.Bin\S-1-5-21\x",
                r"D:\$RECYCLE.BIN\x",
                r"D:\System Volume Information\x",
                r"C:\Recovery\x",
                r"C:\pagefile.sys",
                r"C:\hiberfil.sys",
                r"E:\swapfile.sys",
                r"C:\Users",
                r"C:\Users\someone-else",
                r"C:\Users\Default\NTUSER.DAT",
                r"C:\Users\All Users\x",
                r"C:\Documents and Settings\x",
            ] {
                assert!(protected(p), "{p} should be protected");
            }
        }

        #[test]
        fn honours_a_non_default_system_root_from_the_environment() {
            let env = |v: &str| (v == "SystemRoot").then(|| OsString::from(r"D:\WinOS"));
            assert!(platform::check_protected_with(Path::new(r"d:\winos\system32\x"), None, &env).is_err());
            assert!(platform::check_protected_with(Path::new(r"D:\Data\x"), None, &env).is_ok());
        }

        #[test]
        fn rejects_profile_credentials_and_skeleton_folders() {
            for p in [
                r"C:\Users\someone\.ssh\id_ed25519",
                r"c:\users\SOMEONE\.SSH\config",
                r"C:\Users\someone\.gnupg\x",
                r"C:\Users\someone\AppData",
                r"C:\Users\someone\AppData\Local",
                r"C:\Users\someone\AppData\Roaming",
                r"C:\Users\someone\AppData\LocalLow",
                r"C:\Users\someone\AppData\Roaming\Microsoft\Protect\S-1-5\key",
                r"C:\Users\someone\AppData\Roaming\Microsoft\Credentials\x",
                r"C:\Users\someone\AppData\Local\Microsoft\Vault\x",
                r"C:\Users\someone\AppData\Roaming\Microsoft\Crypto\RSA\x",
                r"C:\Users\someone\AppData\Roaming\Microsoft\SystemCertificates\My",
                r"C:\Users\someone\NTUSER.DAT",
                r"C:\Users\someone\ntuser.dat.LOG1",
            ] {
                assert!(protected(p), "{p} should be protected");
            }
        }

        #[test]
        fn allows_ordinary_user_files() {
            for p in [
                r"C:\Users\someone\Downloads\big.iso",
                r"C:\Users\someone\AppData\Local\Temp\x",
                r"C:\Users\someone\code\app\node_modules",
                r"C:\Users\someone\Documents\ntuser.dat",
                r"D:\Projects\app\target",
            ] {
                assert!(!protected(p), "{p} should be allowed");
            }
        }

        #[test]
        fn rejects_unc_verbatim_device_and_stream_paths() {
            for p in [
                r"\\?\C:\Windows\System32",
                r"\\?\C:\Users\someone\Downloads\x",
                r"\\localhost\C$\Windows\x",
                r"\\server\share\x",
                r"\\.\PhysicalDrive0",
                r"C:\Users\someone\Downloads\file.txt:hidden",
            ] {
                assert!(protected(p), "{p} should be refused");
            }
        }

        #[test]
        fn roots_and_exclusions_match_case_insensitively() {
            let home = scratch("case");
            fs::create_dir_all(home.join("keep/x")).unwrap();
            let upper = PathBuf::from(home.to_string_lossy().to_uppercase());
            // A root typed in another case still contains the path...
            assert!(check(&home.join("keep/x"), &[&upper], &[], &home).is_ok());
            // ...and an exclusion typed in another case still excludes it.
            assert!(check(&home.join("keep/x"), &[&home], &[&upper.join("KEEP")], &home).is_err());
            fs::remove_dir_all(&home).unwrap();
        }

        #[test]
        fn rejects_junctions_in_the_path_and_as_the_target() {
            let home = scratch("junction");
            let outside = scratch("junction-outside");
            fs::create_dir_all(outside.join("inner")).unwrap();
            junction(&home.join("via"), &outside);
            let err = check(&home.join("via"), &[&home], &[], &home).unwrap_err();
            assert!(err.contains("symbolic link"), "{err}");
            assert!(check(&home.join("via/inner"), &[&home], &[], &home).is_err());
            // Deleting the junction permanently leaves what it points at.
            remove_path(&home.join("via"), true).unwrap();
            assert!(outside.join("inner").exists());
            fs::remove_dir_all(&home).unwrap();
            fs::remove_dir_all(&outside).unwrap();
        }

        #[test]
        fn a_short_name_cannot_reach_a_protected_folder() {
            // Only meaningful where 8.3 names exist on the system drive.
            let short = Path::new(r"C:\PROGRA~1");
            if !short.exists() {
                return;
            }
            let err = check(short, &[Path::new(r"C:\")], &[], Path::new(r"C:\Users\nobody")).unwrap_err();
            assert!(err.contains("system folder"), "{err}");
        }

        #[test]
        fn the_real_environment_protects_the_real_system_root() {
            let windir = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap();
            let err = check_removable(&windir.join("System32"), &[windir.parent().unwrap()], &[] as &[&Path]).unwrap_err();
            assert!(err.contains("system folder"), "{err}");
        }
    }
}
