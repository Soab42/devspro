//! The Unix file-metadata calls the Cleaner, Files and AI modules are written
//! against, available on every platform.
//!
//! On Unix this is a plain re-export of `std::os::unix::fs`, so nothing changes
//! there. On Windows the same method names answer with the nearest honest
//! equivalent, chosen so the Unix-shaped logic built on them stays correct
//! rather than merely compiling:
//!
//! - `dev()` is 0 for everything. Scans use it to stay on one filesystem; a
//!   Windows walk never crosses into another drive anyway, since drives are
//!   separate roots rather than mount points inside one tree.
//! - `nlink()` is 1 and `ino()` 0. Every hard-link dedupe is gated on
//!   `nlink() > 1`, so with these no file is ever mistaken for a duplicate of
//!   another — at worst a real hard link is counted twice.
//! - `blocks()` is the length in 512-byte units, rounded up: the allocated size
//!   std can see without opening the file.
//! - `mode()` is synthesised from the read-only flag and file type; there are no
//!   permission bits to report.

#[cfg(unix)]
pub use std::os::unix::fs::{MetadataExt, PermissionsExt};

#[cfg(windows)]
pub use self::windows::{MetadataExt, PermissionsExt};

/// Whether `path` is a file this OS would run: any execute bit on Unix, an
/// executable extension on Windows (where there are no execute bits).
pub fn is_executable(path: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| matches!(e.to_ascii_lowercase().as_str(), "exe" | "cmd" | "bat" | "com"))
            .unwrap_or(false)
    }
}

#[cfg(windows)]
mod windows {
    use std::fs::{Metadata, Permissions};
    use std::time::{SystemTime, UNIX_EPOCH};

    const S_IFDIR: u32 = 0o040000;
    const S_IFREG: u32 = 0o100000;
    const S_IFLNK: u32 = 0o120000;

    fn unix_seconds(time: std::io::Result<SystemTime>) -> i64 {
        time.ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn permission_bits(readonly: bool) -> u32 {
        if readonly {
            0o444
        } else {
            0o644
        }
    }

    pub trait MetadataExt {
        fn dev(&self) -> u64;
        fn ino(&self) -> u64;
        fn nlink(&self) -> u64;
        fn blocks(&self) -> u64;
        fn mtime(&self) -> i64;
        fn atime(&self) -> i64;
        fn mode(&self) -> u32;
        fn uid(&self) -> u32;
        fn gid(&self) -> u32;
    }

    impl MetadataExt for Metadata {
        fn dev(&self) -> u64 {
            0
        }
        fn ino(&self) -> u64 {
            0
        }
        fn nlink(&self) -> u64 {
            1
        }
        fn blocks(&self) -> u64 {
            self.len().div_ceil(512)
        }
        fn mtime(&self) -> i64 {
            unix_seconds(self.modified())
        }
        fn atime(&self) -> i64 {
            unix_seconds(self.accessed())
        }
        fn mode(&self) -> u32 {
            let kind = if self.is_symlink() {
                S_IFLNK
            } else if self.is_dir() {
                S_IFDIR
            } else {
                S_IFREG
            };
            let bits = if self.is_dir() { 0o755 } else { permission_bits(self.permissions().readonly()) };
            kind | bits
        }
        fn uid(&self) -> u32 {
            0
        }
        fn gid(&self) -> u32 {
            0
        }
    }

    pub trait PermissionsExt {
        fn mode(&self) -> u32;
    }

    impl PermissionsExt for Permissions {
        fn mode(&self) -> u32 {
            permission_bits(self.readonly())
        }
    }
}
