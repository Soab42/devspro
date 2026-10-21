# DevsPro

A local development environment manager for Linux and Windows, in one window.
Tauri + React + Rust.

## What it does

| Module | |
| --- | --- |
| **Dashboard** | Live CPU, memory, GPU, storage and network, with a health summary |
| **Nginx & SSL** | Add sites (proxy-to-port or Laravel), each with a locally-trusted HTTPS certificate from a CA it manages for you. Optional public Cloudflare tunnel per site |
| **Docker** | Containers: start, stop, restart, logs, and remap published ports |
| **Databases** | MySQL / PostgreSQL / MongoDB / Redis in DevsPro's own compose project, each with an admin UI, plus users, backup and restore |
| **Mail Catcher** | Mailpit in a container, so outgoing mail from local apps lands somewhere you can read it |
| **Workers** | Any shell command kept running in the background — a Laravel queue worker, say |
| **Processes** | What is listening on which port, and stop it |
| **Logs** | Tail nginx access/error and Laravel logs per site |
| **Apps** | Install common dev tooling: nginx, Docker, PHP, Node/nvm, cloudflared… |

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
packages, reload nginx. There is no "run this command" operation.

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
