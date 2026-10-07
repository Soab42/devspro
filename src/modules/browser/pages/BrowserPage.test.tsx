import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { ModulesProvider } from "../../../app/ModulesContext";
import { resetStore, setState } from "../store";
import BrowserPage from "./BrowserPage";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => undefined) }));

class NoopObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

describe("BrowserPage zoom keys", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", NoopObserver);
    invoke.mockReset();
    invoke.mockResolvedValue([]);
    localStorage.clear();
    resetStore();
    setState({
      tabs: [{ id: "t1", url: "https://example.com", title: "", loading: false, zoom: 1.25, userAgent: "ua" }],
      activeId: "t1",
    });
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  const zoomCalls = () => invoke.mock.calls.filter(([cmd]) => cmd === "browser_set_zoom");

  function mount() {
    const { container } = render(
      <MemoryRouter>
        <ModulesProvider>
          <BrowserPage />
        </ModulesProvider>
      </MemoryRouter>,
    );
    const root = container.querySelector("[data-own-zoom-keys]") as HTMLElement;
    expect(root).not.toBeNull();
    return root;
  }

  it.each([
    ["Ctrl+0 resets", { key: "0" }, 1],
    ["Ctrl+Numpad0 resets", { key: "Insert", code: "Numpad0" }, 1],
    ["Ctrl+- zooms out", { key: "-" }, 1.1],
    ["Ctrl+_ zooms out", { key: "_", shiftKey: true }, 1.1],
    ["Ctrl+= zooms in", { key: "=" }, 1.5],
    ["Ctrl+NumpadAdd zooms in", { key: "+", code: "NumpadAdd" }, 1.5],
  ])("%s the page", async (_name, init, factor) => {
    const root = mount();
    fireEvent.keyDown(root, { ctrlKey: true, ...init });
    await waitFor(() => expect(zoomCalls()).toHaveLength(1));
    expect(zoomCalls()[0][1]).toEqual({ id: "t1", factor: expect.closeTo(factor, 5) });
  });

  it("ignores a plain 0 without Ctrl", () => {
    const root = mount();
    fireEvent.keyDown(root, { key: "0" });
    expect(zoomCalls()).toHaveLength(0);
  });
});
