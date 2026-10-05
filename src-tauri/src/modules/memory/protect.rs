//! What the Memory module must never stop, decided in one place.
//!
//! Three kinds of process are off limits:
//! - DevsPro itself and everything below it — its WebKit children, and the
//!   shells, workers and tunnels its other modules run. Killing a WebKit child
//!   blanks the window; killing DevsPro kills the watchdog doing the killing.
//!   Any other running DevsPro instance gets the same treatment, so two copies
//!   (installed + dev build) never pick each other off.
//! - DevsPro's ancestors: whatever launched it (gnome-shell, a terminal, cargo).
//! - The desktop session, by name or path (the keep list below). Killing
//!   gnome-shell, Xorg or the session manager logs the user out, so the list is
//!   deliberately generous: a false positive only means one more app that has to
//!   be closed from its own window.
//!
//! The keep list starts from killer's `SYSTEM_COMMS`, `SYSTEM_ARG_FRAGMENTS`
//! and full-reset `KEEP`, extended past COSMIC to GNOME, KDE, Xfce, Cinnamon,
//! MATE, LXQt, Budgie and the wlroots compositors. Two of killer's entries are
//! dropped: `python3` and `killer`, which only existed to keep killer itself
//! alive (DevsPro's own tree replaces them, and keeping every `python3` alive
//! would make every Python dev server unstoppable). Killer's name-like argument
//! fragments (`tracker-`, `evolution-`, `goa-daemon`, `xdg-desktop-portal`,
//! `cosmic-*-applet`) are matched against process names here rather than whole
//! command lines, so a project folder called `bug-tracker-api` doesn't make its
//! dev server unstoppable; the daemons they were aimed at are all still covered.

use super::procfs::{Proc, COMM_MAX};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    /// An ordinary process: listed, stoppable, offered by the full reset.
    Free,
    /// DevsPro, another DevsPro instance, or a descendant of either. Invisible
    /// to this module: never counted in an app, never signalled.
    Tree,
    /// An ancestor of DevsPro, or part of the desktop session. Counted in a
    /// catalog app it matches (which then shows as protected), never listed as
    /// a detected app, never signalled.
    Kept,
}

/// killer's `SYSTEM_COMMS`, lowercased. Matched exactly against the process
/// name.
const KILLER_SYSTEM_COMMS: &[&str] = &[
    "systemd", "(sd-pam)", "dbus-daemon", "dbus-run-session",
    "networkmanager", "wpa_supplicant", "dhcpcd", "dhclient",
    "udevadm", "systemd-udevd", "systemd-logind", "systemd-resolved",
    "systemd-networkd", "systemd-journald", "systemd-timesyncd",
    "systemd-oomd", "systemd-userdbd", "systemd-hostnamed",
    "polkitd", "avahi-daemon", "bluetoothd", "upowerd", "udisksd",
    "colord", "rtkit-daemon", "accounts-daemon", "thermald", "irqbalance",
    "rsyslogd", "cron", "atd", "sshd", "dnsmasq",
    "at-spi-bus-launcher", "at-spi2-registryd",
    "gvfsd", "gvfsd-fuse", "gvfsd-trash", "gvfsd-metadata",
    "pipewire", "wireplumber", "pulseaudio", "pipewire-pulse",
    "xwayland", "gnome-keyring-daemon",
    "xdg-desktop-portal", "xdg-document-portal", "xdg-permission-store",
    "cosmic-comp", "cosmic-panel", "cosmic-workspaces", "cosmic-session",
    "cosmic-settings-daemon", "cosmic-bg", "cosmic-osd", "cosmic-greeter",
    "mission-center",
    "ps", "pgrep", "grep", "awk", "sed", "cat",
    "bash", "zsh", "sh", "fish", "dash",
];

/// More exact process names: other shells, privilege tools, display servers,
/// compositors and window managers, panels and notification daemons, terminal
/// emulators and multiplexers (closing one kills every shell in it, along with
/// whatever those shells are running), and DevsPro itself.
const NAME_EXACT: &[&str] = &[
    // Shells and what hosts them.
    "ksh", "mksh", "tcsh", "csh", "nu", "screen", "zellij", "byobu",
    // Privilege and login.
    "sudo", "su", "pkexec", "doas", "login", "agetty", "runuser",
    // Display servers, compositors, window managers.
    "xorg", "x", "sway", "swaybg", "swayidle", "swaylock", "swaync",
    "hyprland", "hypridle", "hyprlock", "hyprpaper", "waybar",
    "i3", "i3bar", "i3status", "i3blocks", "i3lock",
    "labwc", "wayfire", "niri", "weston", "openbox",
    "xfwm4", "xfdesktop", "xfconfd", "xfsettingsd", "xsettingsd",
    "marco", "muffin", "metacity", "mutter", "picom", "compton",
    "ksmserver", "plasmashell", "krunner", "lxpanel", "lxsession",
    "nemo-desktop", "touchegg", "mako", "dunst", "notify-osd",
    "scdaemon", "xss-lock",
    // Terminal emulators.
    "gnome-terminal-server", "kgx", "ptyxis", "ptyxis-agent", "konsole",
    "yakuake", "tilix", "terminator", "terminology", "kitty", "alacritty",
    "wezterm", "wezterm-gui", "foot", "footclient", "ghostty", "xterm",
    "urxvt", "st", "mate-terminal", "lxterminal", "qterminal", "guake",
    "tilda",
    // DevsPro, as another instance would show up.
    "devspro",
];

/// Process-name prefixes. Covers killer's full-reset `KEEP` entries (which it
/// matched as substrings) and the daemons of each desktop.
const NAME_PREFIX: &[&str] = &[
    "systemd", "dbus", "gnome-session", "gnome-shell", "gnome-keyring",
    "gnome-settings-daemon", "gnome-remote-desktop", "gsd-", "csd-", "gdm",
    "sddm", "lightdm", "kwin", "kded", "kglobalaccel", "kwallet", "kscreen",
    "kactivitymanagerd", "baloo", "plasma", "polkit", "cinnamon", "xfce4-",
    "mate-session", "mate-settings", "mate-panel", "lxqt-", "budgie-",
    "cosmic", "ibus", "fcitx", "pipewire", "wireplumber", "pulseaudio",
    "at-spi", "gvfs", "xdg-", "xwayland", "evolution-", "tracker", "goa-",
    "dconf", "p11-kit", "zeitgeist", "geoclue", "obex", "upower", "udisks",
    "avahi", "colord", "rtkit", "accounts-daemon", "networkmanager", "nm-",
    "wpa_supplicant", "sshd", "ssh-agent", "gpg-agent", "tmux", "nxnode",
    "nxrunner", "xrdp", "flatpak-session",
];

/// Process-name substrings, for the families with no common prefix.
const NAME_CONTAINS: &[&str] = &[
    "portal", "notification", "notifyd", "polkit", "authentication-agent",
    "keyring", "bluetooth", "applet", "indicator", "screensaver",
];

/// Whole-command-line substrings. Only system install paths, which a project
/// folder will never contain; killer's path-like fragments come first.
const CMDLINE_FRAGMENTS: &[&str] = &[
    "/usr/lib/systemd/", "/lib/systemd/", "/usr/libexec/",
    "/usr/lib/xorg/", "/usr/share/gnome-shell/", "org.gnome.Shell.",
    "/usr/lib/hidpi-daemon/", "-linux-gnu/libexec/", "/usr/lib/libexec/",
    "/usr/lib/polkit-", "/usr/lib/policykit-1", "/usr/lib/gnome-settings-daemon",
    "/usr/lib/kf5/", "/usr/lib/kf6/", "/usr/lib/at-spi", "/usr/lib/gvfs",
    "/usr/lib/evolution", "/usr/lib/tracker", "/usr/lib/xdg-",
    "/usr/lib/mate-", "/usr/lib/cinnamon-", "/usr/lib/xfce4/",
];

/// `name` against one exact entry, allowing for the kernel's 15-byte `comm`.
fn name_is(name: &str, entry: &str) -> bool {
    name == entry || (name.len() == COMM_MAX && entry.len() > COMM_MAX && entry.starts_with(name))
}

/// Whether a process is part of the desktop session, judged by its `comm`,
/// the basename of its argv[0] and its whole command line. Names compare
/// case-insensitively; a login shell's leading `-` is ignored.
pub fn is_session_process(comm: &str, exe_name: &str, cmdline: &str) -> bool {
    let names = [comm.to_lowercase(), exe_name.trim_start_matches('-').to_lowercase()];
    for name in names.iter().filter(|n| !n.is_empty()) {
        let exact = KILLER_SYSTEM_COMMS.iter().chain(NAME_EXACT).any(|e| name_is(name, e));
        if exact
            || NAME_PREFIX.iter().any(|p| name.starts_with(p))
            || NAME_CONTAINS.iter().any(|c| name.contains(c))
        {
            return true;
        }
    }
    CMDLINE_FRAGMENTS.iter().any(|f| cmdline.contains(f))
}

fn is_devspro(p: &Proc) -> bool {
    p.comm == "devspro" || p.exe_name == "devspro"
}

/// One guard per process, parallel to `procs`. `self_pid` is DevsPro's own pid
/// (a parameter so tests can place it).
pub fn classify(procs: &[Proc], self_pid: u32) -> Vec<Guard> {
    let tree = devspro_trees(procs, self_pid);
    let ancestors = ancestors_of(procs, self_pid);
    procs
        .iter()
        .map(|p| {
            if tree.contains(&p.pid) {
                Guard::Tree
            } else if ancestors.contains(&p.pid) || is_session_process(&p.comm, &p.exe_name, &p.cmdline) {
                Guard::Kept
            } else {
                Guard::Free
            }
        })
        .collect()
}

/// `self_pid`, every running DevsPro, and all of their descendants.
fn devspro_trees(procs: &[Proc], self_pid: u32) -> HashSet<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in procs {
        children.entry(p.ppid).or_default().push(p.pid);
    }
    let mut queue: VecDeque<u32> = std::iter::once(self_pid)
        .chain(procs.iter().filter(|p| is_devspro(p)).map(|p| p.pid))
        .collect();
    let mut tree = HashSet::new();
    while let Some(pid) = queue.pop_front() {
        if pid == 0 || !tree.insert(pid) {
            continue;
        }
        if let Some(kids) = children.get(&pid) {
            queue.extend(kids.iter().copied());
        }
    }
    tree
}

/// The parent chain above `self_pid`, up to and including pid 1.
fn ancestors_of(procs: &[Proc], self_pid: u32) -> HashSet<u32> {
    let parent: HashMap<u32, u32> = procs.iter().map(|p| (p.pid, p.ppid)).collect();
    let mut ancestors = HashSet::new();
    let mut pid = self_pid;
    while let Some(&ppid) = parent.get(&pid) {
        if ppid == 0 || !ancestors.insert(ppid) {
            break;
        }
        pid = ppid;
    }
    ancestors
}

/// Unit types that are never a single app: stopping a target, slice, scope or
/// mount from here could take half the system with it.
const NEVER_UNIT_TYPES: &[&str] = &["target", "slice", "scope", "mount", "automount", "swap", "device"];
const UNIT_TYPES: &[&str] = &[
    "service", "socket", "timer", "path", "target", "slice", "scope", "mount", "automount", "swap", "device",
];

/// systemd units the Memory module refuses to stop even when they are in the
/// catalog: the display manager, the bus, logind, networking, sshd (a remote
/// lockout), the user's own session manager.
const PROTECTED_UNIT_PREFIXES: &[&str] = &[
    "systemd-", "dbus", "gdm", "sddm", "lightdm", "lxdm", "xdm", "greetd", "cosmic-greeter",
    "display-manager", "polkit", "networkmanager", "network-manager", "wpa_supplicant",
    "user@", "user-runtime-dir@", "getty@", "serial-getty@", "autovt@", "accounts-daemon",
    "udisks", "upower", "ssh", "openssh", "rtkit", "pipewire", "wireplumber", "pulseaudio",
    "plymouth", "emergency", "rescue",
];
const PROTECTED_UNITS: &[&str] = &["init", "ly"];

pub fn is_protected_unit(unit: &str) -> bool {
    let unit = unit.to_ascii_lowercase();
    let base = match unit.rsplit_once('.') {
        Some((base, kind)) if UNIT_TYPES.contains(&kind) => {
            if NEVER_UNIT_TYPES.contains(&kind) {
                return true;
            }
            base
        }
        _ => unit.as_str(),
    };
    PROTECTED_UNIT_PREFIXES.iter().any(|p| base.starts_with(p)) || PROTECTED_UNITS.contains(&base)
}

/// `^[A-Za-z0-9@._:-]{1,128}$`, and no leading `-`, so a unit can never be read
/// as a `systemctl` option (and `-.mount`, the root filesystem, is out). The
/// helper applies the same rule again on its side.
pub fn valid_unit_name(unit: &str) -> bool {
    !unit.is_empty()
        && unit.len() <= 128
        && !unit.starts_with('-')
        && unit.chars().all(|c| c.is_ascii_alphanumeric() || "@._:-".contains(c))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn proc(pid: u32, ppid: u32, comm: &str, cmdline: &str) -> Proc {
        let exe_name = cmdline.split(' ').next().unwrap_or("").rsplit('/').next().unwrap_or("").to_string();
        Proc {
            pid,
            ppid,
            comm: comm.to_string(),
            state: 'S',
            ticks: 0,
            start_time: pid as u64 * 10,
            uid: Some(1000),
            rss_kb: 100 * 1024,
            cmdline: cmdline.to_string(),
            exe_name,
        }
    }

    #[test]
    fn desktop_session_processes_are_kept() {
        for (comm, cmdline) in [
            ("gnome-shell", "/usr/bin/gnome-shell"),
            ("gnome-session-b", "/usr/libexec/gnome-session-binary --session=pop"),
            ("Xorg", "/usr/lib/xorg/Xorg vt2 -displayfd 3"),
            ("Xwayland", "Xwayland :0 -rootless"),
            ("gsd-media-keys", "/usr/libexec/gsd-media-keys"),
            ("gjs", "/usr/bin/gjs /usr/share/gnome-shell/org.gnome.Shell.Notifications"),
            ("hidpi-daemon", "/usr/bin/python3 /usr/lib/hidpi-daemon/hidpi-daemon"),
            ("kwin_wayland", "/usr/bin/kwin_wayland --wayland-fd 7"),
            ("plasmashell", "/usr/bin/plasmashell --no-respawn"),
            ("xfce4-panel", "xfce4-panel"),
            ("cinnamon", "cinnamon --replace"),
            ("Hyprland", "Hyprland"),
            ("waybar", "waybar"),
            ("cosmic-comp", "cosmic-comp"),
            ("polkit-gnome-au", "/usr/lib/policykit-1-gnome/polkit-gnome-authentication-agent-1"),
            ("ibus-daemon", "/usr/bin/ibus-daemon --panel disable --xim"),
            ("fcitx5", "/usr/bin/fcitx5 -d"),
            ("pipewire-pulse", "/usr/bin/pipewire-pulse"),
            ("dbus-broker-lau", "/usr/bin/dbus-broker-launch --scope user"),
            ("systemd", "/lib/systemd/systemd --user"),
            ("at-spi-bus-laun", "/usr/libexec/at-spi-bus-launcher"),
            ("gvfsd-fuse", "/usr/libexec/gvfsd-fuse /run/user/1000/gvfs -f"),
            ("xdg-desktop-por", "/usr/libexec/xdg-desktop-portal-gnome"),
            ("evolution-sourc", "/usr/libexec/evolution-source-registry"),
            ("tracker-miner-f", "/usr/libexec/tracker-miner-fs-3"),
            ("gnome-keyring-d", "/usr/bin/gnome-keyring-daemon --daemonize --login"),
            ("gpg-agent", "gpg-agent --homedir /home/u/.gnupg --use-standard-socket --daemon"),
            ("ssh-agent", "/usr/bin/ssh-agent -D -a /run/user/1000/openssh_agent"),
            ("zsh", "-zsh"),
            ("bash", "/bin/bash"),
            ("tmux: server", "tmux new -s work"),
            ("kitty", "/usr/bin/kitty"),
            ("sshd-session", "sshd-session: soab@pts/3"),
        ] {
            assert!(is_session_process(comm, &proc(10, 1, comm, cmdline).exe_name, cmdline), "{comm} should be kept");
        }
    }

    #[test]
    fn ordinary_apps_are_not_kept() {
        for (comm, cmdline) in [
            ("chrome", "/opt/google/chrome/chrome --type=renderer --crashpad-handler-pid=8924"),
            ("chrome_crashpad", "/opt/google/chrome/chrome_crashpad_handler --monitor-self"),
            ("code", "/usr/share/code/code --type=utility"),
            ("firefox", "/usr/lib/firefox/firefox"),
            ("termius-app", "/opt/Termius/termius-app"),
            ("php", "php -S [::]:8080 -t /var/www/html"),
            ("node", "node /home/u/projects/bug-tracker-api/server.js"),
            ("node", "node /home/u/projects/customer-portal-sh/server.js"),
            ("python3", "python3 manage.py runserver"),
            ("io.elementary.a", "io.elementary.appcenter -s"),
            ("claude", "claude"),
            ("WebKitWebProces", "/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess 4 23 25"),
        ] {
            assert!(!is_session_process(comm, &proc(10, 1, comm, cmdline).exe_name, cmdline), "{comm} should not be kept");
        }
    }

    #[test]
    fn truncated_comm_still_matches_a_long_exact_name() {
        // `gnome-terminal-server` arrives as `gnome-terminal-` with an argv[0]
        // that isn't its name.
        assert!(is_session_process("gnome-terminal-", "", "whatever"));
        // A 15-byte name that merely starts like a short entry does not match it.
        assert!(!is_session_process("shotwell-viewer", "", "shotwell"));
    }

    #[test]
    fn own_tree_ancestors_and_other_instances_are_guarded() {
        let procs = vec![
            proc(1, 0, "systemd", "/sbin/init splash"),
            proc(1450, 1, "systemd", "/lib/systemd/systemd --user"),
            proc(1664, 1450, "gnome-shell", "/usr/bin/gnome-shell"),
            // DevsPro, launched from the shell, with its WebKit children and a
            // dev server running in one of its terminals.
            proc(500, 1664, "devspro", "devspro"),
            proc(501, 500, "WebKitWebProces", "/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess 4 23 25"),
            proc(502, 500, "WebKitNetworkPr", "/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitNetworkProcess 1 17 19"),
            proc(503, 500, "zsh", "/usr/bin/zsh"),
            proc(504, 503, "php", "php -S [::]:8080"),
            // A second DevsPro (the installed build) and its child.
            proc(700, 1664, "devspro", "/usr/bin/devspro"),
            proc(701, 700, "WebKitWebProces", "/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess 4 23 25"),
            // An unrelated app started from the shell, like DevsPro was.
            proc(8858, 1664, "chrome", "/opt/google/chrome/chrome"),
            proc(8977, 8858, "chrome", "/opt/google/chrome/chrome --type=gpu-process"),
        ];
        let guards: HashMap<u32, Guard> =
            procs.iter().map(|p| p.pid).zip(classify(&procs, 500)).collect();
        for pid in [500, 501, 502, 503, 504, 700, 701] {
            assert_eq!(guards[&pid], Guard::Tree, "pid {pid}");
        }
        // Ancestors, even ones that would otherwise be ordinary.
        for pid in [1, 1450, 1664] {
            assert_eq!(guards[&pid], Guard::Kept, "pid {pid}");
        }
        assert_eq!(guards[&8858], Guard::Free);
        assert_eq!(guards[&8977], Guard::Free);
    }

    #[test]
    fn an_ordinary_ancestor_is_kept() {
        // DevsPro started from VS Code's terminal: VS Code is now an ancestor.
        let procs = vec![
            proc(10108, 1450, "code", "/usr/share/code/code"),
            proc(20000, 10108, "zsh", "/usr/bin/zsh"),
            proc(20001, 20000, "node", "node tauri dev"),
            proc(20002, 20001, "devspro", "target/debug/devspro"),
            proc(10179, 10108, "code", "/usr/share/code/code --type=renderer"),
        ];
        let guards = classify(&procs, 20002);
        assert_eq!(guards, vec![Guard::Kept, Guard::Kept, Guard::Kept, Guard::Tree, Guard::Free]);
    }

    #[test]
    fn a_pid_cycle_does_not_hang_the_ancestor_walk() {
        let procs = vec![proc(10, 11, "a", "a"), proc(11, 10, "b", "b"), proc(12, 10, "c", "c")];
        let guards = classify(&procs, 12);
        assert_eq!(guards[0], Guard::Kept);
        assert_eq!(guards[1], Guard::Kept);
    }

    #[test]
    fn unit_names_follow_the_helper_rule() {
        for ok in ["nginx", "php8.3-fpm", "mysql", "postgresql@16-main", "docker.service", "a:b", &"x".repeat(128)] {
            assert!(valid_unit_name(ok), "{ok}");
        }
        for bad in ["", "-.mount", "--now", "nginx; rm -rf /", "a b", "ngin$x", "unit/../x", &"x".repeat(129), "naïve"] {
            assert!(!valid_unit_name(bad), "{bad}");
        }
    }

    #[test]
    fn session_and_system_units_are_protected() {
        for unit in [
            "gdm", "gdm3.service", "display-manager", "dbus", "dbus-broker.service", "systemd-logind",
            "NetworkManager", "ssh", "sshd.service", "user@1000.service", "graphical.target", "-.mount",
            "init.scope", "user.slice", "polkit",
        ] {
            assert!(is_protected_unit(unit), "{unit}");
        }
        for unit in ["nginx", "apache2", "php8.3-fpm", "mysql", "mysql.service", "postgresql@16-main", "docker.socket", "redis-server"] {
            assert!(!is_protected_unit(unit), "{unit}");
        }
    }
}
