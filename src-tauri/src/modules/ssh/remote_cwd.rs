//! Where a terminal's remote shell is, read in the background.
//!
//! The follow-along file browser needs the shell's folder. Typing a hook into
//! the shell to have it announce its folder works anywhere, but the user sees
//! a strange command land in their terminal. So each ssh terminal is started
//! with its session id in an environment variable (`TERM_MARKER_ENV`, sent
//! with `SendEnv`), and the folder is read over the browser's own SSH
//! connection instead: find the processes carrying that id under `/proc`, and
//! read the shell's `cwd`. Nothing is typed.
//!
//! It needs a Linux server whose sshd accepts `LC_*` variables (Debian and
//! Ubuntu do by default). Anywhere else nothing is found, and the terminal
//! falls back to the hook.

use super::SshState;
use tauri::State;

/// `LC_` so that sshd's usual `AcceptEnv LANG LC_*` lets it through.
pub const TERM_MARKER_ENV: &str = "LC_DEVSPRO_TERM";

const SHELLS: [&str; 7] = ["bash", "zsh", "sh", "dash", "fish", "ksh", "ash"];

/// Lists `pid<TAB>comm<TAB>cwd` for every process that carries the marker.
/// Only the user's own processes can be read, which is all that can match.
fn script(marker: &str) -> String {
    format!(
        r#"m='{TERM_MARKER_ENV}={marker}'
for p in /proc/[0-9]*; do
  [ -r "$p/environ" ] || continue
  if tr '\000' '\n' < "$p/environ" 2>/dev/null | grep -qxF "$m"; then
    printf '%s\t%s\t%s\n' "${{p#/proc/}}" "$(cat "$p/comm" 2>/dev/null)" "$(readlink "$p/cwd" 2>/dev/null)"
  fi
done"#
    )
}

/// The folder to show: the newest shell's (a nested `bash` is where the user
/// is typing), else the newest process's.
fn pick_cwd(output: &str) -> Option<String> {
    let rows: Vec<(u32, &str, &str)> = output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let pid = parts.next()?.trim().parse().ok()?;
            let comm = parts.next()?.trim();
            let cwd = parts.next()?.trim();
            cwd.starts_with('/').then_some((pid, comm, cwd))
        })
        .collect();
    let shells = rows.iter().filter(|(_, comm, _)| SHELLS.contains(&comm.trim_start_matches('-')));
    shells
        .max_by_key(|(pid, _, _)| *pid)
        .or_else(|| rows.iter().max_by_key(|(pid, _, _)| *pid))
        .map(|(_, _, cwd)| cwd.to_string())
}

/// A terminal session id: what `pty_open` accepts, so it is safe in a script.
fn valid_marker(marker: &str) -> bool {
    !marker.is_empty() && marker.len() <= 64 && marker.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The folder of the shell behind terminal `marker`, read over the SSH
/// connection `session`; None when no such shell can be found there.
#[tauri::command(async)]
pub fn term_remote_cwd(state: State<SshState>, session: String, marker: String) -> Result<Option<String>, String> {
    if !valid_marker(&marker) {
        return Err("invalid terminal session id".to_string());
    }
    let output = super::sftp::exec_on_session(&state, &session, &script(&marker))?;
    Ok(pick_cwd(&output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_newest_shell_over_the_programs_it_runs() {
        let out = "812\tbash\t/root\n903\tvim\t/root/app\n950\tbash\t/root/app/src\n";
        assert_eq!(pick_cwd(out).as_deref(), Some("/root/app/src"));
    }

    #[test]
    fn falls_back_to_the_newest_process_and_skips_unreadable_rows() {
        assert_eq!(pick_cwd("10\tnode\t/srv\n12\tpython3\t/srv/api\n7\tbash\t\n").as_deref(), Some("/srv/api"));
        assert_eq!(pick_cwd(""), None);
        assert_eq!(pick_cwd("garbage\n"), None);
    }

    #[test]
    fn a_login_shell_counts_as_a_shell() {
        assert_eq!(pick_cwd("5\t-bash\t/home/me\n9\tsleep\t/tmp\n").as_deref(), Some("/home/me"));
    }

    #[test]
    fn only_session_ids_go_into_the_script() {
        assert!(valid_marker("5f0c2a1e-9b7d-4c1a-8e2f-0a1b2c3d4e5f"));
        assert!(!valid_marker("x'; rm -rf ~; '"));
        assert!(!valid_marker(""));
        assert!(script("abc-1").contains("m='LC_DEVSPRO_TERM=abc-1'"));
    }
}
