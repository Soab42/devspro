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

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
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
}

/* ------------------------------------------------------------- PATH ---- */

const PATH_START: &str = "__DEVSPRO_PATH__";
const PATH_END: &str = "__DEVSPRO_END__";

/// Pulls the PATH out of what the shell printed, ignoring whatever else the
/// user's profile writes to stdout.
pub fn parse_path_marker(output: &str) -> Option<String> {
    let start = output.rfind(PATH_START)? + PATH_START.len();
    let rest = &output[start..];
    let end = rest.find(PATH_END)?;
    Some(rest[..end].to_string())
}

/// The user's shell, as the terminal module runs it.
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

/// Directories that commonly hold user-installed CLIs and are worth checking
/// even when the shell's profile doesn't add them.
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

/// Every directory to search, in order, without duplicates.
pub fn search_dirs(shell_path: Option<&str>, process_path: Option<&str>, extras: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut push = |dir: PathBuf| {
        if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    };
    for path in [shell_path, process_path].into_iter().flatten() {
        for dir in path.split(':') {
            push(PathBuf::from(dir));
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
            d.install.map(String::from),
            d.homepage.map(String::from),
            false,
        ),
        (None, None) => unreachable!("status_of needs a definition"),
    };
    let path = find_command(&command, dirs);
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
    fn path_marker_is_found_after_profile_noise() {
        let out = "Welcome!\nsome banner\n__DEVSPRO_PATH__/a/bin:/b/bin__DEVSPRO_END__\n";
        assert_eq!(parse_path_marker(out).as_deref(), Some("/a/bin:/b/bin"));
        assert_eq!(parse_path_marker("nothing here"), None);
    }

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
