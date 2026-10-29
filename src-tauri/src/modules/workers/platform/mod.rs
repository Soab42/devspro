//! The two things supervising a worker needs from the OS, split per target the same
//! way `elevate::platform` and `system::platform` are:
//!
//! | | |
//! | --- | --- |
//! | `shell_command()` | A `Command` that runs one user-authored command line through this OS's shell. |
//! | `request_stop()` | Ask a running child, and everything it started, to shut down. `Err` only if the request couldn't be delivered at all — whether it *worked* is decided by the caller, which waits and then checks. |
//! | `force_stop()` | Kill it outright, for when `request_stop` was ignored. A no-op on Windows, where `request_stop` is already forced. |
//!
//! Unix covers Linux and macOS with one implementation because both want exactly the
//! same thing (`sh -c` in a process group of its own, SIGTERM and then SIGKILL to the group).

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
