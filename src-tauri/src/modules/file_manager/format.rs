//! Format on demand, the way VS Code's formatter extensions do: with the
//! project's own tool and config rather than a built-in opinion. The unsaved
//! buffer goes in on stdin and the result comes back; the file on disk is
//! never read or written here. Tools run in the file's folder so
//! `rustfmt.toml`, `.prettierrc` and `pyproject.toml` are found as they would
//! be from the terminal.
//!
//! Prettier is the one tool that may be absent while the buffer is still
//! formattable: the frontend bundles a copy. In that case the answer carries
//! the project's prettier config instead of a result, so the bundled copy
//! can follow the project's style.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::process::run;

const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Formatted {
    /// The formatted text; `None` when no installed tool could do it and the
    /// frontend should fall back to its bundled prettier.
    pub contents: Option<String>,
    pub tool: Option<String>,
    /// Raw JSON of the project's prettier settings, only alongside `None` contents.
    pub prettier_config: Option<String>,
}

const PRETTIER_EXTS: &[&str] = &[
    "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "json", "jsonc", "json5", "css", "scss", "less", "html", "htm", "vue", "md",
    "markdown", "mdx", "yaml", "yml", "graphql", "gql",
];

/// What to run for a file: a name to show and the argv, program first.
#[derive(Debug, Clone, PartialEq)]
struct Tool {
    name: &'static str,
    argv: Vec<String>,
}

fn tool(name: &'static str, argv: &[&str]) -> Tool {
    Tool { name, argv: argv.iter().map(|a| a.to_string()).collect() }
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// The first installed tool for `path`, or the message naming what to install.
/// Prettier is handled by the caller; this covers every other language.
fn pick_tool(path: &Path, path_var: &str, edition: &str) -> Result<Tool, String> {
    let file = path.to_string_lossy();
    let ext = extension(path);
    let candidates: Vec<Tool> = match ext.as_str() {
        "rs" => vec![tool("rustfmt", &["rustfmt", "--edition", edition])],
        "py" => vec![
            tool("ruff", &["ruff", "format", "--stdin-filename", &file, "-"]),
            tool("black", &["black", "-q", "--stdin-filename", &file, "-"]),
        ],
        "go" => vec![tool("gofmt", &["gofmt"])],
        "sh" | "bash" | "zsh" => vec![tool("shfmt", &["shfmt", "--filename", &file])],
        "c" | "h" | "cpp" | "hpp" | "cc" | "cxx" | "java" | "proto" => {
            vec![tool("clang-format", &["clang-format", &format!("--assume-filename={file}")])]
        }
        "xml" | "svg" => vec![tool("xmllint", &["xmllint", "--format", "-"])],
        "toml" => vec![tool("taplo", &["taplo", "fmt", "-"])],
        _ => {
            let what = if ext.is_empty() { "this file".to_string() } else { format!(".{ext} files") };
            return Err(format!("No formatter for {what}"));
        }
    };
    let found = candidates.iter().find(|t| super::on_path(&t.argv[0], path_var)).cloned();
    found.ok_or_else(|| {
        let names: Vec<&str> = candidates.iter().map(|t| t.name).collect();
        format!("No formatter for .{ext} files — install {}", names.join(" or "))
    })
}

/// `node_modules/.bin/prettier` of the nearest enclosing project. The walk
/// stops after the home folder: a stray `~/node_modules` further up should not
/// style an unrelated project.
fn project_prettier(dir: &Path, home: Option<&Path>) -> Option<PathBuf> {
    for ancestor in dir.ancestors() {
        let bin = ancestor.join("node_modules/.bin/prettier");
        if bin.is_file() {
            return Some(bin);
        }
        if home.is_some_and(|h| ancestor == h) {
            break;
        }
    }
    None
}

/// The nearest prettier settings walking up from `dir`: a `.prettierrc` (or
/// `.prettierrc.json`) verbatim, else the `"prettier"` object of a
/// package.json. Only JSON forms are read; the frontend cannot run a JS config.
fn prettier_config(dir: &Path, home: Option<&Path>) -> Option<String> {
    for ancestor in dir.ancestors() {
        for name in [".prettierrc", ".prettierrc.json"] {
            if let Ok(text) = std::fs::read_to_string(ancestor.join(name)) {
                return Some(text);
            }
        }
        if let Ok(text) = std::fs::read_to_string(ancestor.join("package.json")) {
            if let Some(cfg) = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v.get("prettier").cloned()) {
                if cfg.is_object() {
                    return Some(cfg.to_string());
                }
            }
        }
        if home.is_some_and(|h| ancestor == h) {
            break;
        }
    }
    None
}

/// `edition = "2018"` from the nearest Cargo.toml, since rustfmt on stdin has
/// no manifest to read it from and would assume 2015.
fn cargo_edition(dir: &Path) -> Option<String> {
    let manifest = dir.ancestors().map(|d| d.join("Cargo.toml")).find(|p| p.is_file())?;
    std::fs::read_to_string(manifest).ok()?.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("edition")?.trim_start().strip_prefix('=')?;
        let year = rest.trim().trim_matches('"');
        (!year.is_empty() && year.bytes().all(|b| b.is_ascii_digit())).then(|| year.to_string())
    })
}

fn run_tool(tool: &Tool, dir: &Path, contents: &str) -> Result<Formatted, String> {
    let out = run(&tool.argv[0], &tool.argv[1..], Some(dir), Some(contents.as_bytes()), TIMEOUT)?;
    Ok(Formatted { contents: Some(out), tool: Some(tool.name.to_string()), prettier_config: None })
}

fn format_with(path: &Path, contents: &str, path_var: &str, home: Option<&Path>) -> Result<Formatted, String> {
    let dir = path.parent().filter(|d| d.is_dir()).ok_or_else(|| "the file's folder does not exist".to_string())?;
    let file = path.to_string_lossy().into_owned();
    if PRETTIER_EXTS.contains(&extension(path).as_str()) {
        let local = project_prettier(dir, home).map(|p| p.to_string_lossy().into_owned());
        let program = local.or_else(|| super::on_path("prettier", path_var).then(|| "prettier".to_string()));
        return match program {
            Some(program) => {
                let argv = vec![program, "--stdin-filepath".to_string(), file];
                run_tool(&Tool { name: "prettier", argv }, dir, contents)
            }
            None => Ok(Formatted { contents: None, tool: None, prettier_config: prettier_config(dir, home) }),
        };
    }
    let edition = cargo_edition(dir).unwrap_or_else(|| "2021".to_string());
    run_tool(&pick_tool(path, path_var, &edition)?, dir, contents)
}

/// Formats an editor buffer as the file at `path` with the project's tool.
#[tauri::command(async)]
pub fn fm_format(path: String, contents: String) -> Result<Formatted, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let path_var = std::env::var("PATH").unwrap_or_default();
    format_with(Path::new(&path), &contents, &path_var, home.as_deref())
}

#[cfg(test)]
mod tests {
    use super::super::process::installed;
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devspro-format-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn tools_are_chosen_by_extension_and_availability() {
        let dir = temp();
        std::fs::write(dir.join("black"), "").unwrap();
        std::fs::write(dir.join("rustfmt"), "").unwrap();
        let path_var = dir.to_string_lossy().into_owned();
        assert_eq!(pick_tool(Path::new("/x/a.py"), &path_var, "2021").unwrap().name, "black");
        let rs = pick_tool(Path::new("/x/A.RS"), &path_var, "2018").unwrap();
        assert_eq!((rs.name, rs.argv[2].as_str()), ("rustfmt", "2018"));
        let err = pick_tool(Path::new("/x/a.go"), &path_var, "2021").unwrap_err();
        assert!(err.contains("install gofmt"), "{err}");
        let err = pick_tool(Path::new("/x/a.py"), "", "2021").unwrap_err();
        assert!(err.contains("install ruff or black"), "{err}");
        assert!(pick_tool(Path::new("/x/a.xyz"), &path_var, "2021").unwrap_err().starts_with("No formatter for .xyz files"));
        assert!(pick_tool(Path::new("/x/Makefile"), &path_var, "2021").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prettier_config_comes_from_prettierrc_or_package_json() {
        let root = temp();
        let sub = root.join("src/deep");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(prettier_config(&sub, Some(&root)), None);
        std::fs::write(root.join("package.json"), r#"{"name":"x","prettier":{"semi":false}}"#).unwrap();
        assert_eq!(prettier_config(&sub, Some(&root)).as_deref(), Some(r#"{"semi":false}"#));
        std::fs::write(root.join("src/.prettierrc"), "{\"tabWidth\": 4}").unwrap();
        assert_eq!(prettier_config(&sub, Some(&root)).as_deref(), Some("{\"tabWidth\": 4}"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_prettier_wins_and_the_walk_stops_at_home() {
        let home = temp();
        let proj = home.join("proj/src");
        std::fs::create_dir_all(&proj).unwrap();
        let bin = home.join("proj/node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("prettier"), "").unwrap();
        assert_eq!(project_prettier(&proj, Some(&home)), Some(bin.join("prettier")));
        // A prettier above home is out of reach.
        let outer = temp();
        std::fs::create_dir_all(outer.join("node_modules/.bin")).unwrap();
        std::fs::write(outer.join("node_modules/.bin/prettier"), "").unwrap();
        let inner_home = outer.join("home");
        std::fs::create_dir_all(inner_home.join("p")).unwrap();
        assert_eq!(project_prettier(&inner_home.join("p"), Some(&inner_home)), None);
        std::fs::remove_dir_all(home).unwrap();
        std::fs::remove_dir_all(outer).unwrap();
    }

    #[test]
    fn missing_prettier_hands_the_config_to_the_frontend() {
        let dir = temp();
        std::fs::write(dir.join(".prettierrc"), "{\"singleQuote\":true}").unwrap();
        let out = format_with(&dir.join("a.ts"), "let a = 1", "", Some(&dir)).unwrap();
        assert_eq!(out, Formatted { contents: None, tool: None, prettier_config: Some("{\"singleQuote\":true}".into()) });
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn edition_comes_from_the_nearest_manifest() {
        let dir = temp();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\nedition = \"2018\"\n").unwrap();
        assert_eq!(cargo_edition(&dir.join("src")).as_deref(), Some("2018"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rustfmt_round_trip() {
        // The rustup shim is on PATH even when the component is not installed.
        if !installed("rustfmt") || run("rustfmt", &["--version"], None, None, TIMEOUT).is_err() {
            return;
        }
        let dir = temp();
        let path = dir.join("a.rs");
        let out = format_with(&path, "fn   main( ){let x=1;}", &std::env::var("PATH").unwrap(), None).unwrap();
        assert_eq!(out.tool.as_deref(), Some("rustfmt"));
        assert_eq!(out.contents.as_deref(), Some("fn main() {\n    let x = 1;\n}\n"));
        let err = format_with(&path, "fn (", &std::env::var("PATH").unwrap(), None).unwrap_err();
        assert!(!err.is_empty());
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
