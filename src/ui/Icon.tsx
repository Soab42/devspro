import type { LucideIcon } from "./icons";
import "./Icon.css";

export type IconSize = "xs" | "sm" | "md" | "lg";

/**
 * Icon sizes are px, not rem, and are applied as SVG attributes rather than CSS.
 *
 * They deliberately do NOT ride the fluid root font-size. Stroke icons are drawn
 * on a 24px grid; scaling them by fractional amounts lands strokes off the pixel
 * grid and they render visibly soft. Text scales, iconography snaps.
 */
const SIZE_PX: Record<IconSize, number> = {
  xs: 12,
  sm: 14,
  md: 16,
  lg: 20,
};

export interface IconProps {
  icon: LucideIcon;
  size?: IconSize;
  /** Defaults to 1.75, or 2 at "xs" where a thinner stroke disappears. */
  strokeWidth?: number;
  className?: string;
  /** Provide only when the icon carries meaning on its own. Omit inside a
   *  labelled control — the icon is then decorative and gets aria-hidden. */
  "aria-label"?: string;
}

export function Icon({
  icon: Component,
  size = "sm",
  strokeWidth,
  className,
  "aria-label": ariaLabel,
}: IconProps) {
  const px = SIZE_PX[size];
  return (
    <Component
      width={px}
      height={px}
      strokeWidth={strokeWidth ?? (size === "xs" ? 2 : 1.75)}
      className={className ? `icon ${className}` : "icon"}
      aria-hidden={ariaLabel ? undefined : true}
      role={ariaLabel ? "img" : undefined}
      aria-label={ariaLabel}
      focusable="false"
    />
  );
}
