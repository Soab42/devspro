# Changelog

Release notes for DevsPro. `scripts/release.mjs` reads this file: the section
for the version being released becomes the in-app update notes and the
website's changelog. Keep the headings in this shape — `## <version> — <date>`
(or `## Unreleased`), then `### <group>` and `- ` bullets.

## Unreleased

### New
- **Forgot your password?** The lock screen can reset it: your computer asks for an administrator's approval, then you choose a new password or none. Nothing else is lost, since the password never encrypted anything. Settings → Password sets, changes or removes it.

## 0.5.0 — 2026-11-10

### New
- **Windows.** DevsPro now comes for 64-bit Windows as a setup `.exe`. It installs for every user into Program Files, so installing needs an administrator, and anything that needs administrator rights asks through the usual Windows prompt. The installer isn't code-signed yet, so the first time Windows SmartScreen shows "Windows protected your PC": choose **More info → Run anyway**.

### Improved
- SFTP logs in with the keys the terminal's ssh uses. It now also offers the keys `~/.ssh/config` names for a host, as ssh does, so a host you can reach in the terminal opens in the file browser too. When no key works, the error lists the keys in `~/.ssh` that weren't tried, and the terminal sidebar can make one of them the host's key and reconnect. The host form warns when "SSH agent / default keys" would leave out a key in `~/.ssh`.
- A profile no longer needs a name or a password. The welcome screen can be skipped, and a profile without a password opens without asking. An empty name takes your computer account's name. A profile that has a password still asks for it.

### Fixed
- The terminal's file browser keeps each machine's folder to that machine. Moving between a local shell and an SSH host no longer tries to list one machine's path on the other, and each machine goes back to the folder you last browsed on it.

## 0.4.0 — 2026-11-09

### New
- **LLM Traces (beta).** A local Langfuse and LangSmith. Point an LLM app's Langfuse, LangSmith or OpenTelemetry SDK at DevsPro and every trace lands here: a waterfall of its LLM calls, tool calls and steps, with prompts shown as chats, responses, models, tokens, time to first token, errors and scores. Any API key works, apps in Docker containers can send traces too (as `host.docker.internal`), nothing leaves the machine, and the Connect tab has the environment variables or docker-compose snippet for each SDK and a log of what reached the collector.
- **Terminals and SFTP keep running when you leave their page.** Come back and each terminal has its history and each SFTP connection is in the folders you left it in. The sidebar shows how many are running. They end when you disconnect them or close the window.
- **Remember SSH passwords.** Tick "Remember this password" when connecting to a password host, and connecting won't ask again. It is saved encrypted on this computer; forget it from the host's menu in SSH.

### Improved
- The terminal's file browser follows an SSH shell's folder without typing anything into the shell.
- The SFTP page offers Connect, or Add a host when none is saved, instead of sitting empty.
- Pages remember what they held for as long as DevsPro is open, such as the two files in Env Diff.
- Terminal tabs have a full-width row of their own under the header. When there are more than fit, the mouse wheel scrolls them, and the active tab stays in view.
- Hosts file: an entry's hostnames are smaller and wrap onto more lines, so the table no longer scrolls sideways.

### Changed
- HTTP client and Browser are left out of this release while they are being finished.

### Fixed
- Switching from a local terminal to an SSH one no longer shows a "could not list" error in the file browser.

## 0.3.1 — 2026-11-01

### New
- **Paste screenshots into the terminal.** With an image on the clipboard, Ctrl+V saves it and types its path, which Claude Code attaches as an image. In an SSH terminal the image is copied to the server's `/tmp` first, readable by you alone.

### Improved
- Right-click in a terminal pastes, as in Termius and PuTTY: the clipboard's text, or its image when there is no text.

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
