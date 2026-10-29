# Launching DevsPro on Product Hunt

Everything for the launch: the listing copy, the images, the website
switches, and a checklist for before, on and after launch day.

The website lives in [`site/`](../site) and is deployed to GitHub Pages by
[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) on every push to
`main` that touches `site/`. Its URL is `https://soab42.github.io/devspro/`.

## Read this first: the repository is private

`github.com/Soab42/devspro` is private today, and two things the launch depends
on don't work from a private repository on a free plan:

- **GitHub Pages.** A private repository needs GitHub Pro, Team or Enterprise
  to publish a Pages site.
- **Downloads and updates.** Release files on a private repository return 404
  to everyone who isn't signed in with access. The site's Download buttons
  and the in-app updater both fetch from GitHub Releases.

So before launch, either make the repository public (Settings → General →
Danger Zone → Change visibility), or host the site and the release files
somewhere public and update the URLs (see [Changing the site's address](#changing-the-sites-address)).
The site's copy ("Source on GitHub", "View on GitHub", MIT licensed) assumes the
repository is public.

## The listing

**Name:** DevsPro

**Tagline** (60 characters at most). Pick one:

| Characters | Tagline |
| --- | --- |
| 57 | Your local dev stack in one window, for Linux and Windows |
| 57 | Run your local dev stack from one app on Linux or Windows |
| 59 | Local HTTPS sites, Docker and databases for Linux & Windows |
| 55 | The local dev environment manager for Linux and Windows |

**Description** (260 characters at most; this one is 256):

> DevsPro runs your local dev stack from one app on Linux and Windows: nginx sites with trusted HTTPS, Docker, databases, workers, mail, logs, ports, SSH and a terminal. Elevated actions go through one small helper with a fixed list of operations. Free, MIT.

**Links:** website `https://soab42.github.io/devspro/`, GitHub
`https://github.com/Soab42/devspro`.

**Pricing:** Free. **Open source:** yes, once the repository is public.

**Topics** (choose from Product Hunt's list when you create the post):
Developer Tools, Linux, Windows, Open Source, Productivity.

**Platforms:** Linux and Windows (desktop, 64-bit). Not macOS: it is
unfinished, so don't list it.

### Images

| Product Hunt field | File | Size |
| --- | --- | --- |
| Thumbnail | `site/press/thumbnail.png` | 240×240 |
| Gallery 1 | `site/press/gallery/1-hero.png` | 1270×760 |
| Gallery 2 | `site/press/gallery/2-modules.png` | 1270×760 |
| Gallery 3 | `site/press/gallery/3-features.png` | 1270×760 |
| Gallery 4 | `site/press/gallery/4-security.png` | 1270×760 |
| Gallery 5 | `site/press/gallery/5-updates.png` | 1270×760 |
| Gallery 6 | `site/press/gallery/6-platforms.png` | 1270×760 |
| Social card (og:image) | `site/assets/og.png` | 1200×630 |

The gallery slides use HTML illustrations of the app, not screenshots. Real
screenshots make a stronger gallery: capture the ones listed in
[`site/assets/screenshots/README.md`](../site/assets/screenshots/README.md) and
put two or three of them between the slides, for example hero, Dashboard on
Linux, Dashboard on Windows, modules, Nginx & SSL screenshot, security, Linux
and Windows, updates. Show both systems: a gallery of only Linux windows
undercuts a "Linux and Windows" tagline.
A short demo video (60–90 seconds on YouTube: add a site and open it over
HTTPS, start a database and browse it, run the Cleaner) can go first in the
gallery and usually does more than any still.

To change a slide, edit its template in `site/press/` (`og.html`,
`gallery-*.html`), then re-render every image with:

```sh
site/press/render.sh          # needs google-chrome or chromium
```

The slides take their window, wizard, cleaner, terminal and manifest
illustrations from `site/index.html`, so edits to those on the page show up in
the images on the next render.

### Maker's first comment (draft)

Post this as soon as the launch goes live. The bracketed parts are yours to
fill in; the rest describes what the app does today.

> Hi Product Hunt! I'm [name], and I built DevsPro because my local dev setup was spread across a dozen terminals and config files: nginx vhosts, certificates, `docker compose`, a mail catcher, cron jobs, `tail -f` on three logs, and a note to myself about which port was which. [One or two sentences of your own story.]
>
> DevsPro puts that in one window. Add a site and it gets nginx config and a locally-trusted HTTPS certificate. Start MySQL, PostgreSQL, MongoDB or Redis with an admin UI. Keep queue workers and cron tasks running, catch outgoing mail, see what's listening on which port, and find out where your disk space went. There's also a terminal with split panes, SSH and SFTP, a database browser, an HTTP client and a launcher for terminal AI agents.
>
> The part I care most about is how it uses root and administrator rights. The app itself never runs elevated. Anything that needs it goes through one small helper, started with `pkexec` on Linux and a UAC prompt on Windows, that can only do a fixed list of operations: write a file under one allowed prefix, sync a marked block in the hosts file, reload nginx, trust the local CA, and a few more. There is no "run this command" operation. Updates are signed, and the helper checks the signature itself, and refuses downgrades, before it installs anything.
>
> It's free and MIT licensed, for Linux (`.deb` and `.rpm`) and Windows (an installer). Every module works on both; where Windows genuinely differs, like PHP running as `php-cgi.exe` or Firefox needing one setting to trust the local CA, the site and the app say so. One honest caveat: the Windows installer isn't code-signed yet, so SmartScreen warns the first time; More info, then Run anyway. I'd love to hear what you'd want it to manage next, and what broke on your distro or Windows setup.

Product Hunt's rules forbid asking for upvotes, here or anywhere else. Ask
for feedback and questions instead.

## Website switches

All in [`site/assets/config.js`](../site/assets/config.js). Commit and push to
`main`; the workflow redeploys in about a minute.

| Setting | Effect |
| --- | --- |
| `productHunt.url` + `productHunt.launchDate` (with `live: false`) | Banner on every page: "DevsPro launches on Product Hunt on 12 November. Get notified", linked to the post |
| `productHunt.live: true` | Banner: "DevsPro is live on Product Hunt. Come say hi" |
| `productHunt.postId` | The official "Featured on Product Hunt" badge under the hero buttons |
| `screenshots.hero`, `.wizard`, `.cleaner`, `.agents` | Replace that illustration with a real screenshot from `site/assets/screenshots/` |

The post ID is in the embed code Product Hunt gives you for the post
(`...featured.svg?post_id=123456...`).

## What the site reads, and who writes it

| File | Written by | Read by |
| --- | --- | --- |
| `site/latest.json` | `scripts/release.mjs`, at release time | The in-app updater; the Download buttons, the version label and the install commands. Packages: `linux-x86_64-deb`, `linux-x86_64-rpm`, `windows-x86_64-nsis` |
| `site/changelog.json` | `scripts/release.mjs`, from `CHANGELOG.md` | `changelog.html` |

Don't edit either by hand; edit `CHANGELOG.md` and re-run the release script.
When `latest.json` has no package for a format, or doesn't load, that format's
Download buttons fall back to `https://github.com/Soab42/devspro/releases/latest`.

The landing page guesses the visitor's system before it paints (`data-os` on
`<html>`, from `navigator.userAgentData.platform`, then `navigator.platform`
and the user agent). Windows visitors get "Download for Windows" first, Linux
visitors the `.deb` and `.rpm`, and both get an "Also for" link to the other
system. Anyone else, including phones, sees all three. The install section's
Linux / Windows tabs open on the same guess.

## Checklist

### Two weeks before

- [ ] Make the repository public, or decide where the site and the release
      files will live instead (see the top of this file).
- [ ] Enable Pages: repository Settings → Pages → Build and deployment →
      Source: **GitHub Actions**. Then run the "Deploy site" workflow (Actions
      tab → Deploy site → Run workflow) and open the site.
- [ ] Ship the first release that has in-app updates, using
      `scripts/release.mjs`. Everyone who installed an earlier version
      installs this one by hand; after that they update in-app.
- [ ] Build the Windows installer on Windows. There is no cross-building:
      `npm run package` on Linux makes the `.deb` and `.rpm`, and on Windows
      the NSIS `.exe`. Sign it with the same key as the Linux packages,

      ```sh
      npx tauri signer sign -f <path-to-private-key> DevsPro_<version>_x64-setup.exe
      ```

      and upload the `.exe` and its signature to the same GitHub release as
      the Linux packages **before** the new `latest.json` goes live. Otherwise
      Windows users get a feed entry pointing at a file that isn't there yet.
- [ ] On the live site, check that `latest.json` lists all three packages
      (`linux-x86_64-deb`, `linux-x86_64-rpm`, `windows-x86_64-nsis`) and that
      the Download buttons point at them.
- [ ] Install the released `.deb` on a clean Ubuntu or Pop!_OS VM, the `.rpm`
      on Fedora, and the `.exe` on a clean Windows 11 VM. On Windows, expect
      SmartScreen (More info → Run anyway), then a UAC prompt, and check that
      WebView2 gets installed if it's missing. Open Settings → Elevation
      helper → Test elevation on each.
- [ ] Test an in-app update on each system: install the previous version,
      publish the new one, and update from Settings → Updates. On Windows
      that's one UAC prompt and the installer's progress bar, then DevsPro
      restarts.
- [ ] Capture real screenshots with demo data, on Linux and on Windows
      (`site/assets/screenshots/README.md`), set them in `config.js`, and add
      them to the gallery.
- [ ] Re-render the images (`site/press/render.sh`) if you changed any
      template or illustration.
- [ ] Paste the site URL into a link-preview checker (opengraph.xyz, the
      LinkedIn Post Inspector) and check the card shows `og.png`.
- [ ] Create the Product Hunt post and schedule it. Launches start at 12:01 a.m.
      Pacific; Tuesday to Thursday are the busiest days, and a weekend launch
      faces less competition.
- [ ] Put the post URL and date in `config.js` (`url`, `launchDate`) so the
      site shows the "launches on" banner.

### Launch day

- [ ] At 12:01 a.m. Pacific: set `productHunt.live: true` and `postId` in
      `config.js`, push to `main`, and check the banner and badge on the live
      site.
- [ ] Post the maker's comment.
- [ ] Share the post where people already know you. Link to the post itself,
      not a request to upvote.
- [ ] Answer every comment during the day; note bug reports as you go.

### After

- [ ] A week later, set `live: false` and clear `launchDate`. The banner goes
      away; keep `postId` so the "Featured on Product Hunt" badge stays.
- [ ] Put fixes and requests from the launch into `CHANGELOG.md` under
      `## Unreleased`, and ship them. Every user now gets the update in-app.

## Preview the site locally

```sh
python3 -m http.server -d site 8000
# open http://localhost:8000/
```

It has to be served over HTTP: opened as a `file://` page, the browser blocks
the requests for `latest.json` and `changelog.json`, and the page falls back to
its static text.

## Changing the site's address

The address `https://soab42.github.io/devspro/` is written in:

- the `canonical`, `og:url`, `og:image` and `twitter:image` tags in
  `site/index.html` and `site/changelog.html`
- `site/sitemap.xml` and `site/robots.txt`
- `site/press/og.html` (the URL printed on the social card)
- the updater feed URL compiled into the app, which reads `latest.json` from
  this site

For a custom domain, add a `site/CNAME` file containing the domain, set it in
Settings → Pages, change the places above, and re-render the images. Links
inside the site are relative, so nothing else needs to change. `robots.txt`
only takes effect at a domain's root, so it starts working with a custom
domain.
