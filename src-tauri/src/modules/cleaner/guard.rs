//! The one gate every removal passes. Scans decide what to offer; this decides
//! whether a path may be removed at all, so a bad scan, a stale id or a path
//! built from user input cannot reach outside the roots the user allowed.

use super::super::file_manager;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Never removable, and everything below them is not either.
const SYSTEM_TREES: &[&str] = &[
    "/bin", "/boot", "/dev", "/etc", "/lib", "/lib32", "/lib64", "/proc", "/sbin", "/sys", "/usr",
];

/// Never removable themselves; what is inside is another matter.
const SYSTEM_DIRS: &[&str] = &[
    "/", "/home", "/media", "/mnt", "/opt", "/root", "/run", "/snap", "/srv", "/tmp", "/var",
];

/// Under $HOME: credentials, never a cleaning target.
const HOME_PROTECTED: &[&str] = &[".ssh", ".gnupg"];

/// Ok when `path` may be removed: absolute, free of `..`, strictly inside one
/// of `allowed_roots`, reached through no symlink (the path itself included),
/// not $HOME, `/` or a system directory, and not under an excluded prefix.
pub fn check_removable<R, E>(path: &Path, allowed_roots: &[R], exclusions: &[E]) -> Result<(), String>
where
    R: AsRef<Path>,
    E: AsRef<Path>,
{
    let home = std::env::var_os("HOME").map(PathBuf::from);
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
        return Err("refusing to remove /".to_string());
    }
    if home.is_some_and(|h| path == h) {
        return Err("refusing to remove the home directory".to_string());
    }
    if SYSTEM_DIRS.iter().any(|d| path == Path::new(d)) || SYSTEM_TREES.iter().any(|d| path.starts_with(d)) {
        return Err(format!("{shown} is a system directory"));
    }
    if let Some(h) = home {
        if HOME_PROTECTED.iter().any(|p| path.starts_with(h.join(p))) {
            return Err(format!("{shown} holds credentials"));
        }
    }
    // An ancestor of an exclusion would remove the excluded thing with it.
    if exclusions.iter().any(|e| path.starts_with(e.as_ref()) || e.as_ref().starts_with(path)) {
        return Err(format!("{shown} is excluded in the Cleaner settings"));
    }

    let root = allowed_roots
        .iter()
        .map(|r| r.as_ref())
        .filter(|r| r.is_absolute() && path.starts_with(r) && path != *r)
        .max_by_key(|r| r.components().count())
        .ok_or_else(|| format!("{shown} is not inside a folder the Cleaner may clean"))?;

    // The root itself is the user's choice (it may live under a symlinked
    // /home); everything from it down to the target must be a real entry.
    let mut cursor = root.to_path_buf();
    for component in path.strip_prefix(root).unwrap_or(path).components() {
        cursor.push(component);
        let meta = fs::symlink_metadata(&cursor).map_err(|e| format!("{}: {e}", cursor.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!("{} is a symbolic link", cursor.display()));
        }
    }
    Ok(())
}

/// Removes `path`. `permanent` deletes outright without following symlinks;
/// otherwise it goes to the trash. Call `check_removable` first.
pub fn remove_path(path: &Path, permanent: bool) -> Result<(), String> {
    remove_path_with(path, permanent, file_manager::move_to_trash)
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
    let result = if meta.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
    result.map_err(|e| format!("could not delete {}: {e}", path.display()))
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
        assert!(check(Path::new("/"), &[Path::new("/")], &[], &home).is_err());
        assert!(check(&home, &[&home], &[], &home).is_err());
        let other = scratch("other-root");
        assert!(check(&other, &[&other], &[], &home).is_err());
        // $HOME is refused even when a root above it would contain it.
        assert!(check(&home, &[Path::new("/tmp")], &[], &home).is_err());
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
        remove_path_with(&home.join("proj"), false, |p| file_manager::trash_manually(p, &home)).unwrap();
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
}
