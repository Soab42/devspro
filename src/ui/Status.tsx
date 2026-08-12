import { ReactNode } from "react";
import {
  Circle,
  CircleAlert,
  CircleCheck,
  CircleX,
  Info,
  LoaderCircle,
  LucideIcon,
  TriangleAlert,
  X,
} from "./icons";
import { Icon } from "./Icon";
import { IconButton } from "./Button";
import "./Status.css";

/* ----------------------------------------------------------- StatusPill -- */

export type StatusVariant = "ok" | "bad" | "warn" | "pending" | "neutral" | "info";

const STATUS_ICON: Record<StatusVariant, LucideIcon> = {
  ok: CircleCheck,
  bad: CircleX,
  warn: CircleAlert,
  pending: LoaderCircle,
  neutral: Circle,
  info: Info,
};

export interface StatusPillProps {
  variant: StatusVariant;
  label: string;
  icon?: LucideIcon;
  showIcon?: boolean;
  size?: "sm" | "md";
}

/**
 * Semantic state of a thing: running, failed, expiring, pending.
 *
 * Not for neutral metadata — a version string or a count is a <Badge>. Using a
 * green pill as a plain name label (as the old Processes table did) tells the
 * reader "this is healthy" when it only meant "this is a name".
 */
export function StatusPill({
  variant,
  label,
  icon,
  showIcon = true,
  size = "sm",
}: StatusPillProps) {
  const glyph = icon ?? STATUS_ICON[variant];
  return (
    <span className={`status-pill status-pill-${variant} status-pill-${size}`}>
      {showIcon && (
        <Icon
          icon={glyph}
          size="xs"
          className={variant === "pending" ? "status-pill-spin" : undefined}
        />
      )}
      {label}
    </span>
  );
}

/* ---------------------------------------------------------------- Badge -- */

export type BadgeTone = "neutral" | "accent" | "success" | "warning" | "danger" | "info";

export interface BadgeProps {
  children: ReactNode;
  tone?: BadgeTone;
  /** For versions, image tags, and other technical values. */
  mono?: boolean;
  icon?: LucideIcon;
  size?: "sm" | "md";
  className?: string;
}

/** Neutral metadata. Rectangular, so it never reads as a status at a glance. */
export function Badge({
  children,
  tone = "neutral",
  mono = false,
  icon,
  size = "sm",
  className,
}: BadgeProps) {
  const classes = [
    "badge",
    `badge-${tone}`,
    `badge-${size}`,
    mono && "u-mono",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <span className={classes}>
      {icon && <Icon icon={icon} size="xs" />}
      {children}
    </span>
  );
}

/* -------------------------------------------------------------- Callout -- */

export type CalloutTone = "info" | "success" | "warning" | "danger";

const CALLOUT_ICON: Record<CalloutTone, LucideIcon> = {
  info: Info,
  success: CircleCheck,
  warning: TriangleAlert,
  danger: CircleX,
};

export interface CalloutProps {
  tone: CalloutTone;
  title?: string;
  children?: ReactNode;
  icon?: LucideIcon;
  onDismiss?: () => void;
  actions?: ReactNode;
  className?: string;
}

/**
 * Inline message inside a page body.
 *
 * This is where non-fatal errors belong — a failed row action must not be
 * routed through Page's `status`, or one failed button blanks the whole table.
 * It is also how success and failure stop looking identical: several pages
 * previously rendered both as the same grey muted paragraph.
 */
export function Callout({
  tone,
  title,
  children,
  icon,
  onDismiss,
  actions,
  className,
}: CalloutProps) {
  return (
    <div
      className={`callout callout-${tone}${className ? ` ${className}` : ""}`}
      role={tone === "danger" ? "alert" : "status"}
    >
      <Icon icon={icon ?? CALLOUT_ICON[tone]} size="sm" className="callout-icon" />
      <div className="callout-content">
        {title && <p className="callout-title">{title}</p>}
        {children && <div className="callout-body">{children}</div>}
        {actions && <div className="callout-actions">{actions}</div>}
      </div>
      {onDismiss && (
        <IconButton
          icon={X}
          label="Dismiss"
          size="sm"
          variant="ghost"
          onClick={onDismiss}
          className="callout-dismiss"
        />
      )}
    </div>
  );
}
