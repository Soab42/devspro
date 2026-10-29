# Screenshots

Real screenshots of DevsPro for the landing page and the Product Hunt gallery.
Until they exist, the site shows HTML illustrations of the app, captioned as
illustrations.

## Using one on the page

1. Save the PNG here, e.g. `assets/screenshots/dashboard.png`.
2. Name it in `assets/config.js`:

   ```js
   screenshots: { hero: "assets/screenshots/dashboard.png", ... }
   ```

Each key replaces one illustration: `hero` (the big window under the
headline), `wizard`, `cleaner` and `agents` (the three feature rows).

## What to capture

| File | Module and state | Used for |
| --- | --- | --- |
| `dashboard.png` | Dashboard, dark theme, a few sites and containers running | `hero`, PH gallery 1 |
| `wizard.png` | Nginx & SSL → New Project wizard, mid-run with steps ticked | `wizard` |
| `cleaner.png` | Cleaner → Storage map, or the overview grouped by risk | `cleaner`, PH gallery |
| `agents.png` | AI agents or Terminal with two or three tabs | `agents` |
| `sites.png` | Nginx & SSL site list with HTTPS badges | PH gallery |
| `data-browser.png` | Data browser on a demo database | PH gallery |
| `updates.png` | Settings → Updates showing an available update | PH gallery |
| `windows-dashboard.png` | The Dashboard on Windows 11, dark theme | PH gallery, next to the Linux one |
| `windows-sites.png` | Nginx & SSL on Windows with a Laravel site on `php-cgi.exe` | PH gallery |
| `windows-updates.png` | Settings → Updates on Windows, with an update available | PH gallery |

## How

- The whole window only, at 1440×900 (the hero) or 1270×760 (PH gallery size),
  with UI zoom at 100 %. In GNOME, Alt+Print captures the focused window; on
  Windows, Win+Shift+S → Window mode, or Alt+Print Screen.
- Show at least one module on each system, so the gallery makes the
  "Linux and Windows" claim visibly.
- Dark theme for the hero; it matches the site.
- **Use demo data.** Make throwaway sites (`shop.test`, `api.test`), a demo
  database and a scratch project folder. Check every capture for real
  hostnames, IPs, paths under your home folder, usernames, email addresses,
  tokens and `.env` values before committing it. This folder is published.
- PNG, and keep each under about 500 KB (`oxipng -o 4` or `pngquant` help).
