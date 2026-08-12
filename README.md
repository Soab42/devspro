# DevsPro

A local development environment manager for Linux, in one window. Tauri +
React + Rust.

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

Produces a `.deb` and an `.rpm` in `src-tauri/target/release/bundle/`. The
script builds the privileged helper first on purpose — skip that step and the
bundle silently ships a placeholder, giving you an installable app in which
every elevated action fails at runtime. Details in
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
