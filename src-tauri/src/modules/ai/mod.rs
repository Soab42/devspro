//! AI coding agents: a catalog of terminal agents (Claude Code, Codex, Gemini
//! CLI, OpenCode…) plus any the user adds, each reported as installed or not,
//! and launched in a PTY by the frontend (see `PtyCommand::Exec` in ssh/pty.rs).
//!
//! Detection looks the binary up on the PATH the user's *interactive login
//! shell* has, not the app's own. Nearly every one of these tools is an npm
//! global under nvm, or a `~/.local/bin` install, and neither is on the PATH a
//! desktop launcher starts DevsPro with. A shell is spawned once per listing
//! to print its PATH; a few well-known user-local directories are searched as
//! well in case the shell's profile is silent about them.
//!
//! On Windows there is no login shell to ask. The equivalent of "the PATH a
//! fresh terminal would have" is the user and machine PATH in the registry,
//! which an installer updates without touching the PATH this process started
//! with. Commands resolve through PATHEXT, since npm installs each CLI as a
//! `.cmd` shim, and a `.cmd` is launched through `cmd.exe` (see `launch_argv`).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::sync::mpsc;
#[cfg(unix)]
use std::time::Duration;
use tauri::State;

use crate::modules::settings;

const CUSTOM_AGENTS_KEY: &str = "ai.custom_agents";

struct AgentDefinition {
    id: &'static str,
    name: &'static str,
    vendor: &'static str,
    description: &'static str,
    /// The command to run; also what is looked for on the PATH.
    command: &'static str,
    /// A shell script that installs it, run in the user's login shell. None
    /// when the vendor has no one-liner worth trusting from here.
    install: Option<&'static str>,
    homepage: Option<&'static str>,
}

fn catalog() -> Vec<AgentDefinition> {
    vec![
        AgentDefinition {
            id: "claude-code",
            name: "Claude Code",
            vendor: "Anthropic",
            description: "Anthropic's agentic coding tool. Reads and edits the project, runs commands and tests, and works through tasks from the terminal.",
            command: "claude",
            install: Some("npm install -g @anthropic-ai/claude-code"),
            homepage: Some("https://docs.claude.com/en/docs/claude-code"),
        },
        AgentDefinition {
            id: "codex",
            name: "Codex CLI",
            vendor: "OpenAI",
            description: "OpenAI's coding agent for the terminal, with sandboxed command execution and approval modes.",
            command: "codex",
            install: Some("npm install -g @openai/codex"),
            homepage: Some("https://github.com/openai/codex"),
        },
        AgentDefinition {
            id: "gemini-cli",
            name: "Gemini CLI",
            vendor: "Google",
            description: "Google's open-source agent that brings Gemini into the terminal, with a large context window and built-in tools.",
            command: "gemini",
            install: Some("npm install -g @google/gemini-cli"),
            homepage: Some("https://github.com/google-gemini/gemini-cli"),
        },
        AgentDefinition {
            id: "opencode",
            name: "OpenCode",
            vendor: "SST",
            description: "Open-source terminal coding agent with a TUI; works with many model providers, including local ones.",
            command: "opencode",
            install: Some("npm install -g opencode-ai"),
            homepage: Some("https://opencode.ai"),
        },
        AgentDefinition {
            id: "copilot-cli",
            name: "Copilot CLI",
            vendor: "GitHub",
            description: "GitHub Copilot's agent for the terminal, signed in with your GitHub account.",
            command: "copilot",
            install: Some("npm install -g @github/copilot"),
            homepage: Some("https://github.com/github/copilot-cli"),
        },
        AgentDefinition {
            id: "aider",
            name: "Aider",
            vendor: "Aider",
            description: "Pair programming in the terminal that commits as it goes; works with most model providers.",
            command: "aider",
            install: Some("python3 -m pip install --user aider-install && aider-install"),
            homepage: Some("https://aider.chat"),
        },
        AgentDefinition {
            id: "qwen-code",
            name: "Qwen Code",
            vendor: "Alibaba",
            description: "A coding agent for the terminal built around the Qwen models.",
            command: "qwen",
            install: Some("npm install -g @qwen-code/qwen-code"),
            homepage: Some("https://github.com/QwenLM/qwen-code"),
        },
        AgentDefinition {
            id: "amp",
            name: "Amp",
            vendor: "Sourcegraph",
            description: "Sourcegraph's agentic coding tool for the terminal.",
            command: "amp",
            install: Some("npm install -g @sourcegraph/amp"),
            homepage: Some("https://ampcode.com"),
        },
        AgentDefinition {
            id: "cursor-agent",
            name: "Cursor Agent",
            vendor: "Cursor",
            description: "Cursor's agent outside the editor, driven from the terminal.",
            command: "cursor-agent",
            install: Some("curl -fsS https://cursor.com/install | bash"),
            homepage: Some("https://cursor.com/cli"),
        },
        AgentDefinition {
            id: "goose",
            name: "Goose",
            vendor: "Block",
            description: "An open-source, extensible agent that goes beyond code suggestions to run whole tasks.",
            command: "goose",
            install: Some("curl -fsSL https://github.com/block/goose/releases/download/stable/download_cli.sh | bash"),
            homepage: Some("https://block.github.io/goose/"),
        },
        AgentDefinition {
            id: "kimi-cli",
            name: "Kimi Code CLI",
            vendor: "Moonshot AI",
            description: "Moonshot's terminal coding agent built around the Kimi models.",
            command: "kimi",
            install: Some("curl -LsSf https://code.kimi.com/install.sh | bash"),
            homepage: Some("https://github.com/MoonshotAI/kimi-cli"),
        },
        AgentDefinition {
            id: "puku",
            name: "Puku CLI",
            vendor: "Puku",
            description: "Runs the `puku` command. Install it per the vendor's instructions; edit the command from the agent's page if yours is named differently.",
            command: "puku",
            install: None,
            homepage: None,
        },
        AgentDefinition {
            id: "agy",
            name: "Agy",
            vendor: "Agy",
            description: "Runs the `agy` command. Install it per the vendor's instructions; edit the command from the agent's page if yours is named differently.",
            command: "agy",
            install: None,
            homepage: None,
        },
    ]
}

/// An agent the user added: any command, with fixed arguments.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomAgent {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub description: String,
}

/// One row in the module: a catalog entry or a custom one, and whether the
/// command was found.
#[derive(Debug, Clone, Serialize)]
pub struct AgentStatus {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub description: String,
    pub command: String,
    pub args: Vec<String>,
    pub install: Option<String>,
    pub homepage: Option<String>,
    pub installed: bool,
    /// Where the command resolved to, when it did.
    pub path: Option<String>,
    pub custom: bool,
    /// The program and leading arguments that run the agent; its own `args`
    /// and the user's extra ones go after. Just the command on Unix.
    pub launch: Vec<String>,
    /// The program and leading arguments that run `install` as a script:
    /// `sh -c` on Unix, PowerShell on Windows.
    pub install_shell: Vec<String>,
}

/* ------------------------------------------------------------- PATH ---- */

#[cfg(unix)]
const PATH_START: &str = "__DEVSPRO_PATH__";
#[cfg(unix)]
const PATH_END: &str = "__DEVSPRO_END__";

/// Pulls the PATH out of what the shell printed, ignoring whatever else the
/// user's profile writes to stdout.
#[cfg(unix)]
pub fn parse_path_marker(output: &str) -> Option<String> {
    let start = output.rfind(PATH_START)? + PATH_START.len();
    let rest = &output[start..];
    let end = rest.find(PATH_END)?;
    Some(rest[..end].to_string())
}

/// The user's shell, as the terminal module runs it.
#[cfg(unix)]
fn shell() -> String {
    std::env::var("SHELL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".to_string())
}

/// Asks the interactive login shell for its PATH. Bounded by a timeout: a
/// profile that blocks (starts tmux, waits on a prompt) must not hang the
/// listing. None when the shell failed or timed out.
#[cfg(unix)]
fn shell_path() -> Option<String> {
    let script = format!("printf '\\n{PATH_START}%s{PATH_END}\\n' \"$PATH\"");
    let child = Command::new(shell())
        .args(["-l", "-i", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(out)) => parse_path_marker(&String::from_utf8_lossy(&out.stdout)),
        _ => None,
    }
}

/// The PATH a newly opened terminal would get: the machine PATH then the
/// user's, read fresh from the registry, so a CLI installed while the app is
/// running is found without restarting it. None when neither can be read.
#[cfg(windows)]
fn shell_path() -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let machine = registry_string(
        HKEY_LOCAL_MACHINE,
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        "Path",
    );
    let user = registry_string(HKEY_CURRENT_USER, "Environment", "Path");
    let parts: Vec<String> = [machine, user].into_iter().flatten().filter(|p| !p.is_empty()).collect();
    (!parts.is_empty()).then(|| parts.join(";"))
}

/// A string value from the registry, with `%VARIABLES%` already expanded
/// (RegGetValueW does that for REG_EXPAND_SZ unless told not to).
#[cfg(windows)]
fn registry_string(root: windows_sys::Win32::System::Registry::HKEY, subkey: &str, value: &str) -> Option<String> {
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (subkey, value) = (wide(subkey), wide(value));
    let mut buf: Vec<u16> = vec![0; 2048];
    loop {
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: both names are NUL-terminated and `size` is the buffer's
        // length in bytes.
        let status = unsafe {
            RegGetValueW(
                root,
                subkey.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        match status {
            ERROR_SUCCESS => {
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                return Some(String::from_utf16_lossy(&buf[..len]));
            }
            // Expansion can need more room than the size reported; grow and retry.
            ERROR_MORE_DATA if buf.len() < 1 << 20 => {
                let wanted = (size as usize / 2).max(buf.len() * 2);
                buf.resize(wanted, 0);
            }
            _ => return None,
        }
    }
}

/// Directories that commonly hold user-installed CLIs and are worth checking
/// even when the shell's profile doesn't add them.
#[cfg(unix)]
fn extra_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/local/bin")];
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        for rel in [".local/bin", "bin", ".npm-global/bin", ".cargo/bin", ".bun/bin", ".opencode/bin"] {
            dirs.push(home.join(rel));
        }
        // nvm keeps one bin dir per Node version; a global install lands in
        // whichever was current at the time.
        if let Ok(versions) = std::fs::read_dir(home.join(".nvm/versions/node")) {
            for entry in versions.flatten() {
                dirs.push(entry.path().join("bin"));
            }
        }
    }
    dirs
}

/// The Windows counterparts: npm's global prefix, the per-user install
/// folders the native installers use, and the package managers' shim dirs.
#[cfg(windows)]
fn extra_dirs() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut dirs = Vec::new();
    if let Some(appdata) = var("APPDATA") {
        dirs.push(appdata.join("npm"));
        // `pip install --user` scripts: Python\Python312\Scripts and so on.
        if let Ok(versions) = std::fs::read_dir(appdata.join("Python")) {
            for entry in versions.flatten() {
                dirs.push(entry.path().join("Scripts"));
            }
        }
    }
    if let Some(local) = var("LOCALAPPDATA") {
        dirs.push(local.join("pnpm"));
        dirs.push(local.join("Microsoft").join("WinGet").join("Links"));
        let programs = local.join("Programs");
        dirs.push(programs.clone());
        // Per-user installs put their CLI in the app folder or its bin.
        if let Ok(apps) = std::fs::read_dir(&programs) {
            for entry in apps.flatten() {
                dirs.push(entry.path());
                dirs.push(entry.path().join("bin"));
            }
        }
    }
    if let Some(home) = home_dir() {
        for rel in [r".local\bin", "bin", r".cargo\bin", r".bun\bin", r".opencode\bin", r"scoop\shims"] {
            dirs.push(home.join(rel));
        }
    }
    if let Some(program_files) = var("ProgramFiles") {
        dirs.push(program_files.join("nodejs"));
    }
    dirs
}

/// What `~` stands for on Windows: the user profile, falling back to HOME,
/// which Git Bash and MSYS set.
#[cfg(windows)]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var_os("HOME").filter(|h| !h.is_empty()))
        .map(PathBuf::from)
}

/// Every directory to search, in order, without duplicates.
pub fn search_dirs(shell_path: Option<&str>, process_path: Option<&str>, extras: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut push = |dir: PathBuf| {
        if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    };
    for path in [shell_path, process_path].into_iter().flatten() {
        for dir in std::env::split_paths(path) {
            push(dir);
        }
    }
    for dir in extras {
        push(dir);
    }
    dirs
}

fn is_executable(path: &Path) -> bool {
    crate::modules::fs_compat::is_executable(path)
}

/// Resolves a command the way a shell would: a name is looked up along the
/// search dirs, anything with a slash is taken as a path (with `~` expanded).
#[cfg(unix)]
pub fn find_command(command: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    if command.contains('/') {
        let expanded = match command.strip_prefix("~/") {
            Some(rest) => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest),
            None => PathBuf::from(command),
        };
        return is_executable(&expanded).then_some(expanded);
    }
    dirs.iter().map(|d| d.join(command)).find(|p| is_executable(p))
}

/// Resolves a command the way cmd.exe would: either slash makes it a path
/// (with `~` expanded to the user profile), and a name without an executable
/// extension is tried with each PATHEXT one in turn — `claude` is found as
/// `claude.cmd`, never as the extensionless sh script npm puts beside it.
#[cfg(windows)]
pub fn find_command(command: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    let exts = pathext();
    if command.contains(['/', '\\']) || Path::new(command).is_absolute() {
        let expanded = match command.strip_prefix("~/").or_else(|| command.strip_prefix("~\\")) {
            Some(rest) => home_dir().unwrap_or_default().join(rest),
            None => PathBuf::from(command),
        };
        return with_extensions(&expanded, &exts).into_iter().find(|p| is_executable(p));
    }
    dirs.iter()
        .flat_map(|d| with_extensions(&d.join(command), &exts))
        .find(|p| is_executable(p))
}

/// PATHEXT as lowercase extensions, or the stock list when it is unset.
#[cfg(windows)]
fn pathext() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| e.starts_with('.') && e.len() > 1)
        .collect()
}

/// The files `path` may name: itself when it already has an extension
/// PATHEXT lists, otherwise itself with each one appended.
#[cfg(windows)]
fn with_extensions(path: &Path, exts: &[String]) -> Vec<PathBuf> {
    let has_known = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()))
        .is_some_and(|e| exts.contains(&e));
    if has_known {
        return vec![path.to_path_buf()];
    }
    exts.iter()
        .map(|ext| {
            let mut name = path.as_os_str().to_os_string();
            name.push(ext);
            PathBuf::from(name)
        })
        .collect()
}

/// The program and leading arguments that run `command`. On Unix it is the
/// command as typed, resolved by the shell the PTY runs it through.
#[cfg(unix)]
pub fn launch_argv(command: &str, _resolved: Option<&Path>) -> Vec<String> {
    vec![command.to_string()]
}

/// On Windows the resolved path, so a CLI found in a directory that isn't on
/// this process's PATH still runs. A `.cmd`/`.bat` shim can't be started
/// directly — CreateProcess only runs real executables — so it goes through
/// `cmd /c call`: with `call` first, cmd never strips the quotes around a
/// path with spaces in it, which it otherwise does when the line starts with
/// a quote.
#[cfg(windows)]
pub fn launch_argv(command: &str, resolved: Option<&Path>) -> Vec<String> {
    let Some(path) = resolved else {
        return vec![command.to_string()];
    };
    let path_str = path.to_string_lossy().into_owned();
    let is_script = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if is_script {
        vec!["cmd.exe".to_string(), "/d".to_string(), "/c".to_string(), "call".to_string(), path_str]
    } else {
        vec![path_str]
    }
}

/// What runs an install one-liner.
#[cfg(unix)]
fn install_shell() -> Vec<String> {
    vec!["sh".to_string(), "-c".to_string()]
}

/// PowerShell rather than cmd: it runs the `npm install` lines just as well,
/// and the vendors' Windows installers are `irm … | iex` scripts.
#[cfg(windows)]
fn install_shell() -> Vec<String> {
    ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"]
        .map(String::from)
        .to_vec()
}

/// The install one-liner to offer on this OS. On Unix, the catalog's own.
#[cfg(unix)]
fn install_script(def: &AgentDefinition) -> Option<&'static str> {
    def.install
}

/// On Windows only what is known to work there: the npm installs, which are
/// the same command, and vendors' PowerShell installers. The `curl | bash`
/// scripts have no Windows equivalent here, so those agents show no button.
#[cfg(windows)]
fn install_script(def: &AgentDefinition) -> Option<&'static str> {
    match def.id {
        "aider" => Some("irm https://aider.chat/install.ps1 | iex"),
        _ => def.install.filter(|script| script.starts_with("npm ")),
    }
}

/* ---------------------------------------------------- custom agents ---- */

fn load_custom(conn: &rusqlite::Connection) -> Vec<CustomAgent> {
    settings::get(conn, CUSTOM_AGENTS_KEY)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn store_custom(conn: &rusqlite::Connection, agents: &[CustomAgent]) -> Result<(), String> {
    let json = serde_json::to_string(agents).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (CUSTOM_AGENTS_KEY, json),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Validates the fields of a custom agent and returns them trimmed.
pub fn validate_custom(name: &str, command: &str) -> Result<(String, String), String> {
    let name = name.trim();
    let command = command.trim();
    if name.is_empty() {
        return Err("give the agent a name".to_string());
    }
    if command.is_empty() {
        return Err("the command cannot be empty".to_string());
    }
    if command.chars().any(char::is_whitespace) {
        return Err("the command is one program; put its arguments in the arguments field".to_string());
    }
    Ok((name.to_string(), command.to_string()))
}

/// Adds or updates one custom agent in the list, keeping order for updates.
pub fn upsert_custom(agents: &mut Vec<CustomAgent>, agent: CustomAgent) {
    match agents.iter_mut().find(|a| a.id == agent.id) {
        Some(existing) => *existing = agent,
        None => agents.push(agent),
    }
}

/* ---------------------------------------------------------- commands ---- */

fn status_of(dirs: &[PathBuf], custom: Option<&CustomAgent>, def: Option<&AgentDefinition>) -> AgentStatus {
    let (id, name, vendor, description, command, args, install, homepage, is_custom) = match (custom, def) {
        (Some(c), _) => (
            c.id.clone(),
            c.name.clone(),
            "Custom".to_string(),
            c.description.clone(),
            c.command.clone(),
            c.args.clone(),
            None,
            None,
            true,
        ),
        (None, Some(d)) => (
            d.id.to_string(),
            d.name.to_string(),
            d.vendor.to_string(),
            d.description.to_string(),
            d.command.to_string(),
            Vec::new(),
            install_script(d).map(String::from),
            d.homepage.map(String::from),
            false,
        ),
        (None, None) => unreachable!("status_of needs a definition"),
    };
    let path = find_command(&command, dirs);
    let launch = launch_argv(&command, path.as_deref());
    AgentStatus {
        id,
        name,
        vendor,
        description,
        command,
        args,
        install,
        homepage,
        installed: path.is_some(),
        path: path.map(|p| p.to_string_lossy().into_owned()),
        custom: is_custom,
        launch,
        install_shell: install_shell(),
    }
}

/// Every agent, catalog first then custom, each with whether its command
/// resolves. One shell spawn for the PATH, then only filesystem checks.
#[tauri::command(async)]
pub fn ai_agents_list(db: State<crate::db::Db>) -> Result<Vec<AgentStatus>, String> {
    let custom = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        load_custom(&conn)
    };
    let shell_path = shell_path();
    let process_path = std::env::var("PATH").ok();
    let dirs = search_dirs(shell_path.as_deref(), process_path.as_deref(), extra_dirs());

    let mut out: Vec<AgentStatus> = catalog().iter().map(|d| status_of(&dirs, None, Some(d))).collect();
    out.extend(custom.iter().map(|c| status_of(&dirs, Some(c), None)));
    Ok(out)
}

#[tauri::command(async)]
pub fn ai_custom_agent_save(
    db: State<crate::db::Db>,
    id: Option<String>,
    name: String,
    command: String,
    args: Vec<String>,
    description: String,
) -> Result<AgentStatus, String> {
    let (name, command) = validate_custom(&name, &command)?;
    let agent = CustomAgent {
        id: id.unwrap_or_else(|| format!("custom-{}", uuid::Uuid::new_v4())),
        name,
        command,
        args: args.into_iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect(),
        description: description.trim().to_string(),
    };
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut agents = load_custom(&conn);
    upsert_custom(&mut agents, agent.clone());
    store_custom(&conn, &agents)?;
    drop(conn);

    let process_path = std::env::var("PATH").ok();
    let dirs = search_dirs(shell_path().as_deref(), process_path.as_deref(), extra_dirs());
    Ok(status_of(&dirs, Some(&agent), None))
}

#[tauri::command(async)]
pub fn ai_custom_agent_delete(db: State<crate::db::Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut agents = load_custom(&conn);
    agents.retain(|a| a.id != id);
    store_custom(&conn, &agents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn path_marker_is_found_after_profile_noise() {
        let out = "Welcome!\nsome banner\n__DEVSPRO_PATH__/a/bin:/b/bin__DEVSPRO_END__\n";
        assert_eq!(parse_path_marker(out).as_deref(), Some("/a/bin:/b/bin"));
        assert_eq!(parse_path_marker("nothing here"), None);
    }

    #[cfg(unix)]
    #[test]
    fn search_dirs_merges_in_order_without_duplicates() {
        let dirs = search_dirs(
            Some("/home/u/.nvm/v/bin:/usr/bin"),
            Some("/usr/bin:/bin"),
            vec![PathBuf::from("/usr/local/bin"), PathBuf::from("/bin")],
        );
        let as_str: Vec<_> = dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect();
        assert_eq!(as_str, ["/home/u/.nvm/v/bin", "/usr/bin", "/bin", "/usr/local/bin"]);
    }

    #[cfg(unix)]
    #[test]
    fn find_command_resolves_executables_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("devspro-ai-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("agent");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.join("notes"), "").unwrap();

        let dirs = vec![dir.clone()];
        assert_eq!(find_command("agent", &dirs), Some(exe.clone()));
        assert_eq!(find_command("notes", &dirs), None);
        assert_eq!(find_command("missing", &dirs), None);
        assert_eq!(find_command(&exe.to_string_lossy(), &[]), Some(exe));
        assert_eq!(find_command("", &dirs), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn launch_is_the_command_as_typed() {
        assert_eq!(launch_argv("claude", Some(Path::new("/usr/bin/claude"))), ["claude"]);
        assert_eq!(install_shell(), ["sh", "-c"]);
    }

    #[cfg(windows)]
    #[test]
    fn search_dirs_splits_on_semicolons_in_order_without_duplicates() {
        let dirs = search_dirs(
            Some(r"C:\Users\u\AppData\Roaming\npm;C:\Windows\system32;"),
            Some(r"C:\Windows\system32;C:\Program Files\nodejs"),
            vec![PathBuf::from(r"C:\Program Files\nodejs"), PathBuf::from(r"C:\tools")],
        );
        let as_str: Vec<_> = dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect();
        assert_eq!(
            as_str,
            [r"C:\Users\u\AppData\Roaming\npm", r"C:\Windows\system32", r"C:\Program Files\nodejs", r"C:\tools"]
        );
    }

    /// An npm global install on Windows: an extensionless sh script, a .cmd
    /// shim and a .ps1, side by side. Only the .cmd is runnable.
    #[cfg(windows)]
    #[test]
    fn find_command_resolves_through_pathext() {
        let dir = std::env::temp_dir().join(format!("devspro ai {}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["claude", "claude.cmd", "claude.ps1", "notes.txt", "tool.exe"] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        let dirs = vec![PathBuf::from(r"C:\nonexistent"), dir.clone()];
        assert_eq!(find_command("claude", &dirs), Some(dir.join("claude.cmd")));
        assert_eq!(find_command("claude.cmd", &dirs), Some(dir.join("claude.cmd")));
        assert_eq!(find_command("tool", &dirs), Some(dir.join("tool.exe")));
        assert_eq!(find_command("notes", &dirs), None);
        assert_eq!(find_command("missing", &dirs), None);
        // A full path, with or without its extension, and with either slash.
        let full = dir.join("claude");
        assert_eq!(find_command(&full.to_string_lossy(), &[]), Some(dir.join("claude.cmd")));
        let forward = dir.join("tool.exe").to_string_lossy().replace('\\', "/");
        assert_eq!(find_command(&forward, &[]), Some(PathBuf::from(&forward)));
        assert_eq!(find_command("", &dirs), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn find_command_expands_tilde_to_the_user_profile() {
        let profile = PathBuf::from(std::env::var_os("USERPROFILE").expect("USERPROFILE is set"));
        let name = format!("devspro-ai-{}", uuid::Uuid::new_v4().simple());
        let exe = profile.join(format!("{name}.exe"));
        std::fs::write(&exe, "").unwrap();
        assert_eq!(find_command(&format!("~/{name}"), &[]), Some(exe.clone()));
        assert_eq!(find_command(&format!("~\\{name}.exe"), &[]), Some(exe.clone()));
        std::fs::remove_file(&exe).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn cmd_shims_launch_through_cmd_call_and_exes_directly() {
        let shim = Path::new(r"C:\Users\MD. Talha\AppData\Roaming\npm\claude.cmd");
        assert_eq!(
            launch_argv("claude", Some(shim)),
            ["cmd.exe", "/d", "/c", "call", r"C:\Users\MD. Talha\AppData\Roaming\npm\claude.cmd"]
        );
        assert_eq!(launch_argv("x", Some(Path::new(r"C:\t\x.BAT")))[..4], ["cmd.exe", "/d", "/c", "call"]);
        let exe = Path::new(r"C:\Users\me\.local\bin\claude.exe");
        assert_eq!(launch_argv("claude", Some(exe)), [r"C:\Users\me\.local\bin\claude.exe"]);
        assert_eq!(launch_argv("claude", None), ["claude"]);
    }

    /// The argv really runs: a shim in a folder with a space in its name, an
    /// argument with a space in it, quoted the way CreateProcess callers do.
    #[cfg(windows)]
    #[test]
    fn a_cmd_shim_in_a_spaced_folder_runs_with_its_arguments() {
        let dir = std::env::temp_dir().join(format!("devspro ai {}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("agent.cmd");
        std::fs::write(&shim, "@echo [%~1] [%~2]\r\n").unwrap();

        let argv = launch_argv("agent", Some(&shim));
        let out = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .args(["two words", "plain"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "[two words] [plain]");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_offers_only_installers_that_run_there() {
        let defs = catalog();
        let script = |id: &str| install_script(defs.iter().find(|d| d.id == id).unwrap());
        assert_eq!(script("claude-code"), Some("npm install -g @anthropic-ai/claude-code"));
        assert_eq!(script("aider"), Some("irm https://aider.chat/install.ps1 | iex"));
        assert_eq!(script("goose"), None);
        assert_eq!(install_shell()[0], "powershell.exe");
    }

    /// The registry PATH is what a fresh terminal gets; it always holds
    /// System32 on a working Windows.
    #[cfg(windows)]
    #[test]
    fn registry_path_is_readable_and_expanded() {
        let path = shell_path().expect("PATH in the registry");
        assert!(!path.contains("%SystemRoot%"), "{path}");
        assert!(path.to_ascii_lowercase().contains(r"\system32"), "{path}");
    }

    #[test]
    fn custom_validation_rejects_blank_and_multiword_commands() {
        assert!(validate_custom("", "x").is_err());
        assert!(validate_custom("Mine", "  ").is_err());
        assert!(validate_custom("Mine", "foo --bar").is_err());
        assert_eq!(validate_custom(" Mine ", " foo ").unwrap(), ("Mine".to_string(), "foo".to_string()));
    }

    #[test]
    fn upsert_replaces_in_place_or_appends() {
        let a = |id: &str, name: &str| CustomAgent {
            id: id.into(),
            name: name.into(),
            command: "x".into(),
            args: vec![],
            description: String::new(),
        };
        let mut list = vec![a("1", "one"), a("2", "two")];
        upsert_custom(&mut list, a("1", "uno"));
        upsert_custom(&mut list, a("3", "three"));
        let names: Vec<_> = list.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["uno", "two", "three"]);
    }

    #[test]
    fn custom_agent_json_round_trips_with_defaults() {
        let parsed: Vec<CustomAgent> = serde_json::from_str(r#"[{"id":"c","name":"N","command":"n"}]"#).unwrap();
        assert_eq!(parsed[0].args, Vec::<String>::new());
        assert_eq!(parsed[0].description, "");
    }

    #[test]
    fn catalog_ids_and_commands_are_unique() {
        let defs = catalog();
        let mut ids: Vec<_> = defs.iter().map(|d| d.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), defs.len());
    }
}
