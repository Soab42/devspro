import { useRef } from "react";
import type { LucideIcon } from "./icons";
import { Icon } from "./Icon";
import "./SegmentedControl.css";

export interface SegmentOption<V extends string> {
  value: V;
  /** Always the accessible name; visible unless `iconOnly`. */
  label: string;
  icon?: LucideIcon;
  /** Optional trailing count, e.g. "Running 4". */
  count?: number;
}

export interface SegmentedControlProps<V extends string> {
  options: SegmentOption<V>[];
  value: V;
  onChange: (value: V) => void;
  size?: "sm" | "md";
  iconOnly?: boolean;
  ariaLabel: string;
  className?: string;
}

/**
 * Mutually exclusive choice: theme, and every table filter row.
 *
 * Filters were previously pairs of buttons toggling their own className, and in
 * one case a <Tabs>. Tabs imply switching between different views; these all
 * narrow one view, so they are radios and are exposed as such.
 */
export function SegmentedControl<V extends string>({
  options,
  value,
  onChange,
  size = "sm",
  iconOnly = false,
  ariaLabel,
  className,
}: SegmentedControlProps<V>) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);

  function onKeyDown(e: React.KeyboardEvent, index: number) {
    const delta = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!delta) return;
    e.preventDefault();
    const next = (index + delta + options.length) % options.length;
    onChange(options[next].value);
    refs.current[next]?.focus();
  }

  const classes = [
    "segmented",
    `segmented-${size}`,
    iconOnly && "segmented-icon-only",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={classes} role="radiogroup" aria-label={ariaLabel}>
      {options.map((option, index) => {
        const active = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={active}
            aria-label={iconOnly ? option.label : undefined}
            title={iconOnly ? option.label : undefined}
            /* Roving tabindex: the group is one tab stop, arrows move within. */
            tabIndex={active ? 0 : -1}
            ref={(el) => {
              refs.current[index] = el;
            }}
            className={`segment${active ? " segment-active" : ""}`}
            onClick={() => onChange(option.value)}
            onKeyDown={(e) => onKeyDown(e, index)}
          >
            {option.icon && <Icon icon={option.icon} size="sm" />}
            {!iconOnly && <span>{option.label}</span>}
            {!iconOnly && option.count !== undefined && (
              <span className="segment-count">{option.count}</span>
            )}
          </button>
        );
      })}
    </div>
  );
}
