# Packaging DevsPro for Linux

Produces a `.deb` and an `.rpm`. Both install the polkit policy that makes
elevated actions work; see [Why not AppImage](#why-not-appimage).

## Build

```sh
# 1. Build the privileged helper in release mode FIRST.
cd src-tauri
cargo build --release -p ldm-helper

# 2. Then package.
cd ..
npm run tauri build
```

Artifacts land in `src-tauri/target/release/bundle/{deb,rpm}/`.

### Step 1 is not optional

`src-tauri/build.rs` copies `target/<profile>/ldm-helper` into
`binaries/ldm-helper-<target-triple>`, which is what `bundle.externalBin`
ships. It deliberately does **not** invoke cargo itself — a build script
shelling out to cargo can deadlock on cargo's own package-cache lock.

If the release helper isn't there, the build script drops in a **placeholder
shell script** that just prints an error, and packaging succeeds anyway. The
result is an installable package in which every elevated action fails at
runtime. Watch for this line in the build output:

```
warning: ldm-helper binary not found at ... — dropping a placeholder
```

Verify before shipping:

```sh
file src-tauri/binaries/ldm-helper-x86_64-unknown-linux-gnu
# want: ELF 64-bit LSB pie executable ... stripped
# NOT:  POSIX shell script
```

Skipping step 1 also ships a *debug* helper if one happens to be lying around
from `cargo build` — 8 MB unstripped instead of ~600 KB.

## What the package installs

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

## Runtime dependencies

Declared in `tauri.conf.json`:

- deb: `pkexec | policykit-1` — Ubuntu 24.04+ and Pop!_OS split `pkexec` out of
  `policykit-1`, so the alternation covers both old and new naming.
- rpm: `polkit`

Without pkexec, the app runs but every privileged action fails.

## Why not AppImage

`bundle.targets` is `["deb", "rpm"]` on purpose. An AppImage is a self-contained
file with no install step, so it cannot place a file in
`/usr/share/polkit-1/actions/`. Since polkit only authorises actions it has a
policy for, elevation would fail on every attempt — nginx config, `/etc/hosts`,
the certificate trust store, package installs. That is most of the app.

If an AppImage is ever wanted, the policy has to be installed separately by the
user, and that requirement documented loudly.

## Release profile

`[profile.release]` in `src-tauri/Cargo.toml` sets `lto = "thin"`,
`codegen-units = 1` and `strip = true`. It applies to the helper too, since the
helper is a workspace member. `panic` is left at `"unwind"` deliberately: the
elevated helper should run its destructors on a panic rather than abort partway
through applying an operation manifest.

## Verifying a built package

```sh
cd src-tauri/target/release/bundle/deb
dpkg-deb --contents DevsPro_0.1.0_amd64.deb | grep -E "ldm-helper|polkit|desktop"
dpkg-deb --field  DevsPro_0.1.0_amd64.deb Depends Version Maintainer

sudo apt install ./DevsPro_0.1.0_amd64.deb
```

After installing, open Settings → Elevation helper → **Test elevation**. It
round-trips a harmless Ping through the real pkexec prompt, which is the
fastest way to confirm the helper, the polkit policy and the exec path all
line up.
