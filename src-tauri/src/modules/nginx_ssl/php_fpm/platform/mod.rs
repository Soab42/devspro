//! The PHP FastCGI backend a Laravel site talks to, per OS.
//!
//! | | |
//! | --- | --- |
//! | `list_available_versions()` | Every PHP a site could be pointed at, newest first. |
//! | `detect_default()` | What "system default" resolves to, or `None`. |
//! | `target_for_version()` | The `fastcgi_pass` value for one pinned version. |
//! | `install_hint()` | What to tell a user who has no usable PHP. |
//! | `missing_version_hint()` | Why a pinned version can't be used right now. |
//!
//! The contract deliberately returns whole `fastcgi_pass` values rather than socket paths:
//! Unix hands nginx `unix:/run/php/php8.3-fpm.sock` and Windows hands it
//! `127.0.0.1:9803`, and the renderer shouldn't have to know which world it's in.
//!
//! There is one semantic difference worth knowing about, documented at length in
//! `windows.rs`: on Unix, "available" means a pool is *running*, because starting one is
//! the system's job. On Windows nothing starts `php-cgi.exe`, so "available" means
//! installed, and `target_for_version` starts the listener itself.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
