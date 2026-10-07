import { Button, Select } from "../ui";
import { DEFAULT_ZOOM, ZOOM_LEVELS, useZoom } from "./zoom";

const OPTIONS = ZOOM_LEVELS.map((l) => ({ value: String(l), label: `${l}%` }));

/** Interface size: the current percentage and a way back to 100%. */
export function ZoomControl() {
  const { zoom, setZoom, reset } = useZoom();
  return (
    <>
      <Select
        aria-label="Interface size"
        options={OPTIONS}
        value={String(zoom)}
        onChange={(v) => setZoom(Number(v))}
      />
      <Button variant="secondary" size="sm" disabled={zoom === DEFAULT_ZOOM} onClick={reset}>
        Reset
      </Button>
    </>
  );
}
