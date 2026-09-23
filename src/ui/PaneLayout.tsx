import {
  PointerEvent as ReactPointerEvent,
  ReactNode,
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { dragSizes, leafIds, type PaneNode, type PaneSplit } from "./paneTree";
import "./PaneLayout.css";

export interface PaneLayoutProps {
  root: PaneNode;
  focusedId: string | null;
  onFocus: (leafId: string) => void;
  /** A divider was dragged: the split's new child fractions. */
  onSizes: (splitId: string, sizes: number[]) => void;
  /** The content of one pane. Rendered once per leaf and kept mounted across tree changes. */
  renderPane: (leafId: string) => ReactNode;
  /** Keeps every pane mounted while another layout is shown in its place. */
  hidden?: boolean;
  ariaLabel?: string;
}

interface Rect {
  top: number;
  left: number;
  width: number;
  height: number;
}

/**
 * Renders a split tree with draggable dividers.
 *
 * The tree is drawn twice: once as an ordinary flex layout of empty *slots*
 * (this is what the dividers resize), and once as a flat list of the panes'
 * contents, each absolutely positioned over its slot from a measurement.
 * The indirection is the point. A terminal is a live process behind an
 * xterm canvas; if its element were a child of the slot, splitting the pane
 * would move it to a new parent and React would unmount it — killing the
 * shell — on the way. Rendered flat, keyed by leaf id, it stays put while
 * the slots rearrange around it.
 */
export function PaneLayout({ root, focusedId, onFocus, onSizes, renderPane, hidden = false, ariaLabel }: PaneLayoutProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [rects, setRects] = useState<Record<string, Rect>>({});

  const measure = useCallback(() => {
    const host = hostRef.current;
    if (!host || host.offsetWidth === 0 || host.offsetHeight === 0) return;
    const origin = host.getBoundingClientRect();
    const next: Record<string, Rect> = {};
    host.querySelectorAll<HTMLElement>("[data-pane-slot]").forEach((slot) => {
      const r = slot.getBoundingClientRect();
      next[slot.dataset.paneSlot as string] = {
        top: r.top - origin.top,
        left: r.left - origin.left,
        width: r.width,
        height: r.height,
      };
    });
    setRects((prev) => (sameRects(prev, next) ? prev : next));
  }, []);

  // After every render the slots may have moved (a split, a drag, a pane
  // closing), and the observer covers the container itself changing size.
  useLayoutEffect(measure);
  useLayoutEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const observer = new ResizeObserver(measure);
    observer.observe(host);
    return () => observer.disconnect();
  }, [measure]);

  return (
    <div ref={hostRef} className={`pane-layout${hidden ? " pane-layout-hidden" : ""}`} role="group" aria-label={ariaLabel}>
      <div className="pane-slots">
        <SlotTree node={root} onSizes={onSizes} />
      </div>
      {leafIds(root).map((id) => {
        const rect = rects[id];
        const focused = id === focusedId;
        return (
          <div
            key={id}
            className={`pane-float${focused ? " pane-float-focused" : ""}${rect ? "" : " pane-float-unplaced"}`}
            style={rect ? { top: rect.top, left: rect.left, width: rect.width, height: rect.height } : undefined}
            onPointerDownCapture={() => {
              if (!focused) onFocus(id);
            }}
          >
            {renderPane(id)}
          </div>
        );
      })}
    </div>
  );
}

function sameRects(a: Record<string, Rect>, b: Record<string, Rect>): boolean {
  const keys = Object.keys(a);
  if (keys.length !== Object.keys(b).length) return false;
  return keys.every((k) => {
    const x = a[k];
    const y = b[k];
    return y && x.top === y.top && x.left === y.left && x.width === y.width && x.height === y.height;
  });
}

function SlotTree({ node, onSizes }: { node: PaneNode; onSizes: PaneLayoutProps["onSizes"] }) {
  if (node.kind === "leaf") return <div className="pane-slot" data-pane-slot={node.id} />;
  return <SplitSlots split={node} onSizes={onSizes} />;
}

function SplitSlots({ split, onSizes }: { split: PaneSplit; onSizes: PaneLayoutProps["onSizes"] }) {
  const ref = useRef<HTMLDivElement>(null);
  const horizontal = split.direction === "row";

  function startDrag(e: ReactPointerEvent<HTMLDivElement>, index: number) {
    const host = ref.current;
    if (!host || e.button !== 0) return;
    e.preventDefault();
    const extent = horizontal ? host.offsetWidth : host.offsetHeight;
    if (extent === 0) return;
    const origin = horizontal ? e.clientX : e.clientY;
    const startSizes = split.sizes;
    const divider = e.currentTarget;
    divider.setPointerCapture(e.pointerId);
    divider.classList.add("pane-divider-dragging");

    const move = (ev: PointerEvent) => {
      const position = horizontal ? ev.clientX : ev.clientY;
      onSizes(split.id, dragSizes(startSizes, index, (position - origin) / extent));
    };
    const stop = () => {
      divider.classList.remove("pane-divider-dragging");
      divider.removeEventListener("pointermove", move);
      divider.removeEventListener("pointerup", stop);
      divider.removeEventListener("pointercancel", stop);
    };
    divider.addEventListener("pointermove", move);
    divider.addEventListener("pointerup", stop);
    divider.addEventListener("pointercancel", stop);
  }

  return (
    <div ref={ref} className={`pane-split pane-split-${split.direction}`}>
      {split.children.map((child, i) => (
        <div key={child.kind === "leaf" ? child.id : child.id} className="pane-split-cell" style={{ flexGrow: split.sizes[i] }}>
          <SlotTree node={child} onSizes={onSizes} />
          {i < split.children.length - 1 && (
            <div
              className="pane-divider"
              role="separator"
              aria-orientation={horizontal ? "vertical" : "horizontal"}
              onPointerDown={(e) => startDrag(e, i)}
              onDoubleClick={() => onSizes(split.id, split.sizes.map(() => 1 / split.sizes.length))}
            />
          )}
        </div>
      ))}
    </div>
  );
}
