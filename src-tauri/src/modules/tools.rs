//! Finding the command-line tools this app drives — docker, composer, cloudflared and the
//! rest — on whichever OS it is running on.
//!
//! Every module here used to carry its own private copy of the same four-line `which`
//! helper. Windows has no `which`, so all of them reported every tool missing no matter
//! what was installed, and the Docker, Databases, Mail Catcher, Tunnel and Apps pages were
//! all dead for that one reason.
//!
//! Windows' `where` needs more care than being a spelling of `which`:
//!
//! - **It prints every match, not one.** `where docker` lists both
//!   `…\resources\bin\docker` and `…\resources\bin\docker.exe`.
//! - **The first match is usually the one that cannot be run.** That extensionless
//!   `docker` is a shell script for Git Bash; `CreateProcess`, which is what
//!   `std::process::Command` calls, will not execute it. Taking the first line — the
//!   obvious translation of what `which` returns — hands back a path that fails at the
//!   moment it is used, which is worse than reporting the tool missing.
//!
//! So the Windows lookup keeps the first match whose extension is one Windows will
//! actually execute, per `PATHEXT`.

/// The full path to `tool`, or `None` if it isn't installed.
pub fn find(tool: &str) -> Option<String> {
    platform::find(tool)
}

/// Whether `tool` can be run at all. This is what a detection check wants; `find` is for
/// callers that need to show or store the location.
pub fn is_available(tool: &str) -> bool {
    find(tool).is_some()
}

/// What to hand `Command::new` in order to run `tool`.
///
/// Not the same thing as `find` on Windows, and deliberately so. Several of these tools
/// are `.bat` or `.cmd` shims there — `composer.bat`, `npm.cmd` — which only run when
/// named with their extension, so the resolved path is the only thing that works. On Unix
/// the bare name is returned, which is exactly what every call site passed before this
/// module existed: `which` already proved it resolves, and re-resolving it here would
/// change how a command that has always worked gets invoked.
pub fn program(tool: &str) -> Option<String> {
    platform::program(tool)
}

/// `Command::new(program)`, except that on Windows the child never opens a console window.
///
/// The release build is a GUI-subsystem app there, so it has no console of its own, and
/// Windows gives every console program it starts a fresh one: a black window that flashes
/// up for each `docker ps` a page polls, or sits on screen for the whole life of a tunnel.
/// `CREATE_NO_WINDOW` suppresses only that window — stdout and stderr are captured exactly
/// as before, and a GUI program (an editor, a browser) still shows its own window. On Unix
/// this is plain `Command::new`.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut command = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

#[cfg(unix)]
mod platform {
    use std::process::Command;

    pub fn find(tool: &str) -> Option<String> {
        let output = Command::new("which").arg(tool).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (!path.is_empty()).then_some(path)
    }

    pub fn program(tool: &str) -> Option<String> {
        find(tool).map(|_| tool.to_string())
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::Command;

    /// `where` is a console program; without this, every detection check flashes a window.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// What `PATHEXT` holds on a stock Windows, used when the variable is missing.
    const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

    pub fn find(tool: &str) -> Option<String> {
        let output = Command::new("where")
            .arg(tool)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let listing = String::from_utf8_lossy(&output.stdout);
        first_executable(&listing, &pathext())
    }

    pub fn program(tool: &str) -> Option<String> {
        find(tool)
    }

    fn pathext() -> Vec<String> {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| DEFAULT_PATHEXT.to_string())
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| ext.trim().to_ascii_uppercase())
            .collect()
    }

    /// The first line naming a file Windows will actually execute.
    fn first_executable(listing: &str, pathext: &[String]) -> Option<String> {
        listing
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .find(|line| {
                Path::new(line)
                    .extension()
                    .map(|ext| {
                        let ext = format!(".{}", ext.to_string_lossy().to_ascii_uppercase());
                        pathext.iter().any(|known| *known == ext)
                    })
                    .unwrap_or(false)
            })
            .map(str::to_string)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn pathext() -> Vec<String> {
            DEFAULT_PATHEXT.split(';').map(str::to_string).collect()
        }

        /// The real `where docker` output on a machine with Docker Desktop. The
        /// extensionless first line is a Git Bash shell script that `CreateProcess`
        /// refuses to run, so picking it — the naive port of `which` — produces a path
        /// that only fails once something tries to use it.
        #[test]
        fn skips_the_extensionless_shim_windows_cannot_execute() {
            let listing = "C:\\Program Files\\Docker\\Docker\\resources\\bin\\docker\n\
                           C:\\Program Files\\Docker\\Docker\\resources\\bin\\docker.exe\n";
            assert_eq!(
                first_executable(listing, &pathext()).as_deref(),
                Some("C:\\Program Files\\Docker\\Docker\\resources\\bin\\docker.exe")
            );
        }

        /// composer and npm ship as `.bat`/`.cmd` shims, which are executable and must be
        /// found — this is why the rule is "extension in PATHEXT", not "ends in .exe".
        #[test]
        fn accepts_bat_and_cmd_shims() {
            let composer = "C:\\ProgramData\\ComposerSetup\\bin\\composer\n\
                            C:\\ProgramData\\ComposerSetup\\bin\\composer.bat\n";
            assert_eq!(
                first_executable(composer, &pathext()).as_deref(),
                Some("C:\\ProgramData\\ComposerSetup\\bin\\composer.bat")
            );

            let npm = "C:\\Program Files\\nodejs\\npm\nC:\\Program Files\\nodejs\\npm.cmd\n";
            assert_eq!(
                first_executable(npm, &pathext()).as_deref(),
                Some("C:\\Program Files\\nodejs\\npm.cmd")
            );
        }

        /// `where` lists matches in PATH order, so the earliest runnable one is the copy
        /// that would actually be invoked — later installs must not win.
        #[test]
        fn keeps_path_order_between_several_installs() {
            let listing = "C:\\ProgramData\\ComposerSetup\\bin\\composer.bat\n\
                           C:\\laragon\\bin\\composer\\composer.bat\n";
            assert_eq!(
                first_executable(listing, &pathext()).as_deref(),
                Some("C:\\ProgramData\\ComposerSetup\\bin\\composer.bat")
            );
        }

        #[test]
        fn reports_nothing_when_no_match_is_runnable() {
            // Every match is a shell script or a data file.
            let listing = "C:\\tools\\thing\nC:\\tools\\thing.ps1\n";
            assert_eq!(first_executable(listing, &pathext()), None);
            assert_eq!(first_executable("", &pathext()), None);
        }

        #[test]
        fn extension_matching_ignores_case() {
            let listing = "C:\\tools\\Thing.Exe\n";
            assert_eq!(
                first_executable(listing, &pathext()).as_deref(),
                Some("C:\\tools\\Thing.Exe")
            );
        }
    }
}
