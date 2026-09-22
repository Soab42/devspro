export type Skin = "chrome" | "safari" | "firefox";

export const SKINS: { value: Skin; label: string }[] = [
  { value: "chrome", label: "Chrome" },
  { value: "safari", label: "Safari" },
  { value: "firefox", label: "Firefox" },
];

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
  const hostLike = /^(localhost|[\w-]+(\.[\w-]+)+|\d{1,3}(\.\d{1,3}){3})(:\d+)?(\/.*)?$/i;
  if (!/\s/.test(text) && hostLike.test(text)) {
    // Local hosts are plain http: nothing on localhost has a real cert, and
    // DevsPro's own sites are `*.test` domains with the local CA.
    const scheme = /^(localhost|127\.|10\.|192\.168\.)/i.test(text) ? "http" : "https";
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

export const ZOOM_STEPS = [0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];

export function nextZoom(current: number, direction: -1 | 1): number {
  const idx = ZOOM_STEPS.findIndex((z) => Math.abs(z - current) < 0.001);
  const at = idx === -1 ? ZOOM_STEPS.indexOf(1) : idx;
  const next = Math.min(ZOOM_STEPS.length - 1, Math.max(0, at + direction));
  return ZOOM_STEPS[next];
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
