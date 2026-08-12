import { ButtonHTMLAttributes, forwardRef, ReactNode } from "react";
import type { LucideIcon } from "./icons";
import { Icon } from "./Icon";
import { Spinner } from "./Feedback";
import "./Button.css";

export type ButtonVariant =
  | "primary"
  | "secondary"
  | "ghost"
  | "danger"
  | "danger-solid"
  | "link";
export type ButtonSize = "sm" | "md" | "lg";

interface CommonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
}

export interface ButtonProps extends CommonProps {
  icon?: LucideIcon;
  trailingIcon?: LucideIcon;
  fullWidth?: boolean;
  children?: ReactNode;
}

/**
 * Default variant is "secondary", not "primary". That is deliberate: the old
 * global `button { background: var(--primary) }` made every button on screen
 * shout, which is why nearly every call site had to pass `className="secondary"`
 * to undo it. Emphasis should cost a prop; quiet should be free.
 *
 * Also defaults to type="button" — the HTML default of "submit" caused stray
 * form submissions from buttons that were never meant to submit.
 */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  {
    variant = "secondary",
    size = "md",
    icon,
    trailingIcon,
    loading = false,
    fullWidth = false,
    disabled,
    className,
    children,
    type,
    ...rest
  },
  ref,
) {
  const classes = [
    "btn",
    `btn-${variant}`,
    `btn-${size}`,
    fullWidth && "btn-full",
    loading && "btn-loading",
    !children && "btn-icon-only",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <button
      {...rest}
      ref={ref}
      type={type ?? "button"}
      className={classes}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
    >
      {/* The spinner occupies the leading icon slot rather than replacing the
          label, so the button keeps its width and the row doesn't reflow. */}
      {loading ? (
        <Spinner size={size === "lg" ? "md" : "sm"} />
      ) : (
        icon && <Icon icon={icon} size={size === "lg" ? "md" : "sm"} />
      )}
      {children && <span className="btn-label">{children}</span>}
      {trailingIcon && !loading && (
        <Icon icon={trailingIcon} size={size === "lg" ? "md" : "sm"} />
      )}
    </button>
  );
});

export interface IconButtonProps extends CommonProps {
  icon: LucideIcon;
  /** Required — becomes both the accessible name and the hover tooltip. */
  label: string;
  showTitle?: boolean;
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  {
    icon,
    label,
    showTitle = true,
    variant = "ghost",
    size = "md",
    loading = false,
    disabled,
    className,
    type,
    ...rest
  },
  ref,
) {
  const classes = ["btn", `btn-${variant}`, `btn-${size}`, "btn-icon-only", className]
    .filter(Boolean)
    .join(" ");

  return (
    <button
      {...rest}
      ref={ref}
      type={type ?? "button"}
      className={classes}
      disabled={disabled || loading}
      aria-label={label}
      title={showTitle ? label : undefined}
      aria-busy={loading || undefined}
    >
      {loading ? <Spinner size="sm" label={label} /> : <Icon icon={icon} size="sm" />}
    </button>
  );
});
