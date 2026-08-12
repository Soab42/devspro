import { LoaderCircle } from "./icons";
import { Icon, IconSize } from "./Icon";
import "./Feedback.css";

/* -------------------------------------------------------------- Spinner -- */

export interface SpinnerProps {
  size?: IconSize;
  /** Announced to screen readers. Defaults to "Loading". */
  label?: string;
  className?: string;
}

export function Spinner({ size = "sm", label = "Loading", className }: SpinnerProps) {
  return (
    <span className={className ? `spinner ${className}` : "spinner"} role="status">
      <Icon icon={LoaderCircle} size={size} className="spinner-icon" />
      <span className="u-sr-only">{label}</span>
    </span>
  );
}

/* ------------------------------------------------------------- Skeleton -- */

export interface SkeletonProps {
  width?: number | string;
  height?: number | string;
  radius?: "sm" | "md" | "full";
  className?: string;
}

export function Skeleton({ width, height, radius = "sm", className }: SkeletonProps) {
  return (
    <span
      className={`skeleton skeleton-${radius}${className ? ` ${className}` : ""}`}
      style={{ width, height }}
      aria-hidden="true"
    />
  );
}
