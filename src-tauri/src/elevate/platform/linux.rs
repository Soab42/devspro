use std::path::Path;
use std::process::Command;

/// Primary Linux path: PolicyKit's `pkexec` gives a native GUI auth dialog on
/// GNOME/KDE without needing a terminal. A shipped `.policy` file (declaring this
/// exact helper path) is required for a packaged build — deferred until the helper
/// does something real, since an unreviewed policy file is pointless right now.
pub fn launch_helper(helper_path: &Path, manifest_path: &Path) -> Result<(), String> {
    let status = Command::new("pkexec")
        .arg(helper_path)
        .arg(manifest_path)
        .status()
        .map_err(|e| format!("failed to launch pkexec: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "elevated helper exited with status {status} (denied, or pkexec unavailable)"
        ))
    }
}
