//! Git for the editor: the status of a folder's files, the index version of a
//! file to diff the buffer against (the change gutter), per-line blame, and a
//! file's history. Everything shells out to the real `git`, so hooks-free
//! reads agree with what the user's terminal shows.
//!
//! Every command degrades to "nothing to show" rather than an error when git
//! is missing or the path is not in a repository: the editor asks on every
//! file it opens, and most folders are not repos. Paths on the GVfs bridge are
//! never handed to git at all; scanning a phone or a Drive mount for a `.git`
//! would take as long as the mount cares to.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::process::{installed, run, run_bytes};

const QUICK: Duration = Duration::from_secs(10);
const BLAME: Duration = Duration::from_secs(30);
const LOG_LIMIT: &str = "300";
/// `git blame` reports lines not in any commit under this id.
const UNCOMMITTED_SHA: &str = "0000000000000000000000000000000000000000";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GitStatus {
    pub root: String,
    /// `None` on a detached HEAD, where there is no branch name to show.
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<GitFile>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GitFile {
    /// Absolute, without a trailing slash (an untracked folder is one entry).
    pub path: String,
    /// `M` modified, `A` added, `D` deleted, `R` renamed, `U` untracked,
    /// `C` in conflict.
    pub code: String,
    pub staged: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GitCommit {
    pub sha: String,
    pub author: String,
    pub email: String,
    /// Unix seconds.
    pub time: u64,
    pub summary: String,
    pub uncommitted: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct GitBlame {
    pub commits: Vec<GitCommit>,
    /// `lines[i]` indexes `commits` for line `i + 1` of the blamed contents.
    pub lines: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GitLogEntry {
    pub sha: String,
    pub author: String,
    pub email: String,
    pub time: u64,
    pub summary: String,
    /// Repo-relative path of the file at that commit; differs from today's
    /// after a rename.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GitShow {
    pub before: String,
    pub after: String,
}

/// The repository containing `path` (a file, or a folder) and `path`'s
/// location inside it. `None` for anything git should not be asked about.
fn locate(path: &str) -> Option<(PathBuf, PathBuf)> {
    let path = Path::new(path);
    if path.starts_with(super::gvfs::gvfs_root()) || !installed("git") {
        return None;
    }
    let dir = if path.is_dir() { path } else { path.parent()? };
    let out = run("git", &["-C".as_ref(), dir.as_os_str(), "rev-parse".as_ref(), "--show-toplevel".as_ref()], None, None, QUICK).ok()?;
    let root = canonical(Path::new(out.trim_end_matches(['\r', '\n']))).ok()?;
    // A deleted file cannot be canonicalized, but its folder can.
    let full = match canonical(path) {
        Ok(p) => p,
        Err(_) => canonical(path.parent()?).ok()?.join(path.file_name()?),
    };
    let rel = full.strip_prefix(&root).ok()?.to_path_buf();
    Some((root, rel))
}

/// `fs::canonicalize`, spelled the way the rest of the app spells paths. On
/// Windows that means without the `\\?\` prefix it adds, which no listing
/// path carries and which would keep every status entry from matching a row.
fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    let p = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    let p = super::windows::plain(&p);
    Ok(p)
}

/// A repo-relative path as git writes it: forward slashes, which `HEAD:path`
/// requires even on Windows.
fn git_path(rel: &Path) -> String {
    let s = rel.to_string_lossy().into_owned();
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s
    }
}

/// `root` joined with a path git printed (always `/`-separated), in the OS's
/// own spelling so it matches the paths a listing hands out.
fn under_root(root: &Path, git_rel: &str) -> PathBuf {
    if cfg!(windows) {
        root.join(git_rel.replace('/', "\\"))
    } else {
        root.join(git_rel)
    }
}

fn git_args<'a>(root: &'a Path, rest: &[&'a str]) -> Vec<&'a std::ffi::OsStr> {
    let mut args: Vec<&std::ffi::OsStr> = vec!["-C".as_ref(), root.as_os_str()];
    args.extend(rest.iter().map(|&a| -> &'a std::ffi::OsStr { a.as_ref() }));
    args
}

/// Branch, upstream and changed files of the repository containing `dir`.
#[tauri::command(async)]
pub fn fm_git_status(dir: String) -> Option<GitStatus> {
    let (root, _) = locate(&dir)?;
    // No optional locks: this runs on every folder visit and must not fight
    // the user's own git commands over `index.lock`.
    let args = git_args(
        &root,
        &["--no-optional-locks", "status", "--porcelain=v1", "-z", "--branch", "--untracked-files=normal"],
    );
    let out = run("git", &args, None, None, QUICK).ok()?;
    Some(parse_status(&out, &root))
}

/// `## main...origin/main [ahead 1, behind 2]`, `## main`,
/// `## No commits yet on main`, `## HEAD (no branch)`.
fn parse_branch_header(header: &str) -> (Option<String>, Option<String>, u32, u32) {
    let header = header.trim_start_matches("## ");
    if header.starts_with("HEAD (no branch)") {
        return (None, None, 0, 0);
    }
    let header = header
        .strip_prefix("No commits yet on ")
        .or_else(|| header.strip_prefix("Initial commit on "))
        .unwrap_or(header);
    let (names, counts) = match header.split_once(" [") {
        Some((names, counts)) => (names, counts.trim_end_matches(']')),
        None => (header, ""),
    };
    let (branch, upstream) = match names.split_once("...") {
        Some((b, u)) => (b, Some(u.to_string())),
        None => (names, None),
    };
    let count = |word: &str| {
        counts
            .split(", ")
            .find_map(|part| part.strip_prefix(word))
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(0)
    };
    (Some(branch.to_string()).filter(|b| !b.is_empty()), upstream, count("ahead "), count("behind "))
}

fn status_code(x: char, y: char) -> Option<&'static str> {
    match (x, y) {
        ('?', '?') => Some("U"),
        ('!', '!') => None,
        ('D', 'D') | ('A', 'U') | ('U', 'D') | ('U', 'A') | ('D', 'U') | ('A', 'A') | ('U', 'U') => Some("C"),
        _ => match if y != ' ' { y } else { x } {
            'M' | 'T' => Some("M"),
            'A' | 'C' => Some("A"),
            'D' => Some("D"),
            'R' => Some("R"),
            _ => None,
        },
    }
}

/// `-z` output: the header, then `XY path` entries, all NUL-terminated. A
/// rename or copy carries its old path as one extra field. Nothing is quoted,
/// so spaces and unicode in paths arrive as they are.
fn parse_status(out: &str, root: &Path) -> GitStatus {
    let mut fields = out.split('\0');
    let (branch, upstream, ahead, behind) = match fields.next() {
        Some(h) if h.starts_with("## ") => parse_branch_header(h),
        _ => (None, None, 0, 0),
    };
    let mut files = Vec::new();
    while let Some(field) = fields.next() {
        let mut chars = field.chars();
        let (Some(x), Some(y), Some(' ')) = (chars.next(), chars.next(), chars.next()) else {
            continue;
        };
        let path = chars.as_str();
        if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            fields.next();
        }
        let Some(code) = status_code(x, y) else { continue };
        files.push(GitFile {
            path: under_root(root, path.trim_end_matches('/')).to_string_lossy().into_owned(),
            code: code.to_string(),
            staged: x != ' ' && x != '?',
        });
    }
    GitStatus { root: root.to_string_lossy().into_owned(), branch, upstream, ahead, behind, files }
}

/// The index version of a file: what the editor's change gutter diffs the
/// buffer against. `None` when untracked, outside a repo, or not text.
#[tauri::command(async)]
pub fn fm_git_base(path: String) -> Option<String> {
    let (root, rel) = locate(&path)?;
    let spec = format!(":{}", git_path(&rel));
    let args = git_args(&root, &["show", &spec]);
    let bytes = run_bytes("git", &args, None, None, QUICK).ok()?;
    String::from_utf8(bytes).ok()
}

fn line_count(contents: &str) -> usize {
    contents.matches('\n').count() + usize::from(!contents.is_empty() && !contents.ends_with('\n'))
}

fn is_group_header(line: &str) -> Option<(&str, usize)> {
    let mut parts = line.split(' ');
    let sha = parts.next()?;
    if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let _orig = parts.next()?.parse::<usize>().ok()?;
    let fin = parts.next()?.parse::<usize>().ok()?;
    Some((sha, fin))
}

/// `git blame --porcelain` prints, per line of the file, a `sha orig final
/// [count]` header, the commit's metadata keys the first time that sha shows
/// up, and the line itself behind a TAB. Metadata is not repeated, so commits
/// are collected on first sight and every later line just points at them.
fn parse_blame(out: &str, contents: &str) -> GitBlame {
    let mut commits: Vec<GitCommit> = Vec::new();
    let mut lines = vec![0u32; line_count(contents)];
    let mut index_of = std::collections::HashMap::<String, u32>::new();
    let mut current: Option<u32> = None;
    for line in out.lines() {
        if line.starts_with('\t') {
            current = None;
            continue;
        }
        if let Some((sha, fin)) = is_group_header(line) {
            let idx = *index_of.entry(sha.to_string()).or_insert_with(|| {
                let uncommitted = sha == UNCOMMITTED_SHA;
                commits.push(GitCommit {
                    sha: sha.to_string(),
                    author: if uncommitted { "You".into() } else { String::new() },
                    email: String::new(),
                    time: 0,
                    summary: if uncommitted { "Uncommitted changes".into() } else { String::new() },
                    uncommitted,
                });
                commits.len() as u32 - 1
            });
            if let Some(slot) = fin.checked_sub(1).and_then(|i| lines.get_mut(i)) {
                *slot = idx;
            }
            current = Some(idx);
            continue;
        }
        let Some(commit) = current.and_then(|i| commits.get_mut(i as usize)) else { continue };
        if commit.uncommitted {
            continue;
        }
        if let Some(v) = line.strip_prefix("author ") {
            commit.author = v.to_string();
        } else if let Some(v) = line.strip_prefix("author-mail ") {
            commit.email = v.trim_start_matches('<').trim_end_matches('>').to_string();
        } else if let Some(v) = line.strip_prefix("author-time ") {
            commit.time = v.parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("summary ") {
            commit.summary = v.to_string();
        }
    }
    if commits.is_empty() {
        return GitBlame::default();
    }
    GitBlame { commits, lines }
}

/// Who last changed each line of `contents`. The unsaved buffer goes in on
/// stdin, so lines edited since the last save blame as uncommitted, the way
/// GitLens shows them. Untracked files and non-repos give an empty blame.
#[tauri::command(async)]
pub fn fm_git_blame(path: String, contents: String) -> Result<GitBlame, String> {
    let Some((root, rel)) = locate(&path) else { return Ok(GitBlame::default()) };
    let rel = git_path(&rel);
    let tracked = git_args(&root, &["ls-files", "--error-unmatch", "--", &rel]);
    if run("git", &tracked, None, None, QUICK).is_err() {
        return Ok(GitBlame::default());
    }
    let args = git_args(&root, &["blame", "--porcelain", "--contents", "-", "--", &rel]);
    match run("git", &args, None, Some(contents.as_bytes()), BLAME) {
        Ok(out) => Ok(parse_blame(&out, &contents)),
        // Added to the index but never committed, or a repo with no commits.
        Err(msg) if msg.contains("no such path") || msg.contains("no such ref") => Ok(GitBlame::default()),
        Err(msg) => Err(msg),
    }
}

/// Records are `\x1e sha \x1f author \x1f email \x1f time \x1f subject`, then
/// a blank line and the file's name at that commit. Merges list no file, so
/// the current name stands in.
fn parse_log(out: &str, fallback_path: &str) -> Vec<GitLogEntry> {
    out.split('\x1e')
        .filter_map(|record| {
            let mut lines = record.lines();
            let mut head = lines.next()?.split('\x1f');
            let sha = head.next()?.to_string();
            if sha.is_empty() {
                return None;
            }
            let author = head.next()?.to_string();
            let email = head.next()?.to_string();
            let time = head.next()?.parse().ok()?;
            let summary = head.next().unwrap_or("").to_string();
            let path = lines.find(|l| !l.is_empty()).unwrap_or(fallback_path).to_string();
            Some(GitLogEntry { sha, author, email, time, summary, path })
        })
        .collect()
}

/// The commits that touched a file, newest first, following renames.
#[tauri::command(async)]
pub fn fm_git_log(path: String) -> Result<Vec<GitLogEntry>, String> {
    let Some((root, rel)) = locate(&path) else { return Ok(vec![]) };
    let rel = git_path(&rel);
    let args = git_args(
        &root,
        &[
            "-c",
            "core.quotepath=off",
            "log",
            "--follow",
            "-n",
            LOG_LIMIT,
            "--format=%x1e%H%x1f%an%x1f%ae%x1f%at%x1f%s",
            "--name-only",
            "--",
            &rel,
        ],
    );
    match run("git", &args, None, None, QUICK) {
        Ok(out) => Ok(parse_log(&out, &rel)),
        // A repo with no commits yet has no history to show.
        Err(msg) if msg.contains("does not have any commits") => Ok(vec![]),
        Err(msg) => Err(msg),
    }
}

fn show_text(root: &Path, spec: &str) -> Result<String, String> {
    let args = git_args(root, &["show", spec]);
    let bytes = run_bytes("git", &args, None, None, QUICK)?;
    String::from_utf8(bytes).map_err(|_| "binary file".to_string())
}

/// A file before and after one commit, for the history diff. `path` only
/// locates the repository; `rel` is the log entry's path at that commit.
#[tauri::command(async)]
pub fn fm_git_show(path: String, sha: String, rel: String) -> Result<GitShow, String> {
    if !(4..=64).contains(&sha.len()) || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("not a commit id".to_string());
    }
    if rel.is_empty() || rel.starts_with('-') {
        return Err("not a file path".to_string());
    }
    let (root, _) = locate(&path).ok_or_else(|| "not in a git repository".to_string())?;
    // Empty before: the file is new in this commit (or this is the root
    // commit). Empty after: this commit deleted it.
    let before = show_text(&root, &format!("{sha}^:{rel}"));
    let after = show_text(&root, &format!("{sha}:{rel}"));
    match (before, after) {
        (Err(e), _) | (_, Err(e)) if e == "binary file" => Err(e),
        (Err(_), Err(e)) => Err(e),
        (before, after) => Ok(GitShow { before: before.unwrap_or_default(), after: after.unwrap_or_default() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_with_upstream_and_counts() {
        let h = parse_branch_header("## main...origin/main [ahead 1, behind 2]");
        assert_eq!(h, (Some("main".into()), Some("origin/main".into()), 1, 2));
        let h = parse_branch_header("## dev...origin/dev [behind 4]");
        assert_eq!(h, (Some("dev".into()), Some("origin/dev".into()), 0, 4));
        let h = parse_branch_header("## dev...origin/dev [gone]");
        assert_eq!(h, (Some("dev".into()), Some("origin/dev".into()), 0, 0));
    }

    #[test]
    fn header_without_upstream_unborn_and_detached() {
        assert_eq!(parse_branch_header("## main"), (Some("main".into()), None, 0, 0));
        assert_eq!(parse_branch_header("## No commits yet on main"), (Some("main".into()), None, 0, 0));
        assert_eq!(parse_branch_header("## HEAD (no branch)"), (None, None, 0, 0));
    }

    #[test]
    fn status_entries_cover_renames_untracked_dirs_and_odd_names() {
        let out = "## main...origin/main [ahead 1]\0\
                   M  staged.rs\0\
                   \u{20}M worktree.rs\0\
                   MM both.rs\0\
                   R  new name.rs\0old name.rs\0\
                   ?? notes/\0\
                   ?? ünï cödé.txt\0\
                   UU clash.rs\0\
                   !! ignored.log\0\
                   A  added.rs\0\
                   \u{20}D gone.rs\0";
        let s = parse_status(out, Path::new("/repo"));
        assert_eq!((s.branch.as_deref(), s.upstream.as_deref(), s.ahead, s.behind), (Some("main"), Some("origin/main"), 1, 0));
        let got: Vec<(&str, &str, bool)> = s.files.iter().map(|f| (f.path.as_str(), f.code.as_str(), f.staged)).collect();
        // `/repo/x` on Unix; Windows joins with its own separator.
        let p = |name: &str| format!("/repo{}{name}", std::path::MAIN_SEPARATOR);
        let want = [
            (p("staged.rs"), "M", true),
            (p("worktree.rs"), "M", false),
            (p("both.rs"), "M", true),
            (p("new name.rs"), "R", true),
            (p("notes"), "U", false),
            (p("ünï cödé.txt"), "U", false),
            (p("clash.rs"), "C", true),
            (p("added.rs"), "A", true),
            (p("gone.rs"), "D", false),
        ];
        assert_eq!(got, want.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect::<Vec<_>>());
    }

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn blame_reuses_commits_and_marks_uncommitted_lines() {
        let out = format!(
            "{SHA_A} 1 1 2\nauthor Ada\nauthor-mail <ada@x.org>\nauthor-time 1700000000\nauthor-tz +0000\nsummary First\nfilename f.txt\n\tone\n\
             {SHA_A} 2 2\n\ttwo\n\
             {UNCOMMITTED_SHA} 3 3 1\nauthor Not Committed Yet\nauthor-mail <not.committed.yet>\nauthor-time 1700000500\nsummary Version of f.txt from f.txt\nfilename f.txt\n\tthree\n"
        );
        let b = parse_blame(&out, "one\ntwo\nthree");
        assert_eq!(b.lines, vec![0, 0, 1]);
        assert_eq!(b.commits.len(), 2);
        assert_eq!((b.commits[0].author.as_str(), b.commits[0].email.as_str(), b.commits[0].time), ("Ada", "ada@x.org", 1_700_000_000));
        assert_eq!(b.commits[0].summary, "First");
        assert!(!b.commits[0].uncommitted);
        assert!(b.commits[1].uncommitted);
        assert_eq!((b.commits[1].author.as_str(), b.commits[1].summary.as_str()), ("You", "Uncommitted changes"));
    }

    #[test]
    fn blame_of_nothing_is_empty() {
        assert_eq!(parse_blame("", "a\nb\n"), GitBlame::default());
        assert_eq!(line_count("a\nb\n"), 2);
        assert_eq!(line_count("a\nb"), 2);
        assert_eq!(line_count(""), 0);
    }

    #[test]
    fn log_records_carry_the_path_at_that_commit() {
        let out = "\x1eabc123\x1fAda\x1fada@x.org\x1f1700000000\x1fRename it\n\nsrc/new.rs\n\
                   \x1edef456\x1fBo\x1fbo@x.org\x1f1690000000\x1fFix: a | b\n\nsrc/old.rs\n\
                   \x1e789abc\x1fCy\x1fcy@x.org\x1f1680000000\x1fMerge\n";
        let log = parse_log(out, "src/new.rs");
        assert_eq!(log.len(), 3);
        assert_eq!((log[0].sha.as_str(), log[0].path.as_str(), log[0].time), ("abc123", "src/new.rs", 1_700_000_000));
        assert_eq!((log[1].summary.as_str(), log[1].path.as_str()), ("Fix: a | b", "src/old.rs"));
        assert_eq!(log[2].path, "src/new.rs");
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn repo_end_to_end() {
        if !installed("git") {
            return;
        }
        let dir = canonical(&std::env::temp_dir()).unwrap().join(format!("devspro-git-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        let file = dir.join("a.txt");
        std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
        git(&dir, &["add", "a.txt"]);
        git(&dir, &["commit", "-q", "-m", "First"]);
        std::fs::write(&file, "one\nTWO\nthree\n").unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        let (file_s, dir_s) = (file.to_string_lossy().into_owned(), dir.to_string_lossy().into_owned());

        let status = fm_git_status(dir_s.clone()).expect("status");
        assert_eq!(status.root, dir_s);
        assert!(status.branch.is_some());
        let got: Vec<(String, &str, bool)> = status.files.iter().map(|f| (f.path.clone(), f.code.as_str(), f.staged)).collect();
        assert!(got.contains(&(file_s.clone(), "M", false)), "{got:?}");
        assert!(got.contains(&(dir.join("b.txt").to_string_lossy().into_owned(), "U", false)), "{got:?}");

        assert_eq!(fm_git_base(file_s.clone()).as_deref(), Some("one\ntwo\nthree\n"));
        assert_eq!(fm_git_base(dir.join("b.txt").to_string_lossy().into_owned()), None);

        let blame = fm_git_blame(file_s.clone(), "one\nTWO\nthree\n".into()).unwrap();
        assert_eq!(blame.lines.len(), 3);
        assert_eq!(blame.lines[0], blame.lines[2]);
        assert_ne!(blame.lines[0], blame.lines[1]);
        assert!(blame.commits[blame.lines[1] as usize].uncommitted);
        assert_eq!(blame.commits[blame.lines[0] as usize].summary, "First");
        assert_eq!(blame.commits[blame.lines[0] as usize].email, "test@example.com");
        let untracked = fm_git_blame(dir.join("b.txt").to_string_lossy().into_owned(), "new\n".into()).unwrap();
        assert_eq!(untracked, GitBlame::default());

        let log = fm_git_log(file_s.clone()).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!((log[0].summary.as_str(), log[0].path.as_str(), log[0].author.as_str()), ("First", "a.txt", "Test"));

        let shown = fm_git_show(file_s.clone(), log[0].sha.clone(), log[0].path.clone()).unwrap();
        assert_eq!(shown, GitShow { before: String::new(), after: "one\ntwo\nthree\n".into() });
        assert!(fm_git_show(file_s.clone(), "--help".into(), "a.txt".into()).is_err());
        assert!(fm_git_show(file_s, log[0].sha.clone(), "-x".into()).is_err());

        assert!(fm_git_status(std::env::temp_dir().join("devspro-no-such-dir").to_string_lossy().into_owned()).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
