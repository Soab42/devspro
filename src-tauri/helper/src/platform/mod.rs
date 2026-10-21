//! The operations whose *implementation* is OS-specific, split per target. Everything
//! else in this binary — the allowed-prefix check, the marked-block editing, writing the
//! result file — is plain text and path work that behaves the same everywhere and stays
//! in `main.rs`.
//!
//! | | |
//! | --- | --- |
//! | `reload_nginx()` | Make a running nginx pick up the new config. |
//! | `install_root_ca()` | Put the root CA where browsers will trust it. |
//! | `install_package()` | Install named packages with the manager the caller detected. |
//! | `fix_laravel_permissions()` | Give the PHP process write access to `storage/` and `bootstrap/cache/`. |
//!
//! Each returns an `OpResult` rather than a `Result`, because a manifest records an
//! outcome per operation; and each is honest about doing nothing when there is nothing to
//! do on that OS, rather than reporting a success it didn't perform.
//!
//! `unix.rs` is the code this binary ran before the split, moved unchanged down to its
//! result messages — those messages reach the user, so rewording them would be a
//! behaviour change on the platform that already worked.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
