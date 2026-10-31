import "./dragShim.css";

/**
 * In-app drag and drop for Windows.
 *
 * The file views move files by HTML5 drag and drop (a row is `draggable`,
 * targets handle `dragover`/`drop`). On Windows that never works inside the
 * app: Tauri registers its own drop target on the WebView2 window so files
 * dragged in from the desktop arrive with their paths, and with it in place
 * WebView2 delivers no HTML5 drag events at all. Turning Tauri's off would
 * lose the desktop drops instead.
 *
 * So on Windows the native drag is not started, and this drives the same
 * events from the pointer: past a few pixels of movement it fires
 * `dragstart` on the row with a real `DataTransfer`, then `dragenter`,
 * `dragover` and `dragleave` on whatever is under the pointer, and `drop`
 * where the pointer is released if that target accepted the drag. Every
 * existing drag source and drop target works unchanged. Escape cancels.
 */

const THRESHOLD = 4;

interface Pending {
  source: HTMLElement;
  x: number;
  y: number;
  pointerId: number;
}

interface Active {
  source: HTMLElement;
  dt: DataTransfer;
  over: Element | null;
  accepted: boolean;
  ghost: HTMLElement;
}

function fire(target: EventTarget, type: string, dt: DataTransfer, x: number, y: number, related: EventTarget | null = null) {
  const event = new DragEvent(type, {
    bubbles: true,
    cancelable: true,
    composed: true,
    clientX: x,
    clientY: y,
    dataTransfer: dt,
    relatedTarget: related,
    button: 0,
    buttons: type === "drop" || type === "dragend" ? 0 : 1,
  });
  target.dispatchEvent(event);
  return event;
}

/** Fields, editors and anything editable keep their own pointer behaviour. */
function isEditable(el: Element): boolean {
  return Boolean(el.closest("input, textarea, select, [contenteditable=''], [contenteditable='true']"));
}

function label(dt: DataTransfer): string {
  const text = dt.getData("text/plain").trim();
  if (!text) return "Moving";
  const name = text.split(/[\\/]/).filter(Boolean).pop();
  return name ?? text;
}

export function installDragShim() {
  let pending: Pending | null = null;
  let active: Active | null = null;

  // Native drags would hand the pointer to the OS and never come back with
  // events; ours replace them. Capture on window runs before React's root.
  window.addEventListener(
    "dragstart",
    (e) => {
      if (!e.isTrusted) return;
      e.preventDefault();
      e.stopImmediatePropagation();
    },
    true,
  );

  window.addEventListener(
    "pointerdown",
    (e) => {
      if (e.button !== 0 || !(e.target instanceof Element) || isEditable(e.target)) return;
      const source = e.target.closest<HTMLElement>('[draggable="true"]');
      if (!source) return;
      pending = { source, x: e.clientX, y: e.clientY, pointerId: e.pointerId };
    },
    true,
  );

  const start = (e: PointerEvent) => {
    if (!pending) return;
    const dt = new DataTransfer();
    const event = fire(pending.source, "dragstart", dt, e.clientX, e.clientY);
    const source = pending.source;
    pending = null;
    if (event.defaultPrevented) return;
    window.getSelection()?.removeAllRanges();
    const ghost = document.createElement("div");
    ghost.className = "drag-shim-ghost";
    ghost.textContent = label(dt);
    document.body.appendChild(ghost);
    document.body.classList.add("drag-shim-active");
    active = { source, dt, over: null, accepted: false, ghost };
  };

  const finish = (x: number, y: number, drop: boolean) => {
    const a = active;
    if (!a) return;
    active = null;
    if (a.over) {
      if (drop && a.accepted) fire(a.over, "drop", a.dt, x, y);
      else fire(a.over, "dragleave", a.dt, x, y);
    }
    if (!(drop && a.accepted)) a.dt.dropEffect = "none";
    fire(a.source, "dragend", a.dt, x, y);
    a.ghost.remove();
    document.body.classList.remove("drag-shim-active", "drag-shim-blocked");
    // The release that ended a drag is not also a click on what it ended over.
    const swallow = (c: MouseEvent) => {
      c.preventDefault();
      c.stopPropagation();
    };
    window.addEventListener("click", swallow, { capture: true, once: true });
    window.setTimeout(() => window.removeEventListener("click", swallow, true), 0);
  };

  window.addEventListener(
    "pointermove",
    (e) => {
      if (pending && e.pointerId === pending.pointerId) {
        if (Math.hypot(e.clientX - pending.x, e.clientY - pending.y) < THRESHOLD) return;
        start(e);
      }
      const a = active;
      if (!a) return;
      e.preventDefault();
      a.ghost.style.transform = `translate(${e.clientX + 14}px, ${e.clientY + 10}px)`;
      const over = document.elementFromPoint(e.clientX, e.clientY);
      if (over !== a.over) {
        if (a.over) fire(a.over, "dragleave", a.dt, e.clientX, e.clientY, over);
        if (over) fire(over, "dragenter", a.dt, e.clientX, e.clientY, a.over);
        a.over = over;
      }
      // A target accepts by cancelling dragover, as with a real drag.
      a.accepted = over ? fire(over, "dragover", a.dt, e.clientX, e.clientY).defaultPrevented : false;
      document.body.classList.toggle("drag-shim-blocked", !a.accepted);
    },
    true,
  );

  window.addEventListener(
    "pointerup",
    (e) => {
      pending = null;
      if (active) finish(e.clientX, e.clientY, true);
    },
    true,
  );
  window.addEventListener(
    "pointercancel",
    (e) => {
      pending = null;
      if (active) finish(e.clientX, e.clientY, false);
    },
    true,
  );
  window.addEventListener(
    "keydown",
    (e) => {
      if (e.key !== "Escape" || !active) return;
      e.preventDefault();
      e.stopPropagation();
      finish(0, 0, false);
    },
    true,
  );
}
