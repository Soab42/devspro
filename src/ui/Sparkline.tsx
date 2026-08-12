import "./Sparkline.css";

export interface SparklineProps {
  data: number[];
  height?: number;
  color?: string;
  max?: number;
  min?: number;
  ariaLabel?: string;
}

// A trend line with no axes/labels — the shape is the point. Domain is fixed
// (not auto-scaled) when the caller passes max/min, which matters for percent
// metrics: a CPU wobbling between 40-45% shouldn't look like a spike-to-spike
// chart just because autoscale zoomed into that narrow band.
const VIEWBOX_WIDTH = 100;

// Rendered width is controlled entirely by CSS (.sparkline { width: 100% }) —
// the viewBox is a fixed coordinate space, not a pixel size, so the line
// stretches to fill whatever the flex/grid parent gives it without the value
// text next to it having to fight the SVG for horizontal space.
export function Sparkline({
  data,
  height = 28,
  color = "var(--color-accent)",
  max,
  min,
  ariaLabel,
}: SparklineProps) {
  if (data.length < 2) {
    return <svg className="sparkline" height={height} aria-hidden="true" />;
  }

  const upperBound = max ?? Math.max(...data);
  const lowerBound = min ?? Math.min(...data);
  const range = upperBound - lowerBound || 1;
  const stepX = VIEWBOX_WIDTH / (data.length - 1);
  const points = data.map((v, i) => {
    const x = i * stepX;
    const y = height - ((v - lowerBound) / range) * height;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  });
  const linePoints = points.join(" ");
  const areaPoints = `0,${height} ${linePoints} ${VIEWBOX_WIDTH},${height}`;

  return (
    <svg
      className="sparkline"
      height={height}
      viewBox={`0 0 ${VIEWBOX_WIDTH} ${height}`}
      preserveAspectRatio="none"
      role={ariaLabel ? "img" : undefined}
      aria-label={ariaLabel}
      aria-hidden={ariaLabel ? undefined : true}
    >
      <polyline points={areaPoints} fill={color} fillOpacity={0.14} stroke="none" />
      <polyline
        points={linePoints}
        fill="none"
        stroke={color}
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
        /* preserveAspectRatio="none" stretches the x axis, which would also
           stretch the stroke; this keeps it an even 2px at any width. */
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}
