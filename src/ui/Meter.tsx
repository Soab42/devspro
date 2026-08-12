import "./Meter.css";

export type MeterSeverity = "good" | "warning" | "critical";

export interface MeterProps {
  label?: string;
  percent: number;
  valueLabel?: string;
  warningAt?: number;
  criticalAt?: number;
  /** "compact" drops the inline label row — for dense per-core grids. */
  variant?: "row" | "compact";
}

export function meterSeverity(
  percent: number,
  warningAt = 70,
  criticalAt = 90,
): MeterSeverity {
  if (percent >= criticalAt) return "critical";
  if (percent >= warningAt) return "warning";
  return "good";
}

// A horizontal usage meter — CPU/memory/disk/network all reduce to "how full is
// this out of 100", so one component covers them. Severity is carried by fill
// color alone (accent -> warning -> critical) per thresholds, never by the label.
export function Meter({
  label,
  percent,
  valueLabel,
  warningAt = 70,
  criticalAt = 90,
  variant = "row",
}: MeterProps) {
  const clamped = Math.max(0, Math.min(100, percent));
  const severity = meterSeverity(clamped, warningAt, criticalAt);

  if (variant === "compact") {
    return (
      <div className="meter-compact">
        {(label || valueLabel) && (
          <div className="meter-compact-label">
            <span className="u-truncate">{label}</span>
            <span className="u-tnum">{valueLabel ?? `${Math.round(clamped)}%`}</span>
          </div>
        )}
        <Track clamped={clamped} severity={severity} />
      </div>
    );
  }

  return (
    <div className="meter-row">
      {label && (
        <span className="meter-row-label" title={label}>
          {label}
        </span>
      )}
      <Track clamped={clamped} severity={severity} />
      <span className="meter-row-value">
        {valueLabel ?? `${Math.round(clamped)}%`}
      </span>
    </div>
  );
}

function Track({ clamped, severity }: { clamped: number; severity: MeterSeverity }) {
  return (
    <div
      className="meter-track"
      role="progressbar"
      aria-valuenow={Math.round(clamped)}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      {/* One of the few legitimate inline styles left: the width IS the data. */}
      <div className={`meter-fill meter-fill-${severity}`} style={{ width: `${clamped}%` }} />
    </div>
  );
}
