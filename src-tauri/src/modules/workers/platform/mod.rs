//! The two things supervising a worker needs from the OS, split per target the same
//! way `elevate::platform` and `system::platform` are:
//!
//! | | |
//! | --- | --- |
//! | `shell_command()` | A `Command` that runs one user-authored command line through this OS's shell. |
//! | `request_stop()` | Ask a running child to shut down. `Err` only if the request couldn't be delivered at all — whether it *worked* is decided by the caller, which waits and then checks. |
//!
//! Unix covers Linux and macOS with one implementation because both want exactly the
//! same thing (`sh -c`, then SIGTERM), which is also what Linux already did before this
//! split — the file is that code moved, unchanged.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
