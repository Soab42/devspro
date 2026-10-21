# Development

Frontend is React + TypeScript in `src/`; backend is Rust in `src-tauri/`.

## One-time setup

Tauri needs system libraries beyond Node and Rust. On Debian / Ubuntu / Pop!_OS:

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

Fedora:

```sh
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file \
  libappindicator-gtk3-devel librsvg2-devel
sudo dnf group install "C Development Tools and Libraries"
```

Windows needs the MSVC toolchain and WebView2 instead:

```powershell
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools  # with the C++ workload
```

WebView2 ships with Windows 11 and with any recent Edge, so there is normally
nothing to install for it. Everything else Tauri needs on Linux — WebKitGTK and
friends — has no Windows counterpart to install.

Then:

```sh
npm install
```

Rust dependencies fetch on first build — expect a few minutes.

## Running it

```sh
npm run app      # the real desktop app (tauri dev)
npm run dev      # frontend only, in a browser at :1420
```

`npm run app` is what you normally want. `npm run dev` is useful for pure UI
work, but every `invoke()` call fails there — there's no Tauri backend behind a
plain browser — so anything touching Docker, nginx or the database won't load.

Both hot-reload the frontend. Changing Rust triggers a recompile and restart,
which takes appreciably longer.

> Only run one at a time. `tauri dev` holds Cargo's lock on `target/`, so a
> concurrent `npm run package` will sit at *"Blocking waiting for file lock"*
> until you stop it.

## After making a change

```sh
npm run check
```

That's typecheck → frontend tests → Rust tests, about 190 assertions' worth in
total, and the Rust half covers the whole workspace so the elevated helper's own
tests run too. Run it before every commit. The individual pieces, when you want a
faster loop:

| Command | |
| --- | --- |
| `npm run typecheck` | `tsc --noEmit` — no build, fastest signal |
| `npm run test:watch` | vitest in watch mode |
| `npm test` | frontend tests once |
| `npm run test:rust` | backend tests |
| `npm run build` | typecheck + bundle the frontend (no packaging) |

### If a test fails and the message looks strange

Some tests enforce conventions rather than behaviour, and their failures read
oddly the first time. They live in `src/ui/conventions.test.ts` and
`src/ui/visibility.test.tsx`, and each one exists because the bug it describes
actually happened here.

| Failure | What it means |
| --- | --- |
| *contains no emoji or pictographic glyph* | Iconography is lucide SVG only. Add an icon to `src/ui/icons.ts` and use `<Icon icon={…}/>`. |
| *no module renders its own `<h1>`* | `<Page>` owns the page heading. Pages that render their own end up with two or three of them across loading/error/ready states. |
| *nothing uses a native `<select>`* | WebKitGTK renders native dropdowns with GTK's own colours and ignores the theme. Use `src/ui/Select.tsx`. |
| *inline styles appear only where the value is dynamic* | Add a class instead. If the value genuinely *is* data (a meter's width, a portal's measured position), add the file to the allowlist in the test with a reason. |
| *only `src/ui/icons.ts` imports lucide-react* | Import icons from `../ui/icons`, so swapping one is a single-line change. |
| *layers overlays from back to front* | A `--z-*` token in `src/styles/tokens.css` got inverted; something will render behind something else. |

If a guard is genuinely wrong for what you're doing, change the guard *and its
comment* in the same commit — don't work around it.

### Watch out for

- **Comments count.** These guards scan source text. Both files strip comments
  before matching, because a comment quoting the banned pattern would otherwise
  flag itself. New guards need the same treatment.
- **CSS in tests.** `vitest.config.ts` sets `css: true`. Without it Vitest stubs
  stylesheets to empty strings and every CSS assertion passes vacuously.
- **jsdom does no layout.** Nothing here can catch clipping, overflow or
  scrolling. Those need the real app — check narrow windows by hand.

## Where things go

| | |
| --- | --- |
| `src/ui/` | Design-system primitives. Each owns a sibling `.css`. |
| `src/styles/` | Tokens, base element styles, a few utilities. |
| `src/modules/<name>/` | One feature: `api.ts` (invoke wrappers), `pages/`, `components/`. |
| `src/app/` | Shell, routing, module registry, theme, confirm dialog. |
| `src-tauri/src/modules/<name>/` | That feature's Rust half. |
| `src-tauri/src/modules/<name>/platform/` | The per-OS half of it. See below. |
| `src-tauri/helper/` | The privileged helper. Keep it small. |

Adding a module is one entry in `src/app/ModuleRegistry.ts` plus a folder — the
sidebar and router both render off that array.

Module pages should not have their own stylesheets; compose primitives instead.
Two exceptions exist (the Dashboard grid and the Logs full-height layout) and a
test asserts the list stays at exactly those two.

### Anything OS-specific goes in a `platform/` module

Whenever a module has to ask the operating system something, the OS-specific part
lives in a `platform/` submodule with one file per target and a `mod.rs` that
does nothing but `#[cfg]`-dispatch and document the contract:

```
src-tauri/src/modules/system/platform/
    mod.rs        the contract, as a table: what each backend must provide
    linux.rs      /proc, df, lspci
    windows.rs    Win32 through windows-sys
    macos.rs      sysctl, mach, vm_stat
```

`elevate`, `system`, `workers`, `processes`, `nginx_ssl::nginx`,
`nginx_ssl::php_fpm`, `apps` and the helper itself are all laid out this way.
Three rules keep it useful rather than ceremonial:

- **The shared half does the thinking.** Rate maths, retries, assembling a
  result: none of that belongs in a backend. A backend reads or writes one thing
  and returns it.
- **A backend never panics and never invents a number.** Return an empty vec, a
  zero, or `None`. A `#[tauri::command]` that panics cannot unwind back across
  the FFI boundary — it takes the whole app down, which is how the Dashboard used
  to abort on Windows.
- **Say what's different, and why, in the file that's different.** `windows.rs`
  in `nginx` opens with three paragraphs on why Windows nginx needs `-p` and has
  no `conf.d`; that's the file where someone will look for it.

Where a platform can't do something, the honest move is to say so in the type —
`load_avg()` returns `None` on Windows because there is no load average, rather
than zeros that read as an idle machine.

### Changing shared code without breaking the other platform

You can only compile and test for the platform you're sitting on. Two habits
carry most of the weight:

- **Move code, don't retype it.** When splitting something per-OS, the existing
  platform's code should move verbatim — then `git show HEAD:<file>` and a diff
  proves nothing changed. Behaviour includes user-visible strings, so leave the
  messages alone too.
- **Pin the contract with a test.** Ordering and lists are what silently drift:
  `nginx::platform::unix` asserts the operation order its `conf.d` approach
  depends on, and `apps::platform::unix` writes out every apt package name in
  full. Those run on Linux CI even when the change was made on Windows.

## Packaging

See **[PACKAGING.md](PACKAGING.md)**.

```sh
npm run package
```

That builds the release helper first and then bundles — in that order, because
skipping it produces an installable package in which every elevated action
fails. Artifacts land in `src-tauri/target/release/bundle/`.

## Git

`main` is the default branch. Work on a branch:

```sh
git switch -c fix/table-action-buttons
```

Before committing:

```sh
npm run check
git status          # confirm nothing unexpected, and nothing missing
git diff --staged
```

`target/`, `dist/`, `node_modules/` and `src-tauri/binaries/` are ignored —
`binaries/` in particular holds a renamed copy of a build artifact and must
never be committed.

> `.gitignore` has a bare `logs` pattern (for log files) with negations for
> `src/modules/logs/` and `src-tauri/src/modules/logs/`. Those are real source
> directories that happen to share the name. If you add another path containing
> `logs`, check `git check-ignore -v <path>` before assuming it's staged.

Commit in coherent pieces rather than one large drop — a UI change, a backend
change and a packaging change are three commits. Write the message so it says
*why*:

```
Stop table cells clipping their action buttons

.dt-td uses the max-width:0 ellipsis hack, which forces a column below
its min-content width. Correct for text, but it clipped every action
button and dropdown to a sliver. Cells containing controls now opt out
via :has().
```

Then:

```sh
git push -u origin fix/table-action-buttons
```

Open a PR against `main`. If you're pushing straight to `main` on a solo
project, at minimum make sure `npm run check` passed and the app actually
launched — the test suite cannot see layout, so a green run is necessary but
not sufficient.
