# Packaging DevsPro

```sh
npm run package
```

Produces a `.deb` and an `.rpm` on Linux, and an NSIS installer (`.exe`) on
Windows. Cross-building is not set up: each platform's artifacts are built on
that platform.

Artifacts land in `src-tauri/target/release/bundle/{deb,rpm,nsis}/`.

### Where each platform's bundle settings live

`tauri.conf.json` holds `"targets": ["deb", "rpm"]` and everything shared.
Windows adds its own on top, in `tauri.windows.conf.json`, which Tauri merges
automatically when building for that platform:

```json
{ "bundle": { "targets": ["nsis"], "windows": { "nsis": { "installMode": "perMachine" } } } }
```

The alternative — one array listing `deb`, `rpm` *and* `nsis`, leaving the
bundler to skip whatever the host can't build — does work on Windows, where deb
and rpm are quietly ignored. It was not worth relying on: it assumes the skipping
is symmetric on Linux, and the cost of being wrong is a broken Linux release. A
platform file leaves the Linux target list exactly as it has always been.

## Step 1 is not optional, on either platform

`npm run package` is two steps on purpose:

```sh
cargo build --release -p ldm-helper --manifest-path src-tauri/Cargo.toml
tauri build
```

`src-tauri/build.rs` copies `target/<profile>/ldm-helper` into
`binaries/ldm-helper-<target-triple>`, which is what `bundle.externalBin`
ships. It deliberately does **not** invoke cargo itself — a build script
shelling out to cargo can deadlock on cargo's own package-cache lock.

If the release helper isn't there, the build script drops in a **placeholder**
and packaging succeeds anyway. The result is an installable package in which
every elevated action fails at runtime. Watch for this line in the build output:

```
warning: ldm-helper binary not found at ... — dropping a placeholder
```

Verify before shipping:

```sh
# Linux
file src-tauri/binaries/ldm-helper-x86_64-unknown-linux-gnu
# want: ELF 64-bit LSB pie executable ... stripped
# NOT:  POSIX shell script
```

```powershell
# Windows — the placeholder is a zero-byte file, so size alone tells you
Get-Item src-tauri\binaries\ldm-helper-x86_64-pc-windows-msvc.exe |
  Select-Object Name, Length
# want: a few hundred KB. A Length of 0 is the placeholder.
```

Skipping step 1 also ships a *debug* helper if one happens to be lying around
from `cargo build` — 8 MB unstripped instead of a few hundred KB.

## Linux

### What the package installs

| Path | What |
| --- | --- |
| `/usr/bin/devsuite` | Main binary |
| `/usr/bin/ldm-helper` | Privileged helper, invoked via `pkexec` |
| `/usr/share/polkit-1/actions/com.soab.devsuite.policy` | Authorises the helper |
| `/usr/share/applications/DevsPro.desktop` | Menu entry (`Name=DevsPro`, `Exec=devsuite`) |
| `/usr/share/icons/hicolor/*/apps/devsuite.png` | Icons |

### Which name comes from where

This trips people up, because `productName` is *not* the source of most of it.
Verified against a built package:

| Thing | Source |
| --- | --- |
| Binary, `Exec=`, `Icon=`, icon filename | Cargo `package.name` (`devsuite`) |
| `.desktop` `Name=` (menu label), window title | `productName` (`DevsPro`) |
| deb package name | `productName`, sanitised — `DevsPro` → `devs-pro` |
| deb `Maintainer` | Cargo `package.authors[0]` — **not** `bundle.publisher` |

Two consequences worth knowing:

- `<icon_name>` in the `.policy` file must be **`devsuite`**, the crate name.
  Setting it to `DevsPro` gives a broken icon in the pkexec dialog.
- Left at the Tauri template's `authors = ["you"]`, every package ships with
  `Maintainer: you`. `bundle.publisher` does not override it.

The policy's `org.freedesktop.policykit.exec.path` is hardcoded to
`/usr/bin/ldm-helper` — where the deb/rpm bundlers place an `externalBin`
sidecar. The Rust side never names the polkit action; polkit matches on this
path.

### Runtime dependencies

Declared in `tauri.conf.json`:

- deb: `pkexec | policykit-1` — Ubuntu 24.04+ and Pop!_OS split `pkexec` out of
  `policykit-1`, so the alternation covers both old and new naming.
- rpm: `polkit`

Without pkexec, the app runs but every privileged action fails.

### Why not AppImage

An AppImage is a self-contained file with no install step, so it cannot place a
file in `/usr/share/polkit-1/actions/`. Since polkit only authorises actions it
has a policy for, elevation would fail on every attempt — nginx config,
`/etc/hosts`, the certificate trust store, package installs. That is most of the
app.

If an AppImage is ever wanted, the policy has to be installed separately by the
user, and that requirement documented loudly.

## Windows

### The installer is per-machine on purpose

`bundle.windows.nsis.installMode` is `perMachine`, which puts the app in
`Program Files` and requires an administrator to install it. NSIS defaults to
offering a per-user install into `%LOCALAPPDATA%`, and that would be a
privilege-escalation hole in this particular app:

`ldm-helper.exe` sits next to the main executable and is what DevsPro launches
**elevated** through UAC. If it lived in a directory the logged-in user can
write to, any program running as that user could overwrite it and then wait for
DevsPro to elevate it — turning a normal-user process into an administrator one.
`Program Files` is writable only by administrators, which is what makes the UAC
prompt mean something.

This is the Windows counterpart of the guarantee the Linux packages get for
free: the deb and rpm install the helper into root-owned `/usr/bin`.

### What the installer places

| Path | What |
| --- | --- |
| `C:\Program Files\DevsPro\DevsPro.exe` | Main binary |
| `C:\Program Files\DevsPro\ldm-helper.exe` | Privileged helper, launched via UAC |
| Start-menu and (optionally) desktop shortcuts | Created by NSIS |

There is no polkit equivalent to install. `elevate::platform::windows` raises
the UAC prompt with `ShellExecuteExW` and the `runas` verb, which needs no
registration — which is also why nothing here corresponds to the `.policy` file
the Linux packages ship.

### No code signing

The installer and the executables are unsigned, so SmartScreen shows
"Windows protected your PC" on first run and the user has to choose **More
info → Run anyway**. Signing needs a code-signing certificate; until there is
one, say so in the release notes rather than letting people wonder.

### Verifying a built package

```powershell
# The helper really is inside the installer, not the placeholder
Get-Item src-tauri\target\release\ldm-helper.exe | Select-Object Length
Get-ChildItem src-tauri\target\release\bundle\nsis\
```

After installing, open Settings → Elevation helper → **Test elevation**. It
round-trips a harmless Ping through the real UAC prompt, which is the fastest
way to confirm the helper is where the app expects and that elevation works.

## Releasing an update

Installed copies check `https://soab42.github.io/devspro/latest.json` (the
`site/` folder, deployed by `.github/workflows/pages.yml`) a little after they
start and every six hours, and from Settings → Updates. The feed is in the
shape Tauri's updater reads; each package in it is a GitHub release asset with
a minisign signature.

```sh
npm run release:version 0.2.0   # every version field, and dates CHANGELOG's "Unreleased"
npm run package                 # on each platform: .deb + .rpm on Linux, the .exe on Windows
npm run release:feed            # signs this machine's bundles into site/latest.json
npm run release:publish         # creates the GitHub release v0.2.0, or adds to it
git add -A && git commit -m "Release 0.2.0" && git push   # Pages publishes the feed
```

Run `release:feed` and `release:publish` on the Linux machine and again on the
Windows one; the feed keeps both platforms' entries for the same version. Push
`site/latest.json` only after every package it names is uploaded — a copy that
sees the feed downloads at once.

### The signing key

`src-tauri/updater.pub` is compiled into both the app and the helper. The
private key it pairs with is `~/.tauri/devspro-updater.key` (or
`TAURI_SIGNING_PRIVATE_KEY_PATH`, with `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if
it has one). Back it up somewhere safe: **every installed copy trusts only
that key, so losing it means no more in-app updates for them** — those users
would have to install the next version by hand. To replace it before the first
release that ships it: `npx tauri signer generate -w ~/.tauri/devspro-updater.key`,
then copy the `.pub` over `src-tauri/updater.pub`.

`release:feed` checks every signature against `updater.pub` with Node's own
Ed25519 and BLAKE2b before it writes the feed, so a key that doesn't match is
caught there rather than by users' copies refusing the update.

### What an installed copy does

- Downloads the package for how it was installed (`dpkg-query -S` /
  `rpm -qf` on its own binary; NSIS on Windows) and checks the signature as
  it streams in.
- Hands it to the helper as `InstallUpdate`. The helper reads the file once,
  checks the signature again with the key compiled into it, refuses one not
  signed as a newer release than the installed one (the file name in the
  signature's trusted comment carries the version), and works from its own
  copy: Linux installs from `/var/cache/devspro-update` with
  `apt-get install -y` or `rpm -U`, both of which refuse a downgrade, then the
  app restarts itself; Windows starts the installer from a fresh
  Administrators-only folder under `%SystemRoot%\Temp` with `/P /UPDATE /R`
  (passive, keep data, relaunch as the user) and the app exits so it can be
  replaced.
- A development build never checks on its own, and can't install.

## Release profile

`[profile.release]` in `src-tauri/Cargo.toml` sets `lto = "thin"`,
`codegen-units = 1` and `strip = true`. It applies to the helper too, since the
helper is a workspace member. `panic` is left at `"unwind"` deliberately: the
elevated helper should run its destructors on a panic rather than abort partway
through applying an operation manifest.
