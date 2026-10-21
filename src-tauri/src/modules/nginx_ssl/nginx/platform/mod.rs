//! Where nginx lives on this OS, and what it takes to make it serve DevsPro's site files.
//!
//! | | |
//! | --- | --- |
//! | `find_binary()` | Path to the nginx executable, or `None`. |
//! | `resolve_config_path()` | Turn what `nginx -V` reports into a path that can actually be opened. |
//! | `prefix_for()` | nginx's `-p`, where this OS needs one. |
//! | `wire_in_sites_ops()` | Every elevated operation needed to make nginx include our site files. |
//!
//! The last two exist because the Windows nginx distribution differs from every Linux
//! package in ways that aren't just a matter of paths: it is built with an empty
//! `--prefix=` and a *relative* `--conf-path=`, and it ships no `conf.d` directory nor an
//! include of one. See `windows.rs`.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

/// The marker pair wrapping anything DevsPro adds to a config file it doesn't own, so it
/// can be found and replaced — or removed again — without disturbing a line around it.
pub const MARKER_BEGIN: &str = "# >>> devsuite (do not edit this block manually) >>>";
pub const MARKER_END: &str = "# <<< devsuite <<<";

/// The last-resort "ask the system where it is" lookup. Shared with every other tool this
/// app looks for — see `modules::tools`, which is also what keeps the Windows side from
/// picking a `where` match that Windows cannot actually execute.
pub(crate) fn which_nginx() -> Option<String> {
    crate::modules::tools::find("nginx")
}
