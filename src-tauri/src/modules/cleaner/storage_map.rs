//! Storage map tab: a size tree of a folder and a breakdown by file extension.
//!
//! The walk follows `measure::disk_usage`: allocated blocks, hard links once,
//! one filesystem, no symlinks. The tree sent to the frontend is pruned so a
//! whole disk stays a small payload.

use super::measure::{walk_dirs, Scan};
use super::CleanerState;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
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

/// A directory's node while it is built. Children are folded as they arrive,
/// so a directory with a huge number of entries stays a small node.
struct Frame {
    name: String,
    path: PathBuf,
    /// Its own blocks, then everything below.
    bytes: u64,
    files: u64,
    kids: Vec<TreeNode>,
    folded: Folded,
}

#[derive(Default)]
struct Folded {
    items: u64,
    bytes: u64,
    files: u64,
}

impl Folded {
    fn add(&mut self, node: &TreeNode) {
        self.items += 1;
        self.bytes += node.bytes;
        self.files += node.files;
    }
}

impl Frame {
    fn new(name: String, path: PathBuf, bytes: u64) -> Frame {
        Frame { name, path, bytes, files: 0, kids: vec![], folded: Folded::default() }
    }

    /// Adds a child, folding the smallest away once there are many.
    fn push(&mut self, node: TreeNode) {
        self.bytes += node.bytes;
        self.files += node.files;
        self.kids.push(node);
        if self.kids.len() >= MAX_CHILDREN * 4 {
            self.trim();
        }
    }

    /// Keeps the `MAX_CHILDREN` biggest children and folds the rest.
    fn trim(&mut self) {
        if self.kids.len() <= MAX_CHILDREN {
            return;
        }
        self.kids.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        for k in self.kids.split_off(MAX_CHILDREN) {
            self.folded.add(&k);
        }
    }

    /// The finished node. Children under `min` bytes fold now, as the final
    /// pass would fold them anyway, and only the files kept get a path.
    fn finish(mut self, min: u64) -> TreeNode {
        let folded = &mut self.folded;
        self.kids.retain(|k| {
            let keep = k.bytes > 0 && k.bytes >= min;
            if !keep {
                folded.add(k);
            }
            keep
        });
        self.trim();
        let mut children = self.kids;
        children.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        for k in children.iter_mut().filter(|k| k.path.is_empty()) {
            k.path = self.path.join(&k.name).to_string_lossy().into_owned();
        }
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

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A directory whose subdirectories are still being read. The thread that
/// finishes the last of them finishes this one too and hands it up.
struct Pending {
    frame: Mutex<Option<Frame>>,
    left: AtomicUsize,
    parent: Option<Arc<Pending>>,
}

/// A directory to read, with what its parent's listing knows about it.
struct Dir {
    path: PathBuf,
    name: String,
    /// Its own blocks.
    own: u64,
    parent: Option<Arc<Pending>>,
}

impl AsRef<Path> for Dir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

/// Hard-linked files seen so far, split so threads rarely wait on each other.
const LINK_SHARDS: usize = 64;

/// What every walk thread shares.
struct Tree {
    dev: u64,
    max_entries: u64,
    entries: AtomicU64,
    /// Bytes counted so far. The total is at least this, so anything under its
    /// `MIN_SHARE` is folded in the end whatever else turns up.
    counted: AtomicU64,
    capped: AtomicBool,
    links: Vec<Mutex<HashSet<(u64, u64)>>>,
    root: Mutex<Option<TreeNode>>,
}

impl Tree {
    fn min(&self) -> u64 {
        (self.counted.load(Ordering::Relaxed) as f64 * MIN_SHARE) as u64
    }

    /// The first time this inode is seen, anywhere in the walk.
    fn first_link(&self, dev: u64, ino: u64) -> bool {
        lock(&self.links[ino as usize % LINK_SHARDS]).insert((dev, ino))
    }

    /// Hands a finished directory to its parent, and finishes each parent it
    /// was the last subdirectory of. The root ends up in `root`.
    fn hand_up(&self, mut node: TreeNode, mut parent: Option<Arc<Pending>>) {
        while let Some(p) = parent {
            lock(&p.frame).as_mut().expect("a pending directory finished early").push(node);
            if p.left.fetch_sub(1, Ordering::AcqRel) != 1 {
                return;
            }
            let frame = lock(&p.frame).take().expect("a directory finished twice");
            node = frame.finish(self.min());
            parent = p.parent.clone();
        }
        *lock(&self.root) = Some(node);
    }

    /// Reads one directory's entries: files into its node, subdirectories
    /// back to the walk. With none, the directory is finished right away.
    fn read(
        &self,
        scan: &Scan,
        exts: &mut HashMap<String, (u64, u64)>,
        dir: &Dir,
        list: &mut dyn Iterator<Item = fs::DirEntry>,
    ) -> Vec<Dir> {
        let mut frame = Frame::new(dir.name.clone(), dir.path.clone(), dir.own);
        let mut subdirs = Vec::new();
        let before = self.entries.load(Ordering::Relaxed);
        let (mut count, mut bytes) = (0, 0);
        for entry in list {
            if before + count >= self.max_entries {
                self.capped.store(true, Ordering::Relaxed);
                break;
            }
            // Relative to the open directory: no path lookup, no symlink followed.
            let Ok(m) = entry.metadata() else { continue };
            if m.dev() != self.dev {
                continue;
            }
            count += 1;
            let size = m.blocks() * 512;
            let name = entry.file_name().into_string().unwrap_or_else(|n| n.to_string_lossy().into_owned());
            if m.is_dir() {
                bytes += size;
                subdirs.push(Dir { path: entry.path(), name, own: size, parent: None });
                continue;
            }
            if m.nlink() > 1 && !self.first_link(m.dev(), m.ino()) {
                continue;
            }
            bytes += size;
            add_ext(exts, extension_of(&name), size);
            // The path is filled in by `finish`, for the files that are kept.
            frame.push(TreeNode { name, path: String::new(), bytes: size, files: 1, is_dir: false, children: vec![] });
        }
        self.entries.fetch_add(count, Ordering::Relaxed);
        self.counted.fetch_add(bytes, Ordering::Relaxed);
        scan.tick(count, bytes, &dir.path);
        if subdirs.is_empty() {
            self.hand_up(frame.finish(self.min()), dir.parent.clone());
            return subdirs;
        }
        let pending = Arc::new(Pending {
            frame: Mutex::new(Some(frame)),
            left: AtomicUsize::new(subdirs.len()),
            parent: dir.parent.clone(),
        });
        for sub in &mut subdirs {
            sub.parent = Some(pending.clone());
        }
        subdirs
    }
}

fn add_ext(exts: &mut HashMap<String, (u64, u64)>, ext: Cow<'_, str>, bytes: u64) {
    match exts.get_mut(ext.as_ref()) {
        Some(e) => {
            e.0 += bytes;
            e.1 += 1;
        }
        None => {
            exts.insert(ext.into_owned(), (bytes, 1));
        }
    }
}

/// The tree under `root`. Threads read directories in any order; each one is
/// finished and folded into its parent as soon as everything below it is in.
fn scan_tree(root: &Path, scan: &Scan, max_entries: u64) -> Result<StorageTree, String> {
    if !root.is_absolute() {
        return Err(format!("{} is not an absolute path", root.display()));
    }
    let meta = fs::metadata(root).map_err(|e| format!("{}: {e}", root.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    fs::read_dir(root).map_err(|e| format!("{}: {e}", root.display()))?;

    let own = meta.blocks() * 512;
    let tree = Tree {
        dev: meta.dev(),
        max_entries,
        entries: AtomicU64::new(1),
        counted: AtomicU64::new(own),
        capped: AtomicBool::new(false),
        links: (0..LINK_SHARDS).map(|_| Mutex::default()).collect(),
        root: Mutex::new(None),
    };
    scan.tick(1, own, root);
    let name = root.file_name().map_or_else(|| "/".to_string(), |n| n.to_string_lossy().into_owned());
    let start = Dir { path: root.to_path_buf(), name, own, parent: None };
    let (per_thread, unread) =
        walk_dirs(start, scan, HashMap::new, |exts, dir, list| tree.read(scan, exts, dir, list));
    // Cancelled: what was not read keeps its own size, so the rest still adds up.
    let truncated = !unread.is_empty() || tree.capped.load(Ordering::Relaxed);
    for dir in unread {
        tree.hand_up(Frame::new(dir.name, dir.path, dir.own).finish(tree.min()), dir.parent);
    }

    let mut exts: HashMap<String, (u64, u64)> = HashMap::new();
    for (ext, (bytes, files)) in per_thread.into_iter().flatten() {
        let e = exts.entry(ext).or_default();
        e.0 += bytes;
        e.1 += files;
    }
    let mut root_node = lock(&tree.root).take().ok_or("the scan did not finish")?;
    let total = root_node.bytes;
    let files = root_node.files;
    drop_small(&mut root_node, (total as f64 * MIN_SHARE) as u64);
    Ok(StorageTree { root: root_node, extensions: ext_stats(exts), total_bytes: total, files, truncated })
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

/// Lowercased, borrowed when it already is.
fn extension_of(name: &str) -> Cow<'_, str> {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => {
            let ext = &name[i + 1..];
            if ext.chars().any(char::is_uppercase) {
                Cow::Owned(ext.to_lowercase())
            } else {
                Cow::Borrowed(ext)
            }
        }
        _ => Cow::Borrowed("(none)"),
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
    fn a_deep_tree_matches_disk_usage_and_nests_every_folder() {
        let d = scratch("deep");
        for a in 0..5 {
            for b in 0..5 {
                let dir = d.join(format!("a{a}/b{b}"));
                fs::create_dir_all(&dir).unwrap();
                write(&dir.join("f.dat"), 5_000 * (a * 5 + b + 1));
            }
        }
        let t = scan_tree(&d, &Scan::detached(), 1000).unwrap();
        assert!(!t.truncated);
        assert_eq!(t.files, 25);
        assert_eq!(t.total_bytes, super::super::measure::disk_usage(&d, &Scan::detached()).bytes);
        let a4 = t.root.children.iter().find(|c| c.name == "a4").unwrap();
        assert!(a4.is_dir && a4.files == 5 && a4.path == d.join("a4").to_string_lossy());
        let b4 = a4.children.iter().find(|c| c.name == "b4").unwrap();
        assert_eq!(b4.children[0].path, d.join("a4/b4/f.dat").to_string_lossy());
        assert_eq!(t.root.children[0].name, "a4", "largest first");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_hard_link_in_two_folders_counts_once() {
        let d = scratch("hardlink-dirs");
        fs::create_dir_all(d.join("z/x")).unwrap();
        fs::create_dir_all(d.join("a/y")).unwrap();
        write(&d.join("z/x/big.bin"), 2_000_000);
        fs::hard_link(d.join("z/x/big.bin"), d.join("a/y/same.bin")).unwrap();
        for _ in 0..5 {
            let t = scan_tree(&d, &Scan::detached(), 1000).unwrap();
            assert_eq!(t.files, 1);
            let a = t.root.children.iter().find(|c| c.name == "a").unwrap();
            let z = t.root.children.iter().find(|c| c.name == "z").unwrap();
            assert_eq!(a.files + z.files, 1);
            assert_eq!(t.extensions.iter().find(|e| e.ext == "bin").unwrap().files, 1);
        }
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
