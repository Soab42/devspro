import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_ZOOM,
  OWN_ZOOM_ATTR,
  WHEEL_THROTTLE_MS,
  ZOOM_LEVELS,
  ZOOM_STORAGE_KEY,
  clampZoom,
  getZoom,
  getZoomFactor,
  installZoom,
  nextZoom,
  previousZoom,
  setZoom,
  wheelZoom,
  zoomKeyAction,
} from "./zoom";

describe("zoom steps", () => {
  it("has the specified levels", () => {
    expect([...ZOOM_LEVELS]).toEqual([67, 75, 80, 90, 100, 110, 125, 150, 175, 200]);
  });
  it("steps up and down through the levels", () => {
    expect(nextZoom(100)).toBe(110);
    expect(previousZoom(100)).toBe(90);
    expect(nextZoom(67)).toBe(75);
    expect(previousZoom(125)).toBe(110);
  });
  it("clamps at both ends", () => {
    expect(nextZoom(200)).toBe(200);
    expect(previousZoom(67)).toBe(67);
    expect(clampZoom(500)).toBe(200);
    expect(clampZoom(10)).toBe(67);
    expect(clampZoom(NaN)).toBe(DEFAULT_ZOOM);
  });
  it("snaps stray values to the nearest level before stepping", () => {
    expect(nextZoom(104)).toBe(110);
    expect(previousZoom(104)).toBe(90);
  });
  it("wheel up zooms in, wheel down zooms out", () => {
    expect(wheelZoom(100, -100)).toBe(110);
    expect(wheelZoom(100, 100)).toBe(90);
    expect(wheelZoom(100, 0)).toBe(100);
  });
});

describe("zoomKeyAction", () => {
  const k = (key: string, extra: Record<string, unknown> = {}) =>
    zoomKeyAction({ key, ctrlKey: true, ...extra });
  it("maps plus, minus and zero, with shift and numpad variants", () => {
    expect(k("=")).toBe("in");
    expect(k("+", { shiftKey: true })).toBe("in");
    expect(k("+", { code: "NumpadAdd" })).toBe("in");
    expect(k("-")).toBe("out");
    expect(k("-", { code: "NumpadSubtract" })).toBe("out");
    expect(k("0")).toBe("reset");
    expect(k("0", { code: "Numpad0" })).toBe("reset");
  });
  it("maps Ctrl+Shift+- (underscore) and every numpad variant", () => {
    expect(k("_", { shiftKey: true })).toBe("out");
    expect(k("Add", { code: "NumpadAdd" })).toBe("in");
    expect(k("Subtract", { code: "NumpadSubtract" })).toBe("out");
    expect(k("Insert", { code: "Numpad0" })).toBe("reset");
    expect(k("0", { code: "Digit0" })).toBe("reset");
    expect(zoomKeyAction({ key: "0", ctrlKey: false, metaKey: true })).toBe("reset");
  });
  it("ignores keys without Ctrl, or with Alt, or other keys", () => {
    expect(zoomKeyAction({ key: "=", ctrlKey: false })).toBeNull();
    expect(k("=", { altKey: true })).toBeNull();
    expect(k("c")).toBeNull();
  });
});

describe("installZoom handlers", () => {
  let teardown: () => void;
  beforeEach(() => {
    teardown?.();
    localStorage.clear();
    setZoom(100);
    teardown = installZoom();
  });

  const press = (key: string, init: KeyboardEventInit = {}) => {
    const e = new KeyboardEvent("keydown", { key, ctrlKey: true, cancelable: true, bubbles: true, ...init });
    document.body.dispatchEvent(e);
    return e;
  };

  it("steps on Ctrl+= / Ctrl+- and resets on Ctrl+0, persisting the level", () => {
    expect(press("=").defaultPrevented).toBe(true);
    expect(getZoom()).toBe(110);
    expect(getZoomFactor()).toBeCloseTo(1.1);
    expect(localStorage.getItem(ZOOM_STORAGE_KEY)).toBe("110");
    press("-");
    press("-");
    expect(getZoom()).toBe(90);
    press("0");
    expect(getZoom()).toBe(100);
    expect(localStorage.getItem(ZOOM_STORAGE_KEY)).toBeNull();
  });

  it("claims the key before an element handler sees it", () => {
    const seen = vi.fn();
    document.body.addEventListener("keydown", seen);
    press("-");
    document.body.removeEventListener("keydown", seen);
    expect(seen).not.toHaveBeenCalled();
  });

  it("leaves zoom keys to a region that owns them, like the Browser", () => {
    const own = document.createElement("div");
    own.setAttribute(OWN_ZOOM_ATTR, "");
    const inner = document.createElement("input");
    own.appendChild(inner);
    document.body.appendChild(own);
    const seen = vi.fn();
    own.addEventListener("keydown", seen);
    const e = new KeyboardEvent("keydown", { key: "=", ctrlKey: true, cancelable: true, bubbles: true });
    inner.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(false);
    expect(seen).toHaveBeenCalled();
    expect(getZoom()).toBe(100);
    // Outside the region the same key still zooms the UI.
    press("=");
    expect(getZoom()).toBe(110);
    own.remove();
  });

  it("leaves plain keys alone", () => {
    expect(press("-", { ctrlKey: false }).defaultPrevented).toBe(false);
    expect(getZoom()).toBe(100);
  });

  it("steps once per Ctrl+wheel gesture and prevents the default", () => {
    vi.useFakeTimers();
    vi.setSystemTime(10_000);
    const wheel = (deltaY: number, ctrlKey = true) => {
      const e = new WheelEvent("wheel", { deltaY, ctrlKey, cancelable: true, bubbles: true });
      document.body.dispatchEvent(e);
      return e;
    };
    expect(wheel(-100).defaultPrevented).toBe(true);
    expect(getZoom()).toBe(110);
    wheel(-100); // inside the throttle window
    expect(getZoom()).toBe(110);
    vi.setSystemTime(10_000 + WHEEL_THROTTLE_MS + 1);
    wheel(100);
    expect(getZoom()).toBe(100);
    expect(wheel(-100, false).defaultPrevented).toBe(false);
    vi.useRealTimers();
  });

  it("follows a change made in another window", () => {
    localStorage.setItem(ZOOM_STORAGE_KEY, "150");
    window.dispatchEvent(new StorageEvent("storage", { key: ZOOM_STORAGE_KEY }));
    expect(getZoom()).toBe(150);
  });
});

describe("unusable storage", () => {
  it("falls back to 100% and still zooms when storage throws", async () => {
    const boom = () => {
      throw new Error("denied");
    };
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(boom);
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(boom);
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(boom);
    vi.resetModules();
    const fresh = await import("./zoom");
    expect(fresh.getZoom()).toBe(100);
    const off = fresh.installZoom();
    expect(() => fresh.setZoom(125)).not.toThrow();
    expect(fresh.getZoom()).toBe(125);
    off();
    vi.restoreAllMocks();
  });
});
