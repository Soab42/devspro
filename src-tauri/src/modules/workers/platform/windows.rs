//! Windows. Neither half of the Unix approach exists here: there is no `sh`, and no
//! signal a GUI process can send a console program to ask it to wind down.

use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};

/// Without this, every started worker flashes up a console window of its own: a GUI
/// process has no console for the child to inherit, so Windows creates one. The
/// worker's output is redirected into its log file either way, so that window would show
/// nothing and only steal focus.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn shell_command(command_line: &str) -> Command {
    let mut command = Command::new("cmd");
    // `/C` followed by the command line *verbatim*. Rust quotes each `arg()` by the
    // rules `CommandLineToArgvW` parses, which `cmd.exe` does not follow — so a worker
    // command containing quotes, `&&` or a pipe would arrive mangled or split apart.
    // `raw_arg` is the documented way to hand a command processor a line untouched.
    command
        .arg("/C")
        .raw_arg(command_line)
        .creation_flags(CREATE_NO_WINDOW);
    command
}

/// Terminates the worker and everything it started, with `taskkill /T /F`.
///
/// **`/T` (the whole tree) is not optional here, unlike on Unix.** `sh -c <one command>`
/// execs that command in place, so there the pid held by the caller *is* the worker.
/// `cmd /C` doesn't: it stays alive as the parent of the real command. Signalling only the
/// pid we hold would reap the wrapper and leave the actual queue worker or dev server
/// running, unreachable, while the app reported it as stopped.
///
/// **There is no graceful option to try first, and reaching for one makes things worse.**
/// The polite form of `taskkill` asks by posting `WM_CLOSE`, which only a process running a
/// message loop ever receives — a console tool like `php artisan queue:work` has none. The
/// documented alternative, `GenerateConsoleCtrlEvent`, requires the caller to share the
/// child's console, and a GUI application has none to share. Attempting the polite form
/// first is actively harmful: it walks the tree top-down and can close `cmd` while its
/// children survive, orphaning the very process this is meant to stop, with nothing left
/// holding its pid. So this goes straight to a forced stop.
///
/// That is a real functional difference from Unix, not just a different mechanism: a
/// Windows worker does not get to finish the job it is in the middle of, where SIGTERM
/// gives a Unix one the chance.
pub fn request_stop(child: &Child) -> Result<(), String> {
    // The exit code is deliberately ignored. `/T` reports trouble with *any* member of the
    // tree, and a short-lived grandchild that exits between being listed and being
    // terminated is enough to make taskkill unhappy while the process we actually hold is
    // already gone — which is exactly what the workers smoke test hit. Whether the stop
    // worked is settled by the caller waiting on the child, so the only failure worth
    // reporting from here is not being able to run taskkill at all.
    Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        // taskkill is a console program; without this it flashes a window of its own every
        // time a worker is stopped.
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("failed to run taskkill: {e}"))?;
    Ok(())
}

/// `request_stop` is already a forced tree kill, so there is nothing further to escalate to.
pub fn force_stop(_child: &Child) -> Result<(), String> {
    Ok(())
}
