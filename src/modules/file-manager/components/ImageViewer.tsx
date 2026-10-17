import { KeyboardEvent, PointerEvent, ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button, IconButton } from "../../../ui";
import { Expand, Shrink, ZoomIn, ZoomOut } from "../../../ui/icons";

/** Fit to the tab, or a scale of the image's own pixels. */
type Zoom = "fit" | number;

const STEPS = [0.05, 0.1, 0.25, 0.5, 0.75, 1, 1.5, 2, 3, 4, 6, 8, 10, 12, 16];

/* A mouse notch is about 100 of deltaY; a trackpad sends many small ones.
   Each zoom step waits for this much scrolling, and for a short gap after
   the last step, so a flick of the trackpad is not ten steps at once. */
const WHEEL_STEP_DELTA = 50;
const WHEEL_GAP_MS = 60;

/** How long the floating zoom label stays up after the last change. */
const BADGE_MS = 1200;

const percent = (scale: number) => `${Math.round(scale * 100)}%`;

/** How far the pointer may move and still count as a click rather than a drag. */
const CLICK_SLOP = 4;

function step(current: number, dir: 1 | -1): number {
  if (dir > 0) return STEPS.find((s) => s > current + 1e-6) ?? STEPS[STEPS.length - 1];
  return [...STEPS].reverse().find((s) => s < current - 1e-6) ?? STEPS[0];
}

/** A point on the picture, as a fraction of its size, and where on screen it should stay. */
interface Anchor {
  fx: number;
  fy: number;
  x: number;
  y: number;
}

/**
 * A picture in a tab: fitted to the space by default, on a checkerboard so
 * transparency shows. Click to see it at actual size around the point
 * clicked, click again to fit. Zoomed, drag with the left button to move
 * around it; Ctrl+wheel zooms around the pointer, and + − 0 1 work when the
 * picture has focus. Every change of zoom flashes the new level in large
 * type over the picture, Picasa-style. The controls render into the tab bar
 * through `renderControls`.
 */
export default function ImageViewer({
  src,
  name,
  renderControls,
}: {
  src: string;
  name: string;
  renderControls: (controls: ReactNode) => ReactNode;
}) {
  const [zoom, setZoom] = useState<Zoom>("fit");
  const [natural, setNatural] = useState<{ w: number; h: number } | null>(null);
  const [failed, setFailed] = useState(false);
  const [panning, setPanning] = useState(false);
  // What "fit" works out to for this picture in this tab, kept current as the tab is resized.
  const [fitScale, setFitScale] = useState<number | null>(null);
  // The floating zoom label; `seq` restarts its fade on every change.
  const [badge, setBadge] = useState<{ text: string; seq: number } | null>(null);
  const img = useRef<HTMLImageElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const anchor = useRef<Anchor | null>(null);
  const drag = useRef<{ x: number; y: number; left: number; top: number; moved: boolean } | null>(null);

  /** Where the picture itself is drawn. Fitted, the element fills the stage and the picture is scaled down inside it. */
  function pictureRect(): DOMRect | null {
    const el = img.current;
    if (!el?.naturalWidth) return null;
    const box = el.getBoundingClientRect();
    if (zoom !== "fit") return box;
    const pad = parseFloat(getComputedStyle(el).paddingLeft) || 0;
    const scale = Math.min(1, (box.width - 2 * pad) / el.naturalWidth, (box.height - 2 * pad) / el.naturalHeight);
    const w = el.naturalWidth * scale;
    const h = el.naturalHeight * scale;
    return new DOMRect(box.left + (box.width - w) / 2, box.top + (box.height - h) / 2, w, h);
  }

  /* Measured when asked rather than on load: a tab opened in the background
     loads its picture while hidden, at a width of zero. */
  function currentScale(): number {
    if (zoom !== "fit") return zoom;
    const rect = pictureRect();
    return rect && natural?.w ? rect.width / natural.w : 1;
  }

  /** Changes the zoom keeping the point under (x, y) — the stage's middle by default — where it is. */
  function zoomTo(next: Zoom, at?: { x: number; y: number }) {
    const rect = pictureRect();
    const area = stage.current?.getBoundingClientRect();
    if (rect && area && next !== "fit") {
      const x = at?.x ?? area.left + area.width / 2;
      const y = at?.y ?? area.top + area.height / 2;
      const clamp = (v: number) => Math.min(1, Math.max(0, v));
      anchor.current = { fx: clamp((x - rect.left) / rect.width), fy: clamp((y - rect.top) / rect.height), x, y };
    }
    setZoom(next);
  }
  const zoomBy = (dir: 1 | -1, at?: { x: number; y: number }) => zoomTo(step(currentScale(), dir), at);

  // After the new size is laid out, scroll so the anchored point is back under the pointer.
  useLayoutEffect(() => {
    const a = anchor.current;
    anchor.current = null;
    const el = img.current;
    const box = stage.current;
    if (!a || !el || !box || zoom === "fit") return;
    const rect = el.getBoundingClientRect();
    box.scrollLeft += rect.left + a.fx * rect.width - a.x;
    box.scrollTop += rect.top + a.fy * rect.height - a.y;
  }, [zoom]);

  // The wheel zooms around the pointer, as in Picasa — with or without Ctrl.
  // Shift+wheel is left to scroll sideways. Native and not passive (React's
  // wheel listeners are), so the app's own Ctrl+wheel zoom stays out of it.
  const zoomByRef = useRef(zoomBy);
  zoomByRef.current = zoomBy;
  useEffect(() => {
    const el = stage.current;
    if (!el) return;
    let pending = 0;
    let last = 0;
    const onWheel = (e: WheelEvent) => {
      if (e.shiftKey || e.altKey || e.deltaY === 0) return;
      e.preventDefault();
      e.stopPropagation();
      // Line and page modes come from mice that report notches, not pixels.
      pending += e.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? e.deltaY : Math.sign(e.deltaY) * WHEEL_STEP_DELTA;
      const now = Date.now();
      if (Math.abs(pending) < WHEEL_STEP_DELTA || now - last < WHEEL_GAP_MS) return;
      const dir = pending < 0 ? 1 : -1;
      pending = 0;
      last = now;
      zoomByRef.current(dir, { x: e.clientX, y: e.clientY });
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  /* Press and drag moves a zoomed picture around; a press that does not move
     is a click, which toggles between fitted and actual size. */
  function onPointerDown(e: PointerEvent<HTMLDivElement>) {
    const box = stage.current;
    if (e.button !== 0 || failed || !box) return;
    e.preventDefault();
    box.focus({ preventScroll: true });
    drag.current = { x: e.clientX, y: e.clientY, left: box.scrollLeft, top: box.scrollTop, moved: false };
    box.setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent<HTMLDivElement>) {
    const d = drag.current;
    const box = stage.current;
    if (!d || !box) return;
    const dx = e.clientX - d.x;
    const dy = e.clientY - d.y;
    if (!d.moved && Math.abs(dx) + Math.abs(dy) < CLICK_SLOP) return;
    if (!d.moved) {
      d.moved = true;
      if (zoom !== "fit") setPanning(true);
    }
    if (zoom === "fit") return;
    box.scrollLeft = d.left - dx;
    box.scrollTop = d.top - dy;
  }

  function onPointerUp(e: PointerEvent<HTMLDivElement>) {
    const d = drag.current;
    drag.current = null;
    setPanning(false);
    if (!d || d.moved || e.type === "pointercancel") return;
    if (zoom === "fit") zoomTo(1, { x: e.clientX, y: e.clientY });
    else zoomTo("fit");
  }

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.ctrlKey || e.metaKey || e.altKey || failed) return;
    const act: Record<string, () => void> = {
      "+": () => zoomBy(1),
      "=": () => zoomBy(1),
      "-": () => zoomBy(-1),
      "0": () => zoomTo("fit"),
      "1": () => zoomTo(1),
    };
    const run = act[e.key];
    if (!run) return;
    e.preventDefault();
    run();
  }

  useEffect(() => {
    const box = stage.current;
    if (!box || !natural?.w) return;
    const measure = () => {
      const pad = parseFloat(getComputedStyle(box).paddingLeft) || 0;
      const w = box.clientWidth - 2 * pad;
      const h = box.clientHeight - 2 * pad;
      // A tab in the background has no size; keep the last real answer.
      if (w > 0 && h > 0) setFitScale(Math.min(1, w / natural.w, h / natural.h));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    return () => observer.disconnect();
  }, [natural]);

  const shownScale = zoom === "fit" ? fitScale : zoom;

  // Flash the level whenever it changes, not when the picture first appears.
  const firstZoom = useRef(true);
  useEffect(() => {
    if (firstZoom.current) {
      firstZoom.current = false;
      return;
    }
    if (shownScale === null) return;
    setBadge((b) => ({ text: zoom === "fit" ? `Fit · ${percent(shownScale)}` : percent(shownScale), seq: (b?.seq ?? 0) + 1 }));
    const t = window.setTimeout(() => setBadge(null), BADGE_MS);
    return () => window.clearTimeout(t);
    // Only a change of zoom flashes the label; a resize while fitted does not.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [zoom]);

  const controls = (
    <>
      {natural && (
        <span className="fm-viewer-notice u-secondary u-nowrap">
          {natural.w} × {natural.h} · {shownScale === null ? "fit" : `${percent(shownScale)}${zoom === "fit" ? " (fit)" : ""}`}
        </span>
      )}
      <IconButton icon={ZoomOut} label="Zoom out (−)" size="sm" disabled={failed} onClick={() => zoomBy(-1)} />
      <IconButton icon={ZoomIn} label="Zoom in (+)" size="sm" disabled={failed} onClick={() => zoomBy(1)} />
      <Button variant="secondary" size="sm" icon={zoom === "fit" ? Expand : Shrink} disabled={failed} onClick={() => zoomTo(zoom === "fit" ? 1 : "fit")}>
        {zoom === "fit" ? "Actual size" : "Fit"}
      </Button>
    </>
  );

  const stageClass = ["fm-image-stage", zoom === "fit" ? "fm-image-stage-fit" : "fm-image-stage-zoomed", panning && "fm-image-stage-panning"]
    .filter(Boolean)
    .join(" ");

  return (
    <>
      {renderControls(controls)}
      <div className="fm-image-frame">
        <div
          ref={stage}
          className={stageClass}
          data-own-wheel-zoom=""
          tabIndex={0}
          aria-label={`${name}. Click to ${zoom === "fit" ? "show at actual size" : "fit"}, scroll to zoom, drag to move around when zoomed.`}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
          onKeyDown={onKeyDown}
        >
          {failed ? (
            <p className="u-secondary">This picture could not be shown here. Open it with the default app instead.</p>
          ) : (
            <img
              ref={img}
              src={src}
              alt={name}
              className="fm-image"
              draggable={false}
              style={zoom === "fit" || !natural ? undefined : { width: natural.w * zoom, height: natural.h * zoom }}
              onLoad={(e) => {
                setNatural({ w: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight });
                setFailed(false);
              }}
              onError={() => setFailed(true)}
            />
          )}
        </div>
        {/* Outside the scroller, so it stays put over the middle of the picture. */}
        {badge && (
          <span key={badge.seq} className="fm-zoom-badge" aria-live="polite">
            {badge.text}
          </span>
        )}
      </div>
    </>
  );
}
