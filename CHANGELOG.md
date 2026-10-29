# Changelog

Release notes for DevsPro. `scripts/release.mjs` reads this file: the section
for the version being released becomes the in-app update notes and the
website's changelog. Keep the headings in this shape — `## <version> — <date>`
(or `## Unreleased`), then `### <group>` and `- ` bullets.

## Unreleased

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
