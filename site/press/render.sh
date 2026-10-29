#!/usr/bin/env bash
# Re-renders every PNG the site and the Product Hunt launch use, from the HTML
# templates in this folder, with headless Google Chrome (or Chromium).
#
#   site/press/render.sh
#
# Output:
#   site/assets/og.png                 1200×630  social card (og:image)
#   site/press/gallery/*.png           1270×760  Product Hunt gallery
#   site/press/thumbnail.png           240×240   Product Hunt thumbnail
#   site/assets/apple-touch-icon.png   180×180
#   site/assets/icon-192.png, icon-512.png, favicon-32.png
set -euo pipefail

PRESS="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SITE="$(dirname "$PRESS")"
PORT="${PORT:-8766}"

CHROME="${CHROME:-}"
if [[ -z "$CHROME" ]]; then
  for c in google-chrome google-chrome-stable chromium chromium-browser; do
    if command -v "$c" >/dev/null 2>&1; then CHROME="$c"; break; fi
  done
fi
[[ -n "$CHROME" ]] || { echo "render.sh: no Chrome or Chromium found (set CHROME=...)" >&2; exit 1; }

PROFILE="$(mktemp -d)"
# The templates fetch ../index.html, which a file:// page can't do.
python3 -m http.server "$PORT" --bind 127.0.0.1 -d "$SITE" >/dev/null 2>&1 &
SERVER=$!
trap 'kill "$SERVER" 2>/dev/null || true; rm -rf "$PROFILE"' EXIT

for _ in $(seq 50); do
  curl -fs "http://127.0.0.1:$PORT/press/og.html" >/dev/null 2>&1 && break
  sleep 0.1
done

# shot <page> <width> <height> <output> [transparent]
shot() {
  local extra=()
  [[ "${5:-}" == transparent ]] && extra+=(--default-background-color=00000000)
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --no-first-run \
    --user-data-dir="$PROFILE" --force-device-scale-factor=1 \
    --window-size="$2,$3" --virtual-time-budget=5000 "${extra[@]}" \
    --screenshot="$4" "http://127.0.0.1:$PORT/$1" >/dev/null 2>&1
  echo "  $4"
}

mkdir -p "$PRESS/gallery"
echo "Rendering:"
shot press/og.html 1200 630 "$SITE/assets/og.png"
shot press/gallery-1-hero.html 1270 760 "$PRESS/gallery/1-hero.png"
shot press/gallery-2-modules.html 1270 760 "$PRESS/gallery/2-modules.png"
shot press/gallery-3-features.html 1270 760 "$PRESS/gallery/3-features.png"
shot press/gallery-4-security.html 1270 760 "$PRESS/gallery/4-security.png"
shot press/gallery-5-updates.html 1270 760 "$PRESS/gallery/5-updates.png"
shot press/gallery-6-platforms.html 1270 760 "$PRESS/gallery/6-platforms.png"
shot "press/icon.html?bleed" 240 240 "$PRESS/thumbnail.png"
shot "press/icon.html?bleed" 180 180 "$SITE/assets/apple-touch-icon.png"
shot press/icon.html 512 512 "$SITE/assets/icon-512.png" transparent
shot press/icon.html 192 192 "$SITE/assets/icon-192.png" transparent
shot press/icon.html 32 32 "$SITE/assets/favicon-32.png" transparent
