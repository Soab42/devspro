# Changelog

Release notes for DevsPro. `scripts/release.mjs` reads this file: the section
for the version being released becomes the in-app update notes and the
website's changelog. Keep the headings in this shape — `## <version> — <date>`
(or `## Unreleased`), then `### <group>` and `- ` bullets.

## 0.3.0 — 2026-11-01

### New
- **Command library.** 889 commands for development and running servers (Git, Docker, Kubernetes, Laravel, Nginx, systemd, MySQL, PostgreSQL and more) in sections and folding categories, plus your own, kept per profile. Open it beside the terminal with Commands: a click types a command in, the play button runs it, placeholders like `<branch>` are asked for first, and destructive commands ask before they run. The Commands page browses and manages the same library.
- **Apache.** Sites can be served by Apache as well as nginx, one at a time; one switch rewrites every site for the chosen server. When something else holds port 80 or 443, the page names it.
- **Mail Catcher without Docker.** Mailpit runs built in, downloaded once and checksum-verified, or in Docker as before.
- **Saved hosts on the Terminal page.** With no terminal open, every saved SSH host is listed with a Connect button that opens its terminal and its file browser together.

### Improved
- Copy from the terminal by selecting text, and from programs that copy through it, such as Claude Code, tmux and vim, over SSH as well.
- The web server is found from its running process, so XAMPP, WAMP and Laragon installs are picked up. Its status is checked against the live process, and there is a Reload button.
- The Docker, Databases and Data browser pages offer to start Docker when it isn't running, and explain how to install it when it isn't there.
- Apps lets you choose which Docker Desktop version to install on Windows.
- The storage map shows a progress bar while it scans.
- Host dropdowns are as wide as their longest host instead of cutting it short.

### Fixed
- The terminal's file browser and the SFTP page now log in the way the terminal's ssh does: every key in ssh-agent, then your default `~/.ssh` keys. A saved key file that has gone missing no longer stops the login.
- A screen that crashes shows what went wrong, with Try again and Reload, instead of leaving the window black.
- nginx config with a space in a path, such as a Windows account name or `~/Library/Application Support`, no longer fails `nginx -t`.

## 0.2.0 — 2026-10-29

### New
- **Windows.** DevsPro now runs on Windows as well as Linux, from a per-machine installer. Elevation goes through UAC, scheduled tasks through Task Scheduler, terminals through ConPTY; the README lists what still differs.
- **In-app updates.** DevsPro checks for a new version on start and in Settings → Updates, downloads it, checks its signature, and installs it through the elevation helper with one password or UAC prompt, then restarts. The helper checks the signature again before installing anything.
- **Docked terminal.** A terminal on the right of any page, from the top bar or Ctrl+`. Hiding it keeps the shell running.
- **Memory page.** RAM per app with start and stop, a RAM watchdog, dropping caches and a full reset.
- **Cleaner.** See how much space you can free, grouped by risk: package caches, Docker leftovers, large files and folders, idle projects' `node_modules` and `target`, and the apt cache, journal and old snaps. Includes a Storage map treemap and a history of past cleans.
- **Devices in Files.** USB drives, memory cards, phones, cameras, Google Drive and network shares, mounted on a click.
- **Files editor tabs.** Syntax highlighting, folding, Format document, and git status, change bars, inline diff, blame and file history.
- **Log Table view** for workers and scheduled tasks, with the matching Laravel log entries for a failed run.
- **PostgreSQL schema picker** in the database browser.

### Improved
- Zoom the whole interface with Ctrl+Plus, Ctrl+Minus, Ctrl+0 and Ctrl+wheel.
- Cleaner scans read folders on several threads.
- SFTP favourite folders, multi-select actions and Download as zip.
- SSH checks a private key's permissions before connecting, and imports pasted keys.

### Fixed
- Closing a terminal now ends everything started in it, including background jobs, instead of leaving them running.
- Deleting a remote folder that isn't empty from the Terminal's file browser works, and removes what is in it.
- On Linux, disabling or deleting a scheduled task stops its run in progress, as it already did on Windows.
- On Linux, stopping a worker stops everything it started, and one that ignores the stop request is ended after two seconds.

## 0.1.1 — 2026-10-05

### New
- Env Diff sessions, commented keys, layout matching and rename hints.
- Code editor undo and redo, scroll sync, go-to-line and a match count.

### Fixed
- The terminal no longer shows the typed follow hook, and the editor no longer loops on its dirty state.

## 0.1.0 — 2026-09-23

### New
- First packaged release for Linux (`.deb` and `.rpm`): nginx sites with locally-trusted HTTPS, Docker, databases with a data browser, Redis, mail catcher, workers, scheduled tasks, logs, ports, SSH, SFTP, terminal, remote desktop, Files, Browser, HTTP client, hosts file editor, Env Diff, AI agents and backup and restore.
