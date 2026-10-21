//! Storage map tab: a size tree of a folder and a breakdown by file extension.
//!
//! The walk follows `measure::disk_usage`: allocated blocks, hard links once,
//! one filesystem, no symlinks. The tree sent to the frontend is pruned so a
//! whole disk stays a small payload.

use super::measure::Scan;
use super::CleanerState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use crate::modules::fs_compat::MetadataExt;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

/// A directory keeps at most this many children; the rest fold together.
const MAX_CHILDREN: usize = 200;
/// A child smaller than this share of the total (0.05%) is folded.
const MIN_SHARE: f64 = 0.0005;
/// Extensions listed on their own; the rest go into "other".
const MAX_EXTENSIONS: usize = 50;
/// Entries walked before the scan stops and reports `truncated`.
const MAX_ENTRIES: u64 = 3_000_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub files: u64,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtStat {
    pub ext: String,
    pub kind: String,
    pub bytes: u64,
    pub files: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageTree {
    pub root: TreeNode,
    pub extensions: Vec<ExtStat>,
    pub total_bytes: u64,
    pub files: u64,
    /// Cancelled or cut off before it finished.
    pub truncated: bool,
}

#[tauri::command(async)]
pub fn cleaner_scan_tree(
    app: AppHandle,
    state: State<CleanerState>,
    scan_id: String,
    root: String,
) -> Result<StorageTree, String> {
    let scan = Scan::start(&state, &app, &scan_id, "storage-map");
    scan_tree(Path::new(&root), &scan, MAX_ENTRIES)
}

/// A directory being walked. Children are folded as they arrive, so memory
/// stays bounded by the walk's depth, not by how many entries are on disk.
struct Frame {
    name: String,
    path: PathBuf,
    /// Its own blocks, then everything below.
    bytes: u64,
    files: u64,
    read: Option<fs::ReadDir>,
    kids: Vec<TreeNode>,
    folded: Folded,
}

#[derive(Default)]
struct Folded {
    items: u64,
    bytes: u64,
    files: u64,
}

impl Frame {
    fn new(name: String, path: PathBuf, bytes: u64) -> Frame {
        let read = fs::read_dir(&path).ok();
        Frame { name, path, bytes, files: 0, read, kids: vec![], folded: Folded::default() }
    }

    /// Keeps the `MAX_CHILDREN` biggest children and folds the rest.
    fn trim(&mut self) {
        if self.kids.len() <= MAX_CHILDREN {
            return;
        }
        self.kids.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        for k in self.kids.split_off(MAX_CHILDREN) {
            self.folded.items += 1;
            self.folded.bytes += k.bytes;
            self.folded.files += k.files;
        }
    }

    fn finish(mut self) -> TreeNode {
        self.trim();
        let mut children = self.kids;
        children.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        if self.folded.items > 0 {
            children.push(folded_node(&self.folded));
        }
        TreeNode {
            name: self.name,
            path: self.path.to_string_lossy().into_owned(),
            bytes: self.bytes,
            files: self.files,
            is_dir: true,
            children,
        }
    }
}

fn folded_node(f: &Folded) -> TreeNode {
    TreeNode {
        name: format!("{} smaller items", f.items),
        path: String::new(),
        bytes: f.bytes,
        files: f.files,
        is_dir: false,
        children: vec![],
    }
}

fn scan_tree(root: &Path, scan: &Scan, max_entries: u64) -> Result<StorageTree, String> {
    if !root.is_absolute() {
        return Err(format!("{} is not an absolute path", root.display()));
    }
    let meta = fs::metadata(root).map_err(|e| format!("{}: {e}", root.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    fs::read_dir(root).map_err(|e| format!("{}: {e}", root.display()))?;

    let dev = meta.dev();
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut exts: HashMap<String, (u64, u64)> = HashMap::new();
    let root_name = root.file_name().map_or_else(|| "/".to_string(), |n| n.to_string_lossy().into_owned());
    let mut frames = vec![Frame::new(root_name, root.to_path_buf(), meta.blocks() * 512)];
    let mut entries = 1u64;
    let mut stop = false;
    scan.tick(1, meta.blocks() * 512, root);

    let root_node = loop {
        let next = if stop { None } else { frames.last_mut().and_then(|f| f.read.as_mut()).and_then(|r| r.next()) };
        let Some(next) = next else {
            // This directory is done: fold it and hand it to its parent.
            let done = frames.pop().ok_or("empty scan")?;
            let node = done.finish();
            let Some(parent) = frames.last_mut() else { break node };
            parent.bytes += node.bytes;
            parent.files += node.files;
            parent.kids.push(node);
            if parent.kids.len() >= MAX_CHILDREN * 4 {
                parent.trim();
            }
            continue;
        };
        let Ok(entry) = next else { continue };
        if scan.is_cancelled() || entries >= max_entries {
            stop = true;
            continue;
        }
        let path = entry.path();
        let Ok(m) = fs::symlink_metadata(&path) else { continue };
        if m.dev() != dev {
            continue;
        }
        if m.nlink() > 1 && !m.is_dir() && !seen.insert((m.dev(), m.ino())) {
            continue;
        }
        let size = m.blocks() * 512;
        entries += 1;
        scan.tick(1, size, &path);
        let name = entry.file_name();
        if m.is_dir() {
            frames.push(Frame::new(name.to_string_lossy().into_owned(), path, size));
        } else {
            let e = exts.entry(extension_of(&name)).or_default();
            e.0 += size;
            e.1 += 1;
            let Some(top) = frames.last_mut() else { continue };
            top.bytes += size;
            top.files += 1;
            top.kids.push(TreeNode {
                name: name.to_string_lossy().into_owned(),
                path: path.to_string_lossy().into_owned(),
                bytes: size,
                files: 1,
                is_dir: false,
                children: vec![],
            });
            if top.kids.len() >= MAX_CHILDREN * 4 {
                top.trim();
            }
        }
    };

    let total = root_node.bytes;
    let files = root_node.files;
    let mut root_node = root_node;
    drop_small(&mut root_node, (total as f64 * MIN_SHARE) as u64);
    Ok(StorageTree { root: root_node, extensions: ext_stats(exts), total_bytes: total, files, truncated: stop })
}

/// The final pass: children under `min` bytes fold into the directory's
/// "N smaller items" node, which is created or grown as needed.
fn drop_small(node: &mut TreeNode, min: u64) {
    let mut folded = Folded::default();
    let mut kept = Vec::with_capacity(node.children.len());
    for mut k in std::mem::take(&mut node.children) {
        if k.path.is_empty() && !k.is_dir {
            folded.items += k.name.split(' ').next().and_then(|n| n.parse::<u64>().ok()).unwrap_or(1);
            folded.bytes += k.bytes;
            folded.files += k.files;
        } else if k.bytes == 0 || k.bytes < min {
            folded.items += 1;
            folded.bytes += k.bytes;
            folded.files += k.files;
        } else {
            drop_small(&mut k, min);
            kept.push(k);
        }
    }
    if folded.items > 0 {
        kept.push(folded_node(&folded));
    }
    node.children = kept;
}

fn extension_of(name: &std::ffi::OsStr) -> String {
    let name = name.to_string_lossy();
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => name[i + 1..].to_lowercase(),
        _ => "(none)".to_string(),
    }
}

fn ext_stats(exts: HashMap<String, (u64, u64)>) -> Vec<ExtStat> {
    let mut all: Vec<(String, (u64, u64))> = exts.into_iter().collect();
    all.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then_with(|| a.0.cmp(&b.0)));
    let rest = all.split_off(all.len().min(MAX_EXTENSIONS));
    let mut out: Vec<ExtStat> = all
        .into_iter()
        .map(|(ext, (bytes, files))| ExtStat { kind: kind_of(&ext).to_string(), ext, bytes, files })
        .collect();
    if !rest.is_empty() {
        out.push(ExtStat {
            ext: "other".to_string(),
            kind: "other".to_string(),
            bytes: rest.iter().map(|r| r.1 .0).sum(),
            files: rest.iter().map(|r| r.1 .1).sum(),
        });
    }
    out
}

fn kind_of(ext: &str) -> &'static str {
    match ext {
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv" | "m4v" | "mpg" | "mpeg" => "video",
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "tif" | "tiff" | "heic" | "raw" | "cr2"
        | "nef" | "ico" | "psd" => "image",
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" | "opus" | "wma" => "audio",
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "deb" | "rpm" | "jar" => "archive",
        "iso" | "img" | "vmdk" | "qcow2" | "vdi" | "vhd" | "vhdx" => "disk-image",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "txt" | "md" | "epub"
        | "rtf" | "csv" => "document",
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "py" | "go" | "java" | "c" | "h" | "cpp" | "hpp" | "cs"
        | "rb" | "php" | "sh" | "html" | "css" | "json" | "yaml" | "yml" | "toml" | "vue" | "kt" | "swift" => "code",
        "sql" | "dump" | "bak" | "sqlite" | "sqlite3" | "db" => "db-dump",
        "log" => "log",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("devspro-storage-map-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn write(path: &Path, len: usize) {
        fs::File::create(path).unwrap().write_all(&vec![7u8; len]).unwrap();
    }

    #[test]
    fn rejects_relative_missing_and_file_roots() {
        let d = scratch("reject");
        write(&d.join("f"), 10);
        assert!(scan_tree(Path::new("relative"), &Scan::detached(), 100).is_err());
        assert!(scan_tree(&d.join("missing"), &Scan::detached(), 100).is_err());
        assert!(scan_tree(&d.join("f"), &Scan::detached(), 100).is_err());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn sizes_add_up_and_extensions_are_counted() {
        let d = scratch("sizes");
        fs::create_dir_all(d.join("sub")).unwrap();
        write(&d.join("a.MP4"), 400_000);
        write(&d.join("sub/b.mp4"), 300_000);
        write(&d.join("sub/c.log"), 200_000);
        let t = scan_tree(&d, &Scan::detached(), 1000).unwrap();
        assert!(!t.truncated);
        assert_eq!(t.files, 3);
        assert_eq!(t.root.bytes, t.total_bytes);
        let sub = t.root.children.iter().find(|c| c.name == "sub").unwrap();
        assert!(sub.is_dir && sub.files == 2);
        assert!(sub.children.iter().any(|c| c.path == d.join("sub/b.mp4").to_string_lossy()));
        let mp4 = t.extensions.iter().find(|e| e.ext == "mp4").unwrap();
        assert_eq!((mp4.files, mp4.kind.as_str()), (2, "video"));
        assert!(mp4.bytes >= 700_000);
        assert_eq!(t.extensions.iter().find(|e| e.ext == "log").unwrap().kind, "log");
        assert_eq!(t.extensions[0].ext, "mp4", "largest first");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn small_children_fold_into_one_node() {
        let d = scratch("fold");
        write(&d.join("big.bin"), 50_000_000);
        for i in 0..5 {
            write(&d.join(format!("tiny{i}.txt")), 10);
        }
        let t = scan_tree(&d, &Scan::detached(), 1000).unwrap();
        let names: Vec<&str> = t.root.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["big.bin", "5 smaller items"]);
        let folded = &t.root.children[1];
        assert!(!folded.is_dir && folded.files == 5 && folded.path.is_empty());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn keeps_at_most_two_hundred_children() {
        let d = scratch("cap");
        for i in 0..230 {
            write(&d.join(format!("f{i:03}.bin")), 8192);
        }
        let t = scan_tree(&d, &Scan::detached(), 1000).unwrap();
        assert_eq!(t.root.children.len(), MAX_CHILDREN + 1);
        assert_eq!(t.root.children.last().unwrap().name, "30 smaller items");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_directory_with_many_children_folds_while_walking() {
        let d = scratch("many");
        write(&d.join("big.bin"), 5_000_000);
        for i in 0..1000 {
            write(&d.join(format!("s{i:04}.dat")), 4096);
        }
        let t = scan_tree(&d, &Scan::detached(), 10_000).unwrap();
        assert_eq!(t.files, 1001);
        assert!(t.root.children.len() <= MAX_CHILDREN + 1);
        assert_eq!(t.root.children[0].name, "big.bin");
        let folded = t.root.children.last().unwrap();
        assert!(folded.name.ends_with("smaller items") && !folded.is_dir && folded.path.is_empty());
        let kept: u64 = t.root.children.iter().map(|c| c.files).sum();
        assert_eq!(kept, 1001, "nothing is lost, only folded");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn extension_stats_cap_at_fifty_plus_other() {
        let d = scratch("exts");
        for i in 0..60 {
            write(&d.join(format!("f.e{i:02}")), 4096 * (i + 1));
        }
        let t = scan_tree(&d, &Scan::detached(), 1000).unwrap();
        assert_eq!(t.extensions.len(), MAX_EXTENSIONS + 1);
        let other = t.extensions.last().unwrap();
        assert_eq!((other.ext.as_str(), other.kind.as_str(), other.files), ("other", "other", 10));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn symlinks_are_not_followed() {
        let d = scratch("symlink");
        let outside = scratch("symlink-outside");
        write(&outside.join("big.bin"), 1_000_000);
        std::os::unix::fs::symlink(&outside, d.join("link")).unwrap();
        let t = scan_tree(&d, &Scan::detached(), 1000).unwrap();
        // The link itself counts as one small entry; what it points at does not.
        assert_eq!(t.files, 1);
        assert!(t.total_bytes < 1_000_000);
        assert!(t.extensions.iter().all(|e| e.ext != "bin"));
        fs::remove_dir_all(&d).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn hard_links_count_once() {
        let d = scratch("hardlink");
        write(&d.join("a.bin"), 100_000);
        fs::hard_link(d.join("a.bin"), d.join("b.bin")).unwrap();
        let t = scan_tree(&d, &Scan::detached(), 1000).unwrap();
        assert_eq!(t.files, 1);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn cancel_truncates() {
        let d = scratch("cancel");
        write(&d.join("a.bin"), 10);
        let scan = Scan::detached();
        scan.cancel();
        let t = scan_tree(&d, &scan, 1000).unwrap();
        assert!(t.truncated);
        assert_eq!(t.files, 0);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn hitting_the_entry_cap_truncates() {
        let d = scratch("entry-cap");
        for i in 0..10 {
            write(&d.join(format!("f{i}")), 10);
        }
        let t = scan_tree(&d, &Scan::detached(), 5).unwrap();
        assert!(t.truncated);
        assert!(t.files < 10);
        fs::remove_dir_all(&d).unwrap();
    }
}
