//! Devices: USB drives and memory cards, phones and cameras, Google Drive
//! accounts and network shares, as the desktop's own file manager lists them.
//!
//! `gio mount -li` is the source. It reports what GVfs's volume monitors see
//! (UDisks2 for block devices, MTP and gPhoto2 for phones and cameras, GNOME
//! Online Accounts for Drive) whether mounted or not, so an unplugged-then-
//! replugged phone shows up here and mounts on a click, exactly as in Files.
//! Without `gio` the list falls back to what is already mounted under
//! `/media`, `/run/media` and the gvfs bridge.
//!
//! Everything stays at user level: GVfs and UDisks2 decide (through polkit)
//! what a desktop user may mount. Nothing here runs elevated.

use super::gvfs::{gio, gvfs_root};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Device {
    /// Stable across mounting and unmounting, for the sidebar's keys.
    pub id: String,
    pub label: String,
    /// One of `usb`, `drive`, `phone`, `camera`, `disc`, `cloud`, `network` —
    /// the frontend maps it to an icon and a sidebar group.
    pub kind: String,
    /// The folder its files are in. None until it is mounted.
    pub path: Option<String>,
    /// What `fm_device_action("mount", …)` takes; None once mounted or when
    /// it cannot be mounted from here.
    pub mount: Option<String>,
    /// What `fm_device_action("unmount", …)` takes.
    pub unmount: Option<String>,
    /// What `fm_device_action("eject", …)` takes: eject the medium, or stop
    /// (power down) the drive so it can be unplugged safely.
    pub eject: Option<String>,
}

const MOUNT_TIMEOUT: Duration = Duration::from_secs(60);
const LIST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum NodeKind {
    #[default]
    Drive,
    Volume,
    Mount,
}

/// One `Drive(n)`, `Volume(n)` or `Mount(n)` block of `gio mount -li`.
#[derive(Debug, Default)]
struct Node {
    kind: NodeKind,
    indent: usize,
    parent: Option<usize>,
    name: String,
    /// A mount's root, from `Mount(0): NAME -> URI`.
    uri: Option<String>,
    unix_device: Option<String>,
    icons: String,
    props: Vec<(String, String)>,
}

impl Node {
    fn prop(&self, key: &str) -> Option<&str> {
        self.props.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    fn flag(&self, key: &str) -> bool {
        self.prop(key) == Some("1")
    }
}

/// The blocks of `gio mount -li`, each knowing its enclosing block by
/// indentation: drives hold volumes, volumes hold mounts, and volumes and
/// mounts with no drive (Drive accounts, phones, network shares) sit at the
/// top level.
fn parse_tree(text: &str) -> Vec<Node> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - trimmed.len();
        while stack.last().is_some_and(|&i| nodes[i].indent >= indent) {
            stack.pop();
        }
        if let Some((kind, rest)) = header(trimmed) {
            let (name, uri) = match kind {
                NodeKind::Mount => match rest.rsplit_once(" -> ") {
                    Some((n, u)) => (n.to_string(), Some(u.to_string())),
                    None => (rest.to_string(), None),
                },
                _ => (rest.to_string(), None),
            };
            nodes.push(Node { kind, indent, parent: stack.last().copied(), name, uri, ..Node::default() });
            stack.push(nodes.len() - 1);
            continue;
        }
        let Some(&current) = stack.last() else { continue };
        let node = &mut nodes[current];
        if let Some(icons) = trimmed.strip_prefix("themed icons:") {
            node.icons = icons.trim().to_string();
        } else if let Some(dev) = trimmed.strip_prefix("unix-device: ") {
            node.unix_device = Some(dev.trim_matches('\'').to_string());
        } else if let Some((key, value)) = trimmed.split_once('=') {
            if !key.contains(' ') {
                node.props.push((key.to_string(), value.to_string()));
            }
        }
    }
    nodes
}

fn header(line: &str) -> Option<(NodeKind, &str)> {
    for (word, kind) in [("Drive(", NodeKind::Drive), ("Volume(", NodeKind::Volume), ("Mount(", NodeKind::Mount)] {
        if let Some(rest) = line.strip_prefix(word) {
            let (_n, rest) = rest.split_once("): ")?;
            return Some((kind, rest));
        }
    }
    None
}

/// The sidebar's kind for a volume or mount, from its location's scheme and
/// failing that the icon GVfs gives it.
fn classify(uri: &str, icons: &str) -> &'static str {
    let scheme = uri.split_once("://").map(|(s, _)| s).unwrap_or("");
    match scheme {
        "google-drive" | "onedrive" => return "cloud",
        "mtp" | "afc" => return "phone",
        "gphoto2" => return "camera",
        "cdda" | "burn" => return "disc",
        "smb" | "sftp" | "ssh" | "ftp" | "ftps" | "dav" | "davs" | "nfs" | "afp" | "network" => return "network",
        _ => {}
    }
    if icons.contains("phone") || icons.contains("multimedia-player") {
        "phone"
    } else if icons.contains("camera") {
        "camera"
    } else if icons.contains("optical") {
        "disc"
    } else if icons.contains("network") {
        "network"
    } else if icons.contains("usb") || icons.contains("removable") || icons.contains("flash") {
        "usb"
    } else {
        "drive"
    }
}

/// A device before its mount's local path is known: `root` is the mount's
/// URI when mounted.
#[derive(Debug, Clone, PartialEq)]
struct Found {
    device: Device,
    root: Option<String>,
}

/// Turns the parsed blocks into sidebar devices: every volume (mounted or
/// mountable), plus every top-level mount no volume accounts for. Mounts GVfs
/// marks as shadowed are the daemon's side of a volume already listed.
fn devices_from(nodes: &[Node]) -> Vec<Found> {
    let mut out = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        match node.kind {
            NodeKind::Volume => {
                let drive = node.parent.map(|p| &nodes[p]).filter(|p| p.kind == NodeKind::Drive);
                let mount = nodes.iter().find(|m| m.kind == NodeKind::Mount && m.parent == Some(i));
                let activation = node.prop("activation_root");
                let uri = mount.and_then(|m| m.uri.as_deref()).or(activation).unwrap_or("");
                let icons = if node.icons.is_empty() { drive.map(|d| d.icons.as_str()).unwrap_or("") } else { &node.icons };
                let mount_target = if mount.is_some() || !node.flag("can_mount") {
                    None
                } else if let Some(dev) = &node.unix_device {
                    Some(format!("device:{dev}"))
                } else {
                    activation.map(|a| format!("uri:{a}"))
                };
                if mount.is_none() && mount_target.is_none() {
                    continue;
                }
                let root = mount.and_then(|m| m.uri.clone());
                let eject = match (mount, drive) {
                    (Some(m), _) if m.flag("can_eject") || node.flag("can_eject") => m.uri.as_ref().map(|u| format!("uri:{u}")),
                    (_, Some(d)) if d.flag("can_stop") && d.flag("is_removable") => d.unix_device.as_ref().map(|dev| format!("device:{dev}")),
                    _ => None,
                };
                let id = node
                    .prop("uuid")
                    .map(str::to_string)
                    .or_else(|| node.unix_device.clone())
                    .or_else(|| activation.map(str::to_string))
                    .unwrap_or_else(|| format!("volume:{}", node.name));
                out.push(Found {
                    device: Device {
                        id,
                        label: node.name.clone(),
                        kind: classify(uri, icons).to_string(),
                        path: None,
                        mount: mount_target,
                        unmount: mount.filter(|m| m.flag("can_unmount")).and_then(|m| m.uri.as_ref()).map(|u| format!("uri:{u}")),
                        eject,
                    },
                    root,
                });
            }
            NodeKind::Mount if node.parent.is_none() && !node.flag("is_shadowed") => {
                let Some(uri) = node.uri.clone() else { continue };
                out.push(Found {
                    device: Device {
                        id: uri.clone(),
                        label: node.name.clone(),
                        kind: classify(&uri, &node.icons).to_string(),
                        path: None,
                        mount: None,
                        unmount: node.flag("can_unmount").then(|| format!("uri:{uri}")),
                        eject: node.flag("can_eject").then(|| format!("uri:{uri}")),
                    },
                    root: Some(uri),
                });
            }
            _ => {}
        }
    }
    out
}

/// `file:///media/me/My%20Stick` -> `/media/me/My Stick`.
fn file_uri_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok().filter(|p| p.starts_with('/'))
}

/// `gio info` prints a `uri:` line and, for anything on the fuse bridge, a
/// `local path:` line after it.
fn parse_local_paths(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut uri: Option<String> = None;
    for line in text.lines() {
        if let Some(u) = line.strip_prefix("uri: ") {
            uri = Some(u.trim().to_string());
        } else if let Some(p) = line.strip_prefix("local path: ") {
            if let Some(u) = uri.take() {
                out.push((u, p.trim().to_string()));
            }
        }
    }
    out
}

fn same_location(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// Fills in where each mounted device's files are: a `file://` root is its
/// own path; a GVfs root is asked of `gio info`, all in one call. A mount
/// with no local path (the fuse bridge is not running) cannot be browsed and
/// is dropped.
fn resolve_paths(found: Vec<Found>) -> Vec<Device> {
    let remote: Vec<&str> = found
        .iter()
        .filter_map(|f| f.root.as_deref())
        .filter(|r| !r.starts_with("file://"))
        .collect();
    let locals = if remote.is_empty() {
        Vec::new()
    } else {
        let mut args = vec!["info", "-a", "standard::name", "--"];
        args.extend(remote.iter().copied());
        // A share that went away makes gio fail for that one location only;
        // its stdout still has the others.
        gio(&args, LIST_TIMEOUT).map(|out| parse_local_paths(&out)).unwrap_or_default()
    };
    found
        .into_iter()
        .filter_map(|mut f| {
            if let Some(root) = &f.root {
                let path = file_uri_path(root).or_else(|| {
                    locals.iter().find(|(u, _)| same_location(u, root)).map(|(_, p)| p.clone())
                });
                path.as_ref()?;
                f.device.path = path;
            }
            Some(f.device)
        })
        .collect()
}

/// `/proc/self/mounts` escapes spaces and the like as `\040`.
fn unescape_mount_field(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            if let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 4]).ok().and_then(|o| u8::from_str_radix(o, 8).ok()) {
                out.push(b);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `google-drive:host=gmail.com,user=me` -> ("cloud", "me@gmail.com");
/// `mtp:host=Xiaomi_Redmi_Note_13_d243d212` -> ("phone", "Xiaomi Redmi Note 13 d243d212").
fn describe_gvfs_dir(name: &str) -> (&'static str, String) {
    let (backend, spec) = name.split_once(':').unwrap_or((name, ""));
    let field = |key: &str| {
        spec.split(',')
            .find_map(|kv| kv.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
            .map(|v| v.replace('_', " "))
    };
    let kind = match backend {
        "google-drive" | "onedrive" => "cloud",
        "mtp" | "afc" => "phone",
        "gphoto2" => "camera",
        _ => "network",
    };
    let label = match (field("user"), field("host"), field("share")) {
        (Some(user), Some(host), _) if kind == "cloud" => format!("{user}@{host}"),
        (_, _, Some(share)) => share,
        (_, Some(host), _) => host,
        _ => name.to_string(),
    };
    (kind, label)
}

/// Without `gio`: what is mounted under the removable-media folders, and the
/// GVfs bridge's mounts by their directory names.
fn fallback_devices(mounts: &str, gvfs_dirs: &[String], gvfs: &Path) -> Vec<Device> {
    let mut out = Vec::new();
    for line in mounts.lines() {
        let Some(point) = line.split_whitespace().nth(1).map(unescape_mount_field) else { continue };
        let removable = ["/media/", "/run/media/"].iter().any(|p| point.starts_with(p));
        if !removable {
            continue;
        }
        let label = Path::new(&point).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| point.clone());
        out.push(Device { id: point.clone(), label, kind: "usb".into(), path: Some(point), mount: None, unmount: None, eject: None });
    }
    for name in gvfs_dirs {
        let (kind, label) = describe_gvfs_dir(name);
        let path = gvfs.join(name).to_string_lossy().into_owned();
        out.push(Device { id: path.clone(), label, kind: kind.into(), path: Some(path), mount: None, unmount: None, eject: None });
    }
    out
}

#[tauri::command(async)]
pub fn fm_devices() -> Vec<Device> {
    match gio(&["mount", "-li"], LIST_TIMEOUT) {
        Ok(text) => resolve_paths(devices_from(&parse_tree(&text))),
        Err(_) => {
            let mounts = fs::read_to_string("/proc/self/mounts").unwrap_or_default();
            let gvfs = gvfs_root();
            let mut dirs: Vec<String> = fs::read_dir(&gvfs)
                .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
                .unwrap_or_default();
            dirs.sort();
            fallback_devices(&mounts, &dirs, &gvfs)
        }
    }
}

/// Mounts, unmounts or ejects one device, by the target string `fm_devices`
/// handed out for that verb. Waits for GVfs to finish, so the next listing
/// already shows the change. A phone that is locked or not in file-transfer
/// mode, or an encrypted drive, fails here with gio's own explanation.
#[tauri::command(async)]
pub fn fm_device_action(action: String, target: String) -> Result<String, String> {
    let (how, location) = target.split_once(':').ok_or_else(|| format!("unknown device target {target}"))?;
    if location.is_empty() || location.starts_with('-') {
        return Err(format!("unknown device target {target}"));
    }
    let args: Vec<String> = match (action.as_str(), how) {
        ("mount", "device") => vec!["mount".into(), format!("--device={location}")],
        ("mount", "uri") => vec!["mount".into(), "--".into(), location.into()],
        ("unmount", "uri") => vec!["mount".into(), "--unmount".into(), "--".into(), location.into()],
        ("eject", "uri") => vec!["mount".into(), "--eject".into(), "--".into(), location.into()],
        ("eject", "device") => vec!["mount".into(), format!("--stop={location}")],
        _ => return Err(format!("cannot {action} {target}")),
    };
    gio(&args, MOUNT_TIMEOUT)?;
    Ok(match action.as_str() {
        "mount" => "Mounted".into(),
        "unmount" => "Unmounted".into(),
        _ => "Safe to remove".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a real `gio mount -li`: a USB disk with a mounted
    /// partition, a Drive account, an unmounted phone, and a shadowed
    /// daemon mount for the Drive account.
    const SAMPLE: &str = "\
Drive(0): KINGSTON OM8PDP3512B-AI1
  Type: GProxyDrive (GProxyVolumeMonitorUDisks2)
  ids:
   unix-device: '/dev/nvme0n1'
  themed icons:  [drive-harddisk-solidstate]  [drive-harddisk]
  is_removable=0
  can_stop=0
Drive(1): TOSHIBA MQ04UBD200
  Type: GProxyDrive (GProxyVolumeMonitorUDisks2)
  ids:
   unix-device: '/dev/sda'
  themed icons:  [drive-harddisk-usb]  [drive-harddisk]  [drive]
  is_removable=1
  can_eject=0
  can_stop=1
  Volume(0): Ventoy
    Type: GProxyVolume (GProxyVolumeMonitorUDisks2)
    ids:
     class: 'device'
     unix-device: '/dev/sda1'
     uuid: '4E21-0000'
     label: 'Ventoy'
    uuid=4E21-0000
    themed icons:  [drive-harddisk-usb]  [drive-harddisk]
    can_mount=0
    can_eject=0
    Mount(0): Ventoy -> file:///media/me/Ventoy%20Disk
      Type: GProxyMount (GProxyVolumeMonitorUDisks2)
      default_location=file:///media/me/Ventoy%20Disk
      can_unmount=1
      can_eject=0
      is_shadowed=0
  Volume(1): VTOYEFI
    Type: GProxyVolume (GProxyVolumeMonitorUDisks2)
    ids:
     unix-device: '/dev/sda2'
    uuid=1234-ABCD
    themed icons:  [drive-harddisk-usb]
    can_mount=1
    can_eject=0
Volume(0): me@gmail.com
  Type: GProxyVolume (GProxyVolumeMonitorGoa)
  ids:
   class: 'network'
   uuid: 'google-drive://me@gmail.com/'
  uuid=google-drive://me@gmail.com/
  activation_root=google-drive://me@gmail.com/
  themed icons:  [network-server]  [network]
  can_mount=1
  can_eject=0
  Mount(0): me@gmail.com -> google-drive://me@gmail.com/
    Type: GProxyShadowMount (GProxyVolumeMonitorGoa)
    can_unmount=1
    can_eject=0
    is_shadowed=0
Volume(1): Redmi Note 13
  Type: GProxyVolume (GProxyVolumeMonitorMTP)
  ids:
   unix-device: '/dev/bus/usb/003/022'
  activation_root=mtp://Xiaomi_Redmi_Note_13_d243d212/
  themed icons:  [phone]
  can_mount=1
  can_eject=0
Mount(2): me@gmail.com -> google-drive://me@gmail.com/
  Type: GDaemonMount
  default_location=google-drive://me@gmail.com/0AF5
  can_unmount=1
  can_eject=0
  is_shadowed=1
Mount(3): share on nas -> smb://nas/share/
  Type: GDaemonMount
  can_unmount=1
  can_eject=0
  is_shadowed=0
";

    #[test]
    fn volumes_mounts_and_shares_become_devices() {
        let found = devices_from(&parse_tree(SAMPLE));
        let by_label = |l: &str| found.iter().find(|f| f.device.label == l).unwrap_or_else(|| panic!("no {l}"));
        assert_eq!(found.len(), 5, "{found:#?}");

        let ventoy = by_label("Ventoy");
        assert_eq!(ventoy.device.kind, "usb");
        assert_eq!(ventoy.device.id, "4E21-0000");
        assert_eq!(ventoy.root.as_deref(), Some("file:///media/me/Ventoy%20Disk"));
        assert_eq!(ventoy.device.mount, None);
        assert_eq!(ventoy.device.unmount.as_deref(), Some("uri:file:///media/me/Ventoy%20Disk"));
        // The disk cannot eject media, but it can be powered down to unplug.
        assert_eq!(ventoy.device.eject.as_deref(), Some("device:/dev/sda"));

        let efi = by_label("VTOYEFI");
        assert_eq!(efi.device.mount.as_deref(), Some("device:/dev/sda2"));
        assert_eq!(efi.root, None);

        let drive = by_label("me@gmail.com");
        assert_eq!(drive.device.kind, "cloud");
        assert_eq!(drive.root.as_deref(), Some("google-drive://me@gmail.com/"));
        assert_eq!(drive.device.eject, None);

        let phone = by_label("Redmi Note 13");
        assert_eq!(phone.device.kind, "phone");
        assert_eq!(phone.device.mount.as_deref(), Some("device:/dev/bus/usb/003/022"));

        let share = by_label("share on nas");
        assert_eq!(share.device.kind, "network");
        assert_eq!(share.device.unmount.as_deref(), Some("uri:smb://nas/share/"));
    }

    #[test]
    fn internal_disks_without_volumes_are_not_listed() {
        let found = devices_from(&parse_tree(SAMPLE));
        assert!(found.iter().all(|f| !f.device.label.contains("KINGSTON")));
    }

    #[test]
    fn file_uris_decode_to_paths() {
        assert_eq!(file_uri_path("file:///media/me/Ventoy%20Disk").as_deref(), Some("/media/me/Ventoy Disk"));
        assert_eq!(file_uri_path("file:///media/me/100%"), Some("/media/me/100%".into()));
        assert_eq!(file_uri_path("mtp://phone/"), None);
    }

    #[test]
    fn gio_info_local_paths_pair_with_their_uris() {
        let text = "display name: \nuri: mtp://Phone_1/\nlocal path: /run/user/1000/gvfs/mtp:host=Phone_1\n\
                    display name: me\nuri: google-drive://me@gmail.com/\nlocal path: /run/user/1000/gvfs/google-drive:host=gmail.com,user=me\n";
        let paths = parse_local_paths(text);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], ("mtp://Phone_1/".into(), "/run/user/1000/gvfs/mtp:host=Phone_1".into()));
        assert!(same_location("mtp://Phone_1", "mtp://Phone_1/"));
    }

    #[test]
    fn fallback_lists_removable_mounts_and_the_gvfs_bridge() {
        let mounts = "/dev/sda1 /media/me/My\\040Stick exfat rw 0 0\n/dev/nvme0n1p1 /boot/efi vfat rw 0 0\n";
        let dirs = vec!["google-drive:host=gmail.com,user=me".to_string(), "mtp:host=Xiaomi_Redmi_Note_13_d243d212".to_string()];
        let devices = fallback_devices(mounts, &dirs, Path::new("/run/user/1000/gvfs"));
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].label, "My Stick");
        assert_eq!(devices[0].path.as_deref(), Some("/media/me/My Stick"));
        assert_eq!((devices[1].kind.as_str(), devices[1].label.as_str()), ("cloud", "me@gmail.com"));
        assert_eq!((devices[2].kind.as_str(), devices[2].label.as_str()), ("phone", "Xiaomi Redmi Note 13 d243d212"));
    }

    #[test]
    fn device_actions_refuse_unknown_verbs_and_option_like_targets() {
        assert!(fm_device_action("format".into(), "device:/dev/sda".into()).is_err());
        assert!(fm_device_action("mount".into(), "uri:--help".into()).is_err());
        assert!(fm_device_action("mount".into(), "nonsense".into()).is_err());
    }
}
