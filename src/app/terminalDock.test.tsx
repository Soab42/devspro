import { beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import {
  DOCK_DEFAULT,
  DOCK_MIN,
  DOCK_OPEN_KEY,
  DOCK_WIDTH_KEY,
  clampDock,
  dockMax,
  isDockShortcut,
  useTerminalDock,
} from "./terminalDockState";

const invoke = vi.fn(async (_cmd: string, _args?: unknown): Promise<unknown> => null);
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invoke(cmd, args),
}));

let exitDock: (() => void) | null = null;
vi.mock("../ui/Terminal", () => ({
  default: ({ id, active, onExit }: { id: string; active?: boolean; onExit?: () => void }) => {
    exitDock = () => onExit?.();
    return <div data-testid="terminal" data-id={id} data-active={String(active)} />;
  },
}));

import { TerminalDock } from "./TerminalDock";

describe("clampDock", () => {
  it("keeps the width between the minimum and 70% of the window", () => {
    expect(clampDock(100, 1400)).toBe(DOCK_MIN);
    expect(clampDock(600, 1400)).toBe(600);
    expect(clampDock(5000, 1400)).toBe(980);
    expect(dockMax(1400)).toBe(980);
  });

  it("never lets a tiny window push the maximum below the minimum", () => {
    expect(clampDock(500, 300)).toBe(DOCK_MIN);
  });

  it("falls back to the default for nonsense", () => {
    expect(clampDock(Number.NaN, 1400)).toBe(DOCK_DEFAULT);
  });
});

describe("isDockShortcut", () => {
  it("is Ctrl or Cmd with the backtick and nothing else", () => {
    expect(isDockShortcut({ key: "`", ctrlKey: true })).toBe(true);
    expect(isDockShortcut({ key: "`", ctrlKey: false, metaKey: true })).toBe(true);
    expect(isDockShortcut({ key: "`", ctrlKey: false })).toBe(false);
    expect(isDockShortcut({ key: "`", ctrlKey: true, shiftKey: true })).toBe(false);
    expect(isDockShortcut({ key: "k", ctrlKey: true })).toBe(false);
  });
});

describe("useTerminalDock", () => {
  beforeEach(() => {
    localStorage.clear();
    window.innerWidth = 1400;
  });

  it("starts closed at the default width", () => {
    const { result } = renderHook(() => useTerminalDock());
    expect(result.current.open).toBe(false);
    expect(result.current.width).toBe(DOCK_DEFAULT);
  });

  it("remembers open and width", () => {
    const first = renderHook(() => useTerminalDock());
    act(() => first.result.current.setOpen(true));
    act(() => first.result.current.setWidth(5000));
    expect(localStorage.getItem(DOCK_OPEN_KEY)).toBe("1");
    expect(localStorage.getItem(DOCK_WIDTH_KEY)).toBe("980");
    first.unmount();

    const second = renderHook(() => useTerminalDock());
    expect(second.result.current.open).toBe(true);
    expect(second.result.current.width).toBe(980);
  });

  it("toggles with Ctrl+` even from inside a focused field", () => {
    const { result } = renderHook(() => useTerminalDock());
    const field = document.createElement("textarea");
    document.body.appendChild(field);
    act(() => {
      fireEvent.keyDown(field, { key: "`", ctrlKey: true });
    });
    expect(result.current.open).toBe(true);
    act(() => {
      fireEvent.keyDown(field, { key: "`", ctrlKey: true });
    });
    expect(result.current.open).toBe(false);
    field.remove();
  });
});

describe("TerminalDock", () => {
  beforeEach(() => {
    invoke.mockClear();
    window.innerWidth = 1400;
  });

  function dock(open: boolean, onClose = vi.fn()) {
    return <TerminalDock open={open} width={480} onWidthChange={vi.fn()} onClose={onClose} />;
  }

  it("spawns no shell until first opened", async () => {
    render(dock(false));
    expect(screen.queryByTestId("terminal")).toBeNull();
  });

  it("keeps the same session mounted when hidden and shown again", async () => {
    const view = render(dock(true));
    const first = (await screen.findByTestId("terminal")).getAttribute("data-id");
    view.rerender(dock(false));
    expect(screen.getByTestId("terminal").getAttribute("data-active")).toBe("false");
    view.rerender(dock(true));
    expect(screen.getByTestId("terminal").getAttribute("data-id")).toBe(first);
  });

  it("restarts with a new session id after the shell exits", async () => {
    render(dock(true));
    const first = (await screen.findByTestId("terminal")).getAttribute("data-id");
    act(() => exitDock?.());
    fireEvent.click(screen.getByRole("button", { name: "Restart" }));
    expect(screen.getByTestId("terminal").getAttribute("data-id")).not.toBe(first);
    expect(screen.queryByRole("button", { name: "Restart" })).toBeNull();
  });

  it("ends the session and hides, then starts fresh on the next open", async () => {
    let setOpen: (open: boolean) => void = () => undefined;
    function Harness() {
      const [open, set] = useState(true);
      setOpen = set;
      return <TerminalDock open={open} width={480} onWidthChange={vi.fn()} onClose={() => set(false)} />;
    }
    render(<Harness />);
    const first = (await screen.findByTestId("terminal")).getAttribute("data-id");
    fireEvent.click(screen.getByRole("button", { name: "End session" }));
    expect(screen.queryByTestId("terminal")).toBeNull();
    act(() => setOpen(true));
    const next = (await screen.findByTestId("terminal")).getAttribute("data-id");
    expect(next).not.toBe(first);
  });

  it("hides without ending when closed", async () => {
    const onClose = vi.fn();
    render(dock(true, onClose));
    await screen.findByTestId("terminal");
    fireEvent.click(screen.getByRole("button", { name: "Hide terminal" }));
    expect(onClose).toHaveBeenCalled();
    expect(screen.getByTestId("terminal")).toBeDefined();
  });
});
