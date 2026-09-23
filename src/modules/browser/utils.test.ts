import { describe, expect, it } from "vitest";
import type { Bookmark, HistoryEntry } from "./api";
import {
  DEVICES,
  allDevices,
  buildSuggestions,
  clampViewport,
  customToDevice,
  deviceOptionLabel,
  parseCustomDevices,
  viewportOf,
  deviceById,
  displayUrl,
  hostOf,
  isSecure,
  nextZoom,
  originOf,
  parseSession,
  resolveInput,
  serializeSession,
  tabLabel,
  userAgentFor,
} from "./utils";

describe("resolveInput", () => {
  it("keeps full URLs", () => {
    expect(resolveInput(" https://example.com/x?y=1 ")).toBe("https://example.com/x?y=1");
    expect(resolveInput("http://localhost:1420")).toBe("http://localhost:1420");
  });

  it("adds a scheme to host-like input, http for local hosts", () => {
    expect(resolveInput("example.com")).toBe("https://example.com");
    expect(resolveInput("docs.rs/tauri")).toBe("https://docs.rs/tauri");
    expect(resolveInput("localhost:8080")).toBe("http://localhost:8080");
    expect(resolveInput("127.0.0.1:3000/api")).toBe("http://127.0.0.1:3000/api");
    expect(resolveInput("myapp.test")).toBe("https://myapp.test");
  });

  it("searches everything else", () => {
    expect(resolveInput("tauri child webview")).toContain("duckduckgo.com/?q=tauri%20child%20webview");
    expect(resolveInput("file:///etc/passwd")).toContain("?q=file");
    expect(resolveInput("")).toBe("https://duckduckgo.com/");
  });
});

describe("url helpers", () => {
  it("hostOf drops www and survives garbage", () => {
    expect(hostOf("https://www.example.com/a/b")).toBe("example.com");
    expect(hostOf("nope")).toBe("nope");
  });

  it("originOf keeps scheme, host and port; null for non-web", () => {
    expect(originOf("https://www.example.com:8443/a?b")).toBe("https://www.example.com:8443");
    expect(originOf("http://localhost:3000/login")).toBe("http://localhost:3000");
    expect(originOf("about:blank")).toBeNull();
    expect(originOf("")).toBeNull();
  });

  it("displayUrl and isSecure", () => {
    expect(displayUrl("https://example.com/")).toBe("example.com");
    expect(displayUrl("http://localhost:3000/x")).toBe("localhost:3000/x");
    expect(isSecure("https://a")).toBe(true);
    expect(isSecure("http://a")).toBe(false);
  });

  it("tabLabel prefers the title, then the host", () => {
    expect(tabLabel("  Docs ", "https://x.test")).toBe("Docs");
    expect(tabLabel("", "https://www.x.test/y")).toBe("x.test");
    expect(tabLabel("", "")).toBe("New tab");
  });
});

describe("nextZoom", () => {
  it("steps through the ladder and clamps", () => {
    expect(nextZoom(1, 1)).toBe(1.1);
    expect(nextZoom(1, -1)).toBe(0.9);
    expect(nextZoom(2, 1)).toBe(2);
    expect(nextZoom(0.5, -1)).toBe(0.5);
    expect(nextZoom(1.234, 1)).toBe(1.1);
  });
});

describe("user agents", () => {
  it("a device with its own UA wins over the skin; otherwise the skin decides", () => {
    const responsive = deviceById("responsive");
    expect(userAgentFor("chrome", responsive)).toContain("Chrome/");
    expect(userAgentFor("firefox", responsive)).toContain("Firefox/");
    expect(userAgentFor("safari", responsive)).toContain("Safari/");
    expect(userAgentFor("safari", responsive)).not.toContain("Chrome/");
    expect(userAgentFor("chrome", deviceById("iphone-15"))).toContain("iPhone");
    expect(userAgentFor("safari", deviceById("pixel-9"))).toContain("Android");
    expect(userAgentFor("firefox", deviceById("macbook-pro-14"))).toContain("Firefox/");
  });

  it("unknown device ids fall back to full size", () => {
    expect(deviceById("nope")).toBe(DEVICES[0]);
    expect(viewportOf(DEVICES[0])).toBeNull();
    expect(viewportOf(deviceById("iphone-15"))).toEqual({ width: 393, height: 852 });
  });

  it("presets cover every kind with unique ids", () => {
    const kinds = new Set(DEVICES.map((d) => d.kind));
    for (const k of ["phone", "tablet", "laptop", "desktop"]) expect(kinds.has(k as never)).toBe(true);
    expect(new Set(DEVICES.map((d) => d.id)).size).toBe(DEVICES.length);
    expect(deviceOptionLabel(deviceById("iphone-15"))).toBe("Phone · iPhone 15 · 393×852");
    expect(deviceOptionLabel(DEVICES[0])).toBe("Full size");
  });
});

describe("custom devices", () => {
  it("parses only well-formed entries", () => {
    const good = { id: "c1", label: "Kiosk", width: 1080, height: 1920 };
    const raw = JSON.stringify([good, { id: "x" }, { id: "c2", label: "neg", width: -1, height: 5 }, null]);
    expect(parseCustomDevices(raw)).toEqual([good]);
    expect(parseCustomDevices(null)).toEqual([]);
    expect(parseCustomDevices("nope")).toEqual([]);
    expect(parseCustomDevices("{}")).toEqual([]);
  });

  it("joins presets and custom sizes, and custom ids resolve", () => {
    const custom = [{ id: "c1", label: "Kiosk", width: 1080, height: 1920 }];
    const all = allDevices(custom);
    expect(all).toHaveLength(DEVICES.length + 1);
    expect(deviceById("c1", custom)).toEqual(customToDevice(custom[0]));
    expect(deviceOptionLabel(customToDevice(custom[0]))).toBe("Custom · Kiosk · 1080×1920");
  });

  it("clampViewport rounds and enforces the minimum", () => {
    expect(clampViewport({ width: 10, height: 300.6 })).toEqual({ width: 200, height: 301 });
  });
});

describe("buildSuggestions", () => {
  const bookmarks: Bookmark[] = [
    { id: "b1", url: "https://github.com/", title: "GitHub", position: 0, created_at: "" },
    { id: "b2", url: "https://docs.rs/", title: "Docs.rs", position: 1, created_at: "" },
  ];
  const history: HistoryEntry[] = [
    { id: "h1", url: "https://github.com/", title: "GitHub", visits: 9, visited_at: "" },
    { id: "h2", url: "https://github.com/tauri-apps", title: "", visits: 2, visited_at: "" },
  ];

  it("leads with the resolved input, then bookmarks, then history, deduplicated", () => {
    const s = buildSuggestions("github", bookmarks, history);
    expect(s[0]).toMatchObject({ kind: "search" });
    expect(s[0].title).toContain('"github"');
    expect(s[1]).toMatchObject({ kind: "bookmark", url: "https://github.com/" });
    expect(s[2]).toMatchObject({ kind: "history", url: "https://github.com/tauri-apps", title: "github.com" });
    expect(s).toHaveLength(3);
  });

  it("treats a host-like input as a URL suggestion", () => {
    const s = buildSuggestions("docs.rs", bookmarks, history);
    expect(s[0]).toMatchObject({ kind: "url", url: "https://docs.rs", title: "docs.rs" });
    expect(s[1]).toMatchObject({ kind: "bookmark", url: "https://docs.rs/" });
  });

  it("is empty for empty input and honours the cap", () => {
    expect(buildSuggestions("   ", bookmarks, history)).toEqual([]);
    expect(buildSuggestions("github", bookmarks, history, 2)).toHaveLength(2);
  });
});

describe("session persistence", () => {
  it("serializes open web URLs and the active index", () => {
    const tabs = [{ url: "https://a.test/" }, { url: "about:blank" }, { url: "http://localhost:3000/" }];
    const ids = ["1", "2", "3"];
    expect(serializeSession(tabs, "3", ids)).toEqual({ urls: ["https://a.test/", "http://localhost:3000/"], active: 2 });
    expect(serializeSession(tabs, null, ids).active).toBe(0);
  });

  it("parses what it wrote and rejects anything else", () => {
    expect(parseSession(JSON.stringify({ urls: ["https://a.test/"], active: 0 }))).toEqual({ urls: ["https://a.test/"], active: 0 });
    expect(parseSession(JSON.stringify({ urls: ["file:///x", "https://b.test/"], active: 9 }))).toEqual({ urls: ["https://b.test/"], active: 0 });
    expect(parseSession(null)).toBeNull();
    expect(parseSession("garbage")).toBeNull();
    expect(parseSession(JSON.stringify({ urls: [] }))).toBeNull();
    expect(parseSession(JSON.stringify({ urls: "x" }))).toBeNull();
  });
});
