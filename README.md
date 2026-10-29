# DevsPro

A local development environment manager for Linux and Windows, in one window.
Tauri + React + Rust.

## What it does

| Module | |
| --- | --- |
| **Dashboard** | Live CPU, memory, GPU, storage and network, with a health summary |
| **Nginx & SSL** | Add sites (proxy-to-port or Laravel), each with a locally-trusted HTTPS certificate from a CA it manages for you. Optional public Cloudflare tunnel per site. A New Project wizard scaffolds Laravel, adds the site, creates a database, writes `.env` and runs migrations in one go, reporting each step |
| **Docker** | Containers: start, stop, restart, logs, and remap published ports |
| **Databases** | MySQL / PostgreSQL / MongoDB / Redis in DevsPro's own compose project, each with an admin UI, plus users, backup and restore |
| **Cleaner** | See how much space you can free, grouped by risk, with a "Clean suggested" shortcut and a history of past cleans. Covers package caches and stores (npm, pnpm, composer, cargo, gradle and others), each cleaned with the tool's own command; Docker leftovers, item by item, with DevsPro's own database volumes protected; large files and the biggest folders; node_modules, target and .next in idle projects; and the apt cache, journal and old snaps through the helper. A WinDirStat-style Storage map shows where the space went. Scans change nothing, and every clean goes through a review list |
| **Data browser** | A TablePlus-style browser for any MySQL/Postgres server — table and view sidebar, structure (columns, indexes, foreign keys), a filterable grid with inline row editing, relation columns you can click through, a multi-sheet SQL console with completion and history, and CSV/JSON/INSERT export. Read-only per connection until you switch it to read & write; opens databases DevsPro doesn't run too — remote servers, ones installed on this machine, and other containers'. PostgreSQL databases load one schema at a time, with a schema picker |
| **Mail Catcher** | Mailpit in a container, so outgoing mail from local apps lands somewhere you can read it |
| **Workers** | Any shell command kept running in the background — a Laravel queue worker, say |
| **Scheduled tasks** | Recurring commands (`php artisan schedule:run` every minute, say) written to your crontab inside a block DevsPro manages, with each run's output |
| **Redis** | Browse keys by pattern with typed values, and watch Laravel queues (pending / delayed / reserved) with a peek at the next job's payload — native or Docker, over RESP directly |
| **Processes** | What is listening on which port, and stop it |
| **Logs** | Tail nginx access/error and Laravel logs per site |
| **Apps** | Install common dev tooling: nginx, Docker, PHP, Node/nvm, cloudflared… |
| **SSH** | Saved hosts and key generation / install on host |
| **AI agents** | Launch terminal coding agents — Claude Code, Codex, Gemini CLI, OpenCode, Copilot CLI, Aider and more, or any command you add — in a project folder of your choice, each in its own tab; sees which are installed and installs the missing ones |
| **Terminal** | Local and ssh shells, tmux-style: split panes, multiple windows, pop-out into its own OS window, a file browser that follows the shell, and one shell docked on the right of any page (Ctrl+`) |
| **SFTP** | Two-pane file browser and transfers for a saved host |
| **Remote Desktop** | Saved VNC and RDP machines: VNC is viewed in-app (noVNC over a local bridge), RDP opens in FreeRDP or Remmina |
| **Files** | A Finder-style explorer: places down the side, back/forward and a path bar, list, icon or column view, filter, hidden files, and per-item open / reveal / rename / move to trash. USB drives, memory cards, phones and cameras (MTP, gPhoto2), Google Drive accounts and network shares appear under Devices and Cloud & network, mount on a click and eject from the sidebar; Drive files show their real titles. Files open in editor-style tabs: source and text in a syntax-highlighted editor with folding, a right-click menu and Format document (the project's prettier, rustfmt, ruff/black, gofmt…, else a built-in prettier; optional format on save), pictures with wheel zoom and drag to pan, audio and video players, and PDFs; Ctrl+Tab and Ctrl+W move between and close tabs, and open tabs come back after a restart. Inside a git repository the listing shows VS Code's status letters and colours and the branch, and the editor shows change bars against the staged copy, an inline diff with Revert per change, GitLens-style blame on the current line and the file's history |
| **Browser** | A small tabbed browser with Chrome, Safari and Firefox-style frames to switch between; pages render in a native webview so local `.test` sites and anything else load as they would in a real browser |
| **Env Diff** | Compare two `.env` files by variable — missing, new and changed values, wherever they sit in the file — and copy them across |
| **Settings** | Theme, UI zoom (Interface size; Ctrl+Plus, Ctrl+Minus, Ctrl+0, Ctrl+wheel), autostart and tray behaviour, plus Backup & restore: export sites, workers, preferences and the root CA as one JSON file and import them on another machine |
| **Hosts file** | Edit `/etc/hosts` entry by entry, with DevsPro-written lines told apart from system ones, and one-click entries for nginx sites that are missing theirs |
| **HTTP client** | Send a request (any method, headers, body) to any URL and read the status, headers and body back; local HTTPS sites are trusted via the app's own root CA, and every request is kept in a history to rerun |

## Install

Grab a package from `src-tauri/target/release/bundle/` after building, or:

```sh
sudo apt install ./DevsPro_0.1.0_amd64.deb    # Debian / Ubuntu / Pop!_OS
sudo dnf install ./DevsPro-0.1.0-1.x86_64.rpm # Fedora / RHEL
```

Requires `pkexec` (polkit), which the package depends on.

On Windows, run `DevsPro_0.1.0_x64-setup.exe` from `bundle/nsis/`. It installs
per-machine and asks for administrator rights, which is deliberate — see
[PACKAGING.md](docs/PACKAGING.md#the-installer-is-per-machine-on-purpose). The
installer is unsigned, so SmartScreen will interpose once.

### Updates

Once installed, DevsPro updates itself: it checks for a new version on start
and in Settings → Updates, and installs it through the elevation helper with
one password (or UAC) prompt. Packages are signed, and the helper checks the
signature itself before installing anything. Release notes:
**[soab42.github.io/devspro](https://soab42.github.io/devspro/changelog.html)**.

## Platform support

Every module works on both, but a few things genuinely differ, and the app says
so rather than pretending otherwise:

| | Linux | Windows |
| --- | --- | --- |
| Elevation | `pkexec` (polkit policy shipped in the package) | UAC |
| PHP for Laravel sites | PHP-FPM pools under `/run/php` | `php-cgi.exe`, which DevsPro starts itself on a port per PHP version |
| nginx wiring | a drop-in file in `conf.d` | one `include` spliced into `http{}`, since Windows nginx has no `conf.d` |
| CA trust | OS store + NSS, so Firefox and Chrome both trust it | Windows Root store — Chrome and Edge trust it, **Firefox does not** without `security.enterprise_roots.enabled` |
| Stopping a worker | SIGTERM, so it can finish the job it's on | forced, because Windows offers a GUI app no way to ask a console program to wind down |
| nginx not running | reloaded through systemd | DevsPro writes the config and tells you to start nginx; it will not start it for you |
| Processes (listening ports) | `ss` + `/proc` | `GetExtendedTcpTable`; the full command line isn't shown, as Windows keeps it out of reach |

macOS is partly implemented — elevation, nginx discovery and the system metrics
have backends — but it has not been run on real hardware, and the Processes and
PHP paths have no macOS implementation yet. Treat it as unfinished.

## Develop

```sh
npm install
npm run app        # the desktop app
npm run check      # typecheck + frontend tests + Rust tests
```

Full guide, including the system libraries Tauri needs and what the
convention-enforcing tests mean when they fail:
**[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)**.

## Packaging

```sh
npm run package
```

Produces a `.deb` and an `.rpm` on Linux, and an NSIS installer on Windows, in
`src-tauri/target/release/bundle/`. Each platform builds its own; there is no
cross-building. The script builds the privileged helper first on purpose — skip
that step and the bundle silently ships a placeholder, giving you an installable
app in which every elevated action fails at runtime. Details in
**[docs/PACKAGING.md](docs/PACKAGING.md)**.

## How privilege works

Anything needing root — writing nginx config, `/etc/hosts`, the certificate
trust store, installing apt packages — goes through one small helper binary
(`src-tauri/helper`) invoked via `pkexec`, and it only ever applies a declared
*operation manifest*. The manifest's operations are a fixed enum: write a file
under one allowed prefix, sync a marker-delimited block, install named
packages, reload nginx, clean the apt cache, vacuum the journal, remove a
disabled snap revision, install a DevsPro update — and only one carrying a
valid signature from the release key compiled into the helper. There is no
"run this command" operation.

Everything that does not genuinely need root deliberately does not get it —
nvm installs into `~/.nvm`, npm globals go to your own npm prefix, and
standalone binaries like `cloudflared` land in `~/.local/bin`.

## Design system

UI primitives live in `src/ui/` — each component owns a sibling stylesheet, and
`src/styles/` holds the tokens everything reads. A few conventions are enforced
by tests in `src/ui/conventions.test.ts` rather than by review:

- No emoji anywhere; iconography is lucide SVG, re-exported from
  `src/ui/icons.ts` (the only file allowed to import `lucide-react`).
- No `<h1>` inside a module — `<Page>` owns the page heading.
- No native `<select>`; WebKitGTK renders those with GTK's own colours and
  ignores the theme, which is why `src/ui/Select.tsx` exists.
- Inline styles only where the value *is* the data (a meter's width, a
  portal's measured position).

## License

MIT — see [LICENSE](LICENSE).
