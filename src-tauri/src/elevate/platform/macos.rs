use std::path::Path;
use std::process::Command;

/// `osascript ... do shell script ... with administrator privileges` is macOS's
/// native equivalent of `pkexec`: it pops the standard admin-password dialog and,
/// once approved, runs the given command as root. The helper/manifest paths get
/// embedded in a shell command string, which itself gets embedded in an AppleScript
/// string literal — two nested layers, each with its own escaping rules. Skipping
/// either (shell single-quoting for the paths, then backslash/double-quote escaping
/// for the AppleScript literal) would break on a path containing a space or quote,
/// or worse, open a shell-injection hole. Cancelling or repeatedly failing the
/// password prompt makes `do shell script` throw inside AppleScript, which surfaces
/// here as a non-zero `osascript` exit with the error text on stderr.
pub fn launch_helper(helper_path: &Path, manifest_path: &Path) -> Result<(), String> {
    let helper = helper_path
        .to_str()
        .ok_or_else(|| "helper path is not valid UTF-8".to_string())?;
    let manifest = manifest_path
        .to_str()
        .ok_or_else(|| "manifest path is not valid UTF-8".to_string())?;

    let shell_command = format!("{} {}", shell_quote(helper), shell_quote(manifest));
    let script = format!(
        "do shell script \"{}\" with administrator privileges",
        escape_applescript_string(&shell_command)
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("failed to launch osascript: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "elevated helper exited with status {} (denied, or osascript unavailable): {}",
            output.status,
            stderr.trim()
        ))
    }
}

/// Wraps `s` in single quotes so it is safe as one literal shell word, escaping any
/// embedded single quotes with the standard close-quote/escaped-quote/reopen-quote
/// trick (`'...'"'"'...'`, spelled here as `'\''` since we're not inside a real
/// shell — AppleScript hands this string straight to `/bin/sh -c`).
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Escapes `s` for use inside an AppleScript double-quoted string literal.
/// Backslashes must be doubled *first*, then embedded double quotes escaped —
/// otherwise the backslashes that `shell_quote` may have introduced (for paths
/// containing a single quote) would be corrupted before AppleScript reconstructs
/// the literal shell command text.
fn escape_applescript_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_wraps_plain_path() {
        assert_eq!(shell_quote("/usr/local/bin/helper"), "'/usr/local/bin/helper'");
    }

    #[test]
    fn shell_quote_escapes_embedded_single_quote() {
        // Shell-level correctness: concatenating the three single-quoted/escaped
        // segments must reconstruct the original text under shell quoting rules.
        assert_eq!(shell_quote("/tmp/John's App/helper"), "'/tmp/John'\\''s App/helper'");
    }

    #[test]
    fn escape_applescript_string_doubles_backslashes_before_quotes() {
        assert_eq!(escape_applescript_string(r"a\b"), r"a\\b");
        assert_eq!(escape_applescript_string(r#"a"b"#), r#"a\"b"#);
        // Order matters: a backslash introduced by shell_quote must survive intact.
        assert_eq!(
            escape_applescript_string("'John'\\''s'"),
            "'John'\\\\''s'"
        );
    }

    #[test]
    fn full_pipeline_roundtrips_path_with_space_and_quote() {
        let helper = "/Applications/John's App.app/Contents/MacOS/helper";
        let manifest = "/tmp/manifest with space.json";
        let shell_command = format!("{} {}", shell_quote(helper), shell_quote(manifest));
        let script = escape_applescript_string(&shell_command);
        // The AppleScript literal must not contain an unescaped double quote, since
        // that would terminate the string early in `do shell script "...".
        assert!(!script.contains('"'));
    }
}
