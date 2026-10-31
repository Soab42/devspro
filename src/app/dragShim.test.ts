import { beforeAll, describe, expect, it, vi } from "vitest";

// jsdom has neither, and WebView2 has both; enough of each to drive the shim.
class FakeDataTransfer {
  private data = new Map<string, string>();
  dropEffect = "none";
  effectAllowed = "all";
  get types() {
    return [...this.data.keys()];
  }
  setData(type: string, value: string) {
    this.data.set(type, value);
  }
  getData(type: string) {
    return this.data.get(type) ?? "";
  }
}
class FakeDragEvent extends MouseEvent {
  dataTransfer: FakeDataTransfer | null;
  constructor(type: string, init: MouseEventInit & { dataTransfer?: FakeDataTransfer }) {
    super(type, init);
    this.dataTransfer = init.dataTransfer ?? null;
  }
}

let under: Element | null = null;

beforeAll(async () => {
  vi.stubGlobal("DataTransfer", FakeDataTransfer);
  vi.stubGlobal("DragEvent", FakeDragEvent);
  document.elementFromPoint = () => under;
  const { installDragShim } = await import("./dragShim");
  installDragShim();
});

function pointer(type: string, target: EventTarget, x: number, y: number) {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0 });
  Object.defineProperty(event, "pointerId", { value: 1 });
  target.dispatchEvent(event);
}

function setup(accepts: boolean) {
  document.body.innerHTML = '<div id="row" draggable="true">a.txt</div><div id="pane"></div>';
  const row = document.getElementById("row") as HTMLElement;
  const pane = document.getElementById("pane") as HTMLElement;
  const seen: string[] = [];
  row.addEventListener("dragstart", (e) => {
    seen.push("dragstart");
    e.dataTransfer?.setData("text/plain", "/srv/a.txt");
  });
  row.addEventListener("dragend", () => seen.push("dragend"));
  pane.addEventListener("dragover", (e) => {
    if (accepts) e.preventDefault();
  });
  pane.addEventListener("drop", (e) => seen.push(`drop ${e.dataTransfer?.getData("text/plain")}`));
  return { row, pane, seen };
}

describe("the Windows drag shim", () => {
  it("turns a pointer drag into dragstart, drop and dragend on the right elements", () => {
    const { row, pane, seen } = setup(true);
    pointer("pointerdown", row, 10, 10);
    under = pane;
    pointer("pointermove", window, 40, 40);
    pointer("pointermove", window, 60, 60);
    pointer("pointerup", window, 60, 60);
    expect(seen).toEqual(["dragstart", "drop /srv/a.txt", "dragend"]);
    expect(document.querySelector(".drag-shim-ghost")).toBeNull();
  });

  it("drops nothing on a target that did not accept the drag", () => {
    const { row, pane, seen } = setup(false);
    pointer("pointerdown", row, 10, 10);
    under = pane;
    pointer("pointermove", window, 40, 40);
    pointer("pointerup", window, 40, 40);
    expect(seen).toEqual(["dragstart", "dragend"]);
  });

  it("leaves a click that never moved alone", () => {
    const { row, seen } = setup(true);
    pointer("pointerdown", row, 10, 10);
    pointer("pointerup", window, 11, 10);
    expect(seen).toEqual([]);
  });
});
