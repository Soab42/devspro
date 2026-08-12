use std::path::Path;

/// Planned: `osascript -e 'do shell script "<helper> <manifest>" with administrator
/// privileges'` for the native admin password dialog. Not implemented yet — this
/// machine is Linux, so the macOS path has no way to be verified here. Fill in and
/// test on macOS before the nginx/SSL module ships an elevated op there.
pub fn launch_helper(_helper_path: &Path, _manifest_path: &Path) -> Result<(), String> {
    Err("macOS elevation is not implemented yet".to_string())
}
