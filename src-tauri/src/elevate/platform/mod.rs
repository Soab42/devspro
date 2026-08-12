// Every per-OS launcher below implements the same contract: run
// `helper_path manifest_path` elevated via that OS's native prompt, and block until
// the helper process exits (returning Err on a non-zero/denied exit).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::launch_helper;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::launch_helper;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::launch_helper;
