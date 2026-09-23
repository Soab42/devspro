import type { Bookmark, HistoryEntry } from "./api";

export type Skin = "chrome" | "safari" | "firefox";

/**
 * The skin is also the identity a tab presents to the site. The engine is
 * WebKitGTK either way — Chrome and Firefox can't be embedded without
 * shipping them — but a site's "is this Chrome?" branches, polyfill choices
 * and mobile redirects all key off the user-agent string, and that is what
 * most cross-browser bugs are made of.
 */
export const SKINS: { value: Skin; label: string; userAgent: string }[] = [
  {
    value: "chrome",
    label: "Chrome",
    userAgent:
      "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
  },
  {
    value: "safari",
    label: "Safari",
    userAgent:
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15",
  },
  {
    value: "firefox",
    label: "Firefox",
    userAgent:
      "Mozilla/5.0 (X11; Linux x86_64; rv:143.0) Gecko/20100101 Firefox/143.0",
  },
];

export function isSkin(v: unknown): v is Skin {
  return v === "chrome" || v === "safari" || v === "firefox";
}

export type DeviceKind = "phone" | "tablet" | "laptop" | "desktop" | "custom";

/**
 * Viewport presets. `null` width means "fill the pane". A device that has a
 * user-agent of its own also changes what the site is told, so the mobile
 * layout a site serves by UA sniffing shows up too, not just the CSS one.
 * Sizes are CSS pixels (the device's logical viewport), which is what a
 * page's media queries see.
 */
export interface Device {
  id: string;
  label: string;
  kind: DeviceKind;
  width: number | null;
  height: number | null;
  userAgent?: string;
}

export interface Viewport {
  width: number;
  height: number;
}

const UA_IPHONE =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";
const UA_IPAD =
  "Mozilla/5.0 (iPad; CPU OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";
const UA_ANDROID = (model: string) =>
  `Mozilla/5.0 (Linux; Android 15; ${model}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36`;
const UA_ANDROID_TABLET = (model: string) =>
  `Mozilla/5.0 (Linux; Android 15; ${model}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36`;

export const FULL_SIZE_ID = "responsive";

export const DEVICES: Device[] = [
  {
    id: FULL_SIZE_ID,
    label: "Full size",
    kind: "desktop",
    width: null,
    height: null,
  },
  // phones
  {
    id: "iphone-se",
    label: "iPhone SE",
    kind: "phone",
    width: 375,
    height: 667,
    userAgent: UA_IPHONE,
  },
  {
    id: "iphone-15",
    label: "iPhone 15",
    kind: "phone",
    width: 393,
    height: 852,
    userAgent: UA_IPHONE,
  },
  {
    id: "iphone-15-pro-max",
    label: "iPhone 15 Pro Max",
    kind: "phone",
    width: 430,
    height: 932,
    userAgent: UA_IPHONE,
  },
  {
    id: "pixel-9",
    label: "Pixel 9",
    kind: "phone",
    width: 412,
    height: 915,
    userAgent: UA_ANDROID("Pixel 9"),
  },
  {
    id: "galaxy-s24",
    label: "Galaxy S24",
    kind: "phone",
    width: 360,
    height: 780,
    userAgent: UA_ANDROID("SM-S921B"),
  },
  {
    id: "galaxy-a15",
    label: "Galaxy A15",
    kind: "phone",
    width: 384,
    height: 854,
    userAgent: UA_ANDROID("SM-A155F"),
  },
  {
    id: "galaxy-fold",
    label: "Galaxy Z Fold (open)",
    kind: "phone",
    width: 673,
    height: 841,
    userAgent: UA_ANDROID("SM-F956B"),
  },
  // tablets
  {
    id: "ipad-mini",
    label: "iPad Mini",
    kind: "tablet",
    width: 744,
    height: 1133,
    userAgent: UA_IPAD,
  },
  {
    id: "ipad",
    label: "iPad",
    kind: "tablet",
    width: 820,
    height: 1180,
    userAgent: UA_IPAD,
  },
  {
    id: "ipad-air",
    label: 'iPad Air 11"',
    kind: "tablet",
    width: 834,
    height: 1194,
    userAgent: UA_IPAD,
  },
  {
    id: "ipad-pro-13",
    label: 'iPad Pro 13"',
    kind: "tablet",
    width: 1024,
    height: 1366,
    userAgent: UA_IPAD,
  },
  {
    id: "galaxy-tab",
    label: "Galaxy Tab S9",
    kind: "tablet",
    width: 800,
    height: 1280,
    userAgent: UA_ANDROID_TABLET("SM-X710"),
  },
  {
    id: "surface-pro",
    label: "Surface Pro",
    kind: "tablet",
    width: 912,
    height: 1368,
  },
  // laptops
  {
    id: "macbook-air-13",
    label: 'MacBook Air 13"',
    kind: "laptop",
    width: 1280,
    height: 832,
  },
  {
    id: "macbook-air-15",
    label: 'MacBook Air 15"',
    kind: "laptop",
    width: 1440,
    height: 932,
  },
  {
    id: "macbook-pro-14",
    label: 'MacBook Pro 14"',
    kind: "laptop",
    width: 1512,
    height: 982,
  },
  {
    id: "macbook-pro-16",
    label: 'MacBook Pro 16"',
    kind: "laptop",
    width: 1728,
    height: 1117,
  },
  {
    id: "laptop-hd",
    label: "Laptop 1366×768",
    kind: "laptop",
    width: 1366,
    height: 768,
  },
  {
    id: "laptop-1536",
    label: "Laptop 1536×864",
    kind: "laptop",
    width: 1536,
    height: 864,
  },
  // desktops
  {
    id: "desktop-1080",
    label: "Desktop 1080p",
    kind: "desktop",
    width: 1920,
    height: 1080,
  },
  {
    id: "imac-24",
    label: 'iMac 24"',
    kind: "desktop",
    width: 2240,
    height: 1260,
  },
  {
    id: "desktop-1440",
    label: "Desktop 1440p",
    kind: "desktop",
    width: 2560,
    height: 1440,
  },
  {
    id: "desktop-4k",
    label: "Desktop 4K",
    kind: "desktop",
    width: 3840,
    height: 2160,
  },
];

const KIND_LABEL: Record<DeviceKind, string> = {
  phone: "Phone",
  tablet: "Tablet",
  laptop: "Laptop",
  desktop: "Desktop",
  custom: "Custom",
};

/** "Phone · iPhone 15 · 393×852" — the dropdown text; the kind prefix keeps a long list scannable. */
export function deviceOptionLabel(d: Device): string {
  if (d.width === null || d.height === null) return d.label;
  return `${KIND_LABEL[d.kind]} · ${d.label} · ${d.width}×${d.height}`;
}

/** A user-defined size. Kept in localStorage; every window sees the same list. */
export interface CustomDevice {
  id: string;
  label: string;
  width: number;
  height: number;
}

export const CUSTOM_DEVICES_KEY = "devspro.browser.customDevices";

export function parseCustomDevices(raw: string | null): CustomDevice[] {
  if (!raw) return [];
  try {
    const v: unknown = JSON.parse(raw);
    if (!Array.isArray(v)) return [];
    return v.filter(
      (d): d is CustomDevice =>
        !!d &&
        typeof d === "object" &&
        typeof (d as CustomDevice).id === "string" &&
        typeof (d as CustomDevice).label === "string" &&
        Number.isFinite((d as CustomDevice).width) &&
        Number.isFinite((d as CustomDevice).height) &&
        (d as CustomDevice).width > 0 &&
        (d as CustomDevice).height > 0,
    );
  } catch {
    return [];
  }
}

export function customToDevice(c: CustomDevice): Device {
  return {
    id: c.id,
    label: c.label,
    kind: "custom",
    width: c.width,
    height: c.height,
  };
}

/** Presets first, then the user's own sizes. */
export function allDevices(custom: CustomDevice[]): Device[] {
  return [...DEVICES, ...custom.map(customToDevice)];
}

export function deviceById(id: string, custom: CustomDevice[] = []): Device {
  return allDevices(custom).find((d) => d.id === id) ?? DEVICES[0];
}

/** The size a device pins the viewport to; null for full size. */
export function viewportOf(d: Device): Viewport | null {
  return d.width === null || d.height === null
    ? null
    : { width: d.width, height: d.height };
}

export const MIN_VIEWPORT = 200;

export function clampViewport(v: Viewport): Viewport {
  return {
    width: Math.max(MIN_VIEWPORT, Math.round(v.width)),
    height: Math.max(MIN_VIEWPORT, Math.round(v.height)),
  };
}

/** The UA a tab should present: the device's if it has one, else the skin's. */
export function userAgentFor(skin: Skin, device: Device): string {
  return (
    device.userAgent ??
    SKINS.find((s) => s.value === skin)?.userAgent ??
    SKINS[0].userAgent
  );
}

export const DEFAULT_URL = "https://duckduckgo.com/";
const SEARCH_URL = "https://duckduckgo.com/?q=";

/**
 * What the address bar does with what you typed, in the order a browser does
 * it: a full URL is used as-is, something that looks like a host (a dot, or
 * localhost, or an explicit port) gets https:// in front, and anything else is
 * a search.
 */
export function resolveInput(raw: string): string {
  const text = raw.trim();
  if (!text) return DEFAULT_URL;
  if (/^https?:\/\//i.test(text)) return text;
  if (/^[a-z][a-z0-9+.-]*:/i.test(text) && !/^localhost:/i.test(text)) {
    // Some other scheme (file:, mailto:). Not ours; search for it instead.
    return `${SEARCH_URL}${encodeURIComponent(text)}`;
  }
  const hostLike =
    /^(localhost|[\w-]+(\.[\w-]+)+|\d{1,3}(\.\d{1,3}){3})(:\d+)?(\/.*)?$/i;
  if (!/\s/.test(text) && hostLike.test(text)) {
    // Local hosts are plain http: nothing on localhost has a real cert, and
    // DevsPro's own sites are `*.test` domains with the local CA.
    const scheme = /^(localhost|127\.|10\.|192\.168\.)/i.test(text)
      ? "http"
      : "https";
    return `${scheme}://${text}`;
  }
  return `${SEARCH_URL}${encodeURIComponent(text)}`;
}

/** "https://www.example.com/a/b?c" -> "example.com", for the Safari-style compact bar. */
export function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

/** "https://www.example.com:8443/a?b" -> "https://www.example.com:8443"; null for non-web URLs. */
export function originOf(url: string): string | null {
  try {
    const u = new URL(url);
    if (u.protocol !== "http:" && u.protocol !== "https:") return null;
    return u.origin;
  } catch {
    return null;
  }
}

/** The address bar text: scheme and trailing slash dropped, like every browser. */
export function displayUrl(url: string): string {
  return url.replace(/^https?:\/\//i, "").replace(/\/$/, "");
}

export function isSecure(url: string): boolean {
  return /^https:\/\//i.test(url);
}

/** Tab label: the page title, else the host, else "New tab". */
export function tabLabel(title: string, url: string): string {
  if (title.trim()) return title.trim();
  const host = hostOf(url);
  return host || "New tab";
}

export const ZOOM_STEPS = [
  0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2,
];

export function nextZoom(current: number, direction: -1 | 1): number {
  const idx = ZOOM_STEPS.findIndex((z) => Math.abs(z - current) < 0.001);
  const at = idx === -1 ? ZOOM_STEPS.indexOf(1) : idx;
  const next = Math.min(ZOOM_STEPS.length - 1, Math.max(0, at + direction));
  return ZOOM_STEPS[next];
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

// ---- address bar suggestions ----

export interface Suggestion {
  kind: "bookmark" | "history" | "search" | "url";
  url: string;
  title: string;
}

/**
 * What the dropdown under the address bar shows for `input`: the resolved
 * input first (so Enter always does the obvious thing), then bookmarks that
 * match, then history. Duplicates by URL are dropped, first wins.
 */
export function buildSuggestions(
  input: string,
  bookmarks: Bookmark[],
  history: HistoryEntry[],
  max = 8,
): Suggestion[] {
  const text = input.trim();
  if (!text) return [];
  const resolved = resolveInput(text);
  const out: Suggestion[] = [
    {
      kind: resolved.startsWith(SEARCH_URL) ? "search" : "url",
      url: resolved,
      title: resolved.startsWith(SEARCH_URL)
        ? `Search for "${text}"`
        : displayUrl(resolved),
    },
  ];
  const seen = new Set<string>([resolved]);
  const needle = text.toLowerCase();
  const matches = (b: { url: string; title: string }) =>
    b.url.toLowerCase().includes(needle) ||
    b.title.toLowerCase().includes(needle);
  for (const b of bookmarks) {
    if (out.length >= max) break;
    if (seen.has(b.url) || !matches(b)) continue;
    seen.add(b.url);
    out.push({ kind: "bookmark", url: b.url, title: b.title });
  }
  for (const h of history) {
    if (out.length >= max) break;
    if (seen.has(h.url)) continue;
    seen.add(h.url);
    out.push({ kind: "history", url: h.url, title: h.title || hostOf(h.url) });
  }
  return out;
}

// ---- session persistence ----

export interface SavedSession {
  urls: string[];
  active: number;
}

export function serializeSession(
  tabs: { url: string }[],
  activeId: string | null,
  ids: string[],
): SavedSession {
  const active = Math.max(0, ids.indexOf(activeId ?? ""));
  return {
    urls: tabs.map((t) => t.url).filter((u) => /^https?:\/\//i.test(u)),
    active,
  };
}

export function parseSession(raw: string | null): SavedSession | null {
  if (!raw) return null;
  try {
    const v: unknown = JSON.parse(raw);
    if (!v || typeof v !== "object") return null;
    const { urls, active } = v as { urls?: unknown; active?: unknown };
    if (!Array.isArray(urls)) return null;
    const clean = urls.filter(
      (u): u is string => typeof u === "string" && /^https?:\/\//i.test(u),
    );
    if (clean.length === 0) return null;
    const idx =
      typeof active === "number" && active >= 0 && active < clean.length
        ? active
        : 0;
    return { urls: clean, active: idx };
  } catch {
    return null;
  }
}
