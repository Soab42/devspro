import { PointerEvent, ReactNode, useRef, useState } from "react";
import { Button, Icon, IconButton, TextInput } from "../../../ui";
import { Plus, RefreshCw, Smartphone, Trash2, X } from "../../../ui/icons";
import { CustomDevice, Device, Viewport, clampViewport } from "../utils";

// ---------------------------------------------------------------- device bar

interface DeviceBarProps {
  device: Device;
  viewport: Viewport;
  onViewport: (v: Viewport) => void;
  onSavePreset: (label: string) => void;
  onDeletePreset: (id: string) => void;
  onFullSize: () => void;
  customDevices: CustomDevice[];
}

/**
 * Sits above the page while a size is pinned: type a width and height, swap
 * them to rotate, keep the current size as a named preset, or drop back to
 * full size. Dragging the frame's handles feeds the same fields.
 */
export function DeviceBar({
  device,
  viewport,
  onViewport,
  onSavePreset,
  onDeletePreset,
  onFullSize,
  customDevices,
}: DeviceBarProps) {
  const [saving, setSaving] = useState(false);
  const [name, setName] = useState("");
  const isCustom = customDevices.some((c) => c.id === device.id);

  function commit(field: "width" | "height", raw: string) {
    const n = Number(raw);
    if (!Number.isFinite(n)) return;
    onViewport(clampViewport({ ...viewport, [field]: n }));
  }

  function save() {
    const label = name.trim();
    if (!label) return;
    onSavePreset(label);
    setSaving(false);
    setName("");
  }

  return (
    <div
      className="browser-bar browser-device"
      role="toolbar"
      aria-label="Viewport size"
    >
      <Icon icon={Smartphone} size="sm" />
      <span className="browser-device-name">{device.label}</span>
      <label className="browser-device-field">
        <span className="u-sr-only">Width</span>
        <input
          className="browser-device-input u-tnum"
          type="number"
          min={200}
          step={1}
          value={viewport.width}
          onChange={(e) => commit("width", e.target.value)}
          aria-label="Viewport width"
        />
      </label>
      <span className="browser-device-times" aria-hidden="true">
        ×
      </span>
      <label className="browser-device-field">
        <span className="u-sr-only">Height</span>
        <input
          className="browser-device-input u-tnum"
          type="number"
          min={200}
          step={1}
          value={viewport.height}
          onChange={(e) => commit("height", e.target.value)}
          aria-label="Viewport height"
        />
      </label>
      <span className="browser-device-unit">px</span>
      <IconButton
        icon={RefreshCw}
        label="Rotate"
        size="sm"
        onClick={() =>
          onViewport({ width: viewport.height, height: viewport.width })
        }
      />
      {saving ? (
        <form
          className="browser-device-save"
          onSubmit={(e) => {
            e.preventDefault();
            save();
          }}
        >
          <TextInput
            inputSize="sm"
            autoFocus
            placeholder="Preset name"
            aria-label="Preset name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setSaving(false)}
          />
          <Button
            size="sm"
            variant="primary"
            type="submit"
            disabled={!name.trim()}
          >
            Save
          </Button>
          <Button size="sm" type="button" onClick={() => setSaving(false)}>
            Cancel
          </Button>
        </form>
      ) : (
        <Button size="sm" icon={Plus} onClick={() => setSaving(true)}>
          Save as preset
        </Button>
      )}
      {isCustom && (
        <IconButton
          icon={Trash2}
          label={`Delete preset ${device.label}`}
          size="sm"
          variant="danger"
          onClick={() => onDeletePreset(device.id)}
        />
      )}
      <div className="browser-bar-actions">
        <Button size="sm" icon={X} onClick={onFullSize}>
          Full size
        </Button>
      </div>
    </div>
  );
}

// ------------------------------------------------------------ resizable frame

interface ViewportFrameProps {
  viewport: Viewport;
  /** The pane's inner size; a drag can't make the page larger than its pane. */
  maxSize: () => { width: number; height: number };
  onResizeStart: () => void;
  onResize: (v: Viewport) => void;
  onResizeEnd: () => void;
  children: ReactNode;
}

type Edge = "e" | "s" | "se";

/**
 * Handles on the right, bottom and corner of the pinned viewport. They sit
 * just outside the page's rectangle because the native view paints over
 * anything inside it. While dragging, the parent hides the native view and
 * the box shows its live size instead — pointer events don't reach the DOM
 * through the native view, so this is the one way a drag stays smooth.
 */
export function ViewportFrame({
  viewport,
  maxSize,
  onResizeStart,
  onResize,
  onResizeEnd,
  children,
}: ViewportFrameProps) {
  const drag = useRef<{
    edge: Edge;
    x: number;
    y: number;
    start: Viewport;
    max: { width: number; height: number };
  } | null>(null);

  function down(edge: Edge) {
    return (e: PointerEvent<HTMLDivElement>) => {
      if (e.button !== 0) return;
      e.preventDefault();
      e.currentTarget.setPointerCapture(e.pointerId);
      drag.current = {
        edge,
        x: e.clientX,
        y: e.clientY,
        start: viewport,
        max: maxSize(),
      };
      onResizeStart();
    };
  }

  function move(e: PointerEvent<HTMLDivElement>) {
    const d = drag.current;
    if (!d) return;
    const dx = e.clientX - d.x;
    const dy = e.clientY - d.y;
    const width =
      d.edge === "s"
        ? d.start.width
        : Math.min(d.max.width, d.start.width + dx);
    const height =
      d.edge === "e"
        ? d.start.height
        : Math.min(d.max.height, d.start.height + dy);
    onResize(clampViewport({ width, height }));
  }

  function up(e: PointerEvent<HTMLDivElement>) {
    if (!drag.current) return;
    drag.current = null;
    try {
      e.currentTarget.releasePointerCapture(e.pointerId);
    } catch {
      /* already released */
    }
    onResizeEnd();
  }

  const handle = (edge: Edge, label: string) => (
    <div
      className={`browser-handle browser-handle-${edge}`}
      role="separator"
      aria-label={label}
      aria-orientation={edge === "s" ? "horizontal" : "vertical"}
      onPointerDown={down(edge)}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
    />
  );

  return (
    <div className="browser-frame">
      {children}
      {handle("e", "Resize width")}
      {handle("s", "Resize height")}
      {handle("se", "Resize")}
    </div>
  );
}
