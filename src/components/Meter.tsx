interface MeterProps {
  label?: string;
  percent: number;
  valueLabel?: string;
  warningAt?: number;
  criticalAt?: number;
}

export function meterSeverity(percent: number, warningAt = 70, criticalAt = 90): "good" | "warning" | "critical" {
  if (percent >= criticalAt) return "critical";
  if (percent >= warningAt) return "warning";
  return "good";
}

// A horizontal usage meter — CPU/memory/disk/network all reduce to "how full is
// this out of 100", so one component covers them. Severity is carried by fill
// color alone (accent -> warning -> critical) per thresholds, never by the label.
export default function Meter({ label, percent, valueLabel, warningAt = 70, criticalAt = 90 }: MeterProps) {
  const clamped = Math.max(0, Math.min(100, percent));
  const severity = meterSeverity(clamped, warningAt, criticalAt);

  return (
    <div className="meter-row">
      {label && <span className="meter-row-label" title={label}>{label}</span>}
      <div className="meter-track" role="progressbar" aria-valuenow={Math.round(clamped)} aria-valuemin={0} aria-valuemax={100}>
        <div className={`meter-fill meter-fill-${severity}`} style={{ width: `${clamped}%` }} />
      </div>
      <span className="meter-row-value">{valueLabel ?? `${Math.round(clamped)}%`}</span>
    </div>
  );
}
