use std::path::Path;

/// Planned: launch `ldm-helper.exe` (built with an embedded `requireAdministrator`
/// manifest) via `ShellExecuteW` with verb `"runas"` for the native UAC prompt. Not
/// implemented yet — no Windows machine available to verify this path. Fill in and
/// test on Windows before the nginx/SSL module ships an elevated op there.
pub fn launch_helper(_helper_path: &Path, _manifest_path: &Path) -> Result<(), String> {
    Err("Windows elevation is not implemented yet".to_string())
}
