import { describe, expect, it } from "vitest";
import { displayUrl, hostOf, isSecure, nextZoom, resolveInput, tabLabel } from "./utils";

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

describe("display helpers", () => {
  it("shows the host without www and the URL without scheme", () => {
    expect(hostOf("https://www.example.com/a/b")).toBe("example.com");
    expect(hostOf("not a url")).toBe("not a url");
    expect(displayUrl("https://example.com/")).toBe("example.com");
    expect(displayUrl("http://localhost:8080/x")).toBe("localhost:8080/x");
    expect(isSecure("https://a")).toBe(true);
    expect(isSecure("http://a")).toBe(false);
  });

  it("labels tabs by title, then host", () => {
    expect(tabLabel("  Docs ", "https://x.y")).toBe("Docs");
    expect(tabLabel("", "https://www.x.y/z")).toBe("x.y");
    expect(tabLabel("", "")).toBe("New tab");
  });

  it("steps zoom through fixed stops", () => {
    expect(nextZoom(1, 1)).toBe(1.1);
    expect(nextZoom(1, -1)).toBe(0.9);
    expect(nextZoom(2, 1)).toBe(2);
    expect(nextZoom(0.5, -1)).toBe(0.5);
    expect(nextZoom(0.93, 1)).toBe(1.1);
  });
});
