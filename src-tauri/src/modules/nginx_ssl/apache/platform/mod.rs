//! Where Apache lives on this OS, and what it takes to make it serve DevsPro's sites —
//! the same split, for the same reasons, as `nginx::platform`.
//!
//! | | |
//! | --- | --- |
//! | `find_binary()` | What to run Apache through: `apachectl`/`apache2ctl` on Unix, `httpd.exe` on Windows. |
//! | `server_root_arg()` | The `-d` every invocation needs, where this OS needs one. |
//! | `wire_in_ops()` | Every elevated operation needed to make Apache include DevsPro's config. |
//! | `reload_ops()` / `after_apply()` | Making a running Apache pick the change up — in the manifest on Unix, unelevated afterwards on Windows. |
//! | `fcgi_handler()` / `script_filename_fix()` | How a Laravel site reaches PHP's FastCGI listener. |
//!
//! Unix runs everything through the control script because Debian's `apache2` binary
//! cannot even parse its config without the environment `apache2ctl` sets up. Windows
//! differs in the other direction: its builds carry a compiled-in root that is never where
//! Apache was unpacked, and `httpd -k restart` only reaches an Apache installed as a
//! service — not the plain process Laragon and XAMPP run.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
