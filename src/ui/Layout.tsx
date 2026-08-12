import { CSSProperties, ReactNode } from "react";
import type { LucideIcon } from "./icons";
import { Inbox } from "./icons";
import { Icon } from "./Icon";
import "./Layout.css";

/* ---------------------------------------------------------------- Stack -- */

export type StackGap = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8;

export interface StackProps {
  children: ReactNode;
  gap?: StackGap;
  direction?: "row" | "column";
  wrap?: boolean;
  align?: CSSProperties["alignItems"];
  justify?: CSSProperties["justifyContent"];
  className?: string;
}

/**
 * Owns vertical rhythm so components never do.
 *
 * The old layout gave every card a `margin-bottom`, which is why a card in an
 * unusual position always needed an inline `style={{ marginTop: 0 }}` to cancel
 * it — that exact override appeared nine times. Spacing is a property of the
 * arrangement, not of the thing being arranged.
 */
export function Stack({
  children,
  gap = 5,
  direction = "column",
  wrap = false,
  align,
  justify,
  className,
}: StackProps) {
  const classes = [
    "stack",
    `stack-${direction}`,
    `stack-gap-${gap}`,
    wrap && "stack-wrap",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={classes} style={{ alignItems: align, justifyContent: justify }}>
      {children}
    </div>
  );
}

/* ----------------------------------------------------------- EmptyState -- */

export interface EmptyStateProps {
  icon?: LucideIcon;
  title: string;
  description?: ReactNode;
  action?: ReactNode;
  /** "sm" sits inside a card or table body; "md" fills a page region. */
  size?: "sm" | "md";
}

export function EmptyState({
  icon = Inbox,
  title,
  description,
  action,
  size = "sm",
}: EmptyStateProps) {
  return (
    <div className={`empty-state empty-state-${size}`}>
      <span className="empty-state-icon">
        <Icon icon={icon} size={size === "md" ? "lg" : "md"} />
      </span>
      <p className="empty-state-title">{title}</p>
      {description && <div className="empty-state-desc">{description}</div>}
      {action && <div className="empty-state-action">{action}</div>}
    </div>
  );
}

/* -------------------------------------------------------------- Toolbar -- */

export interface ToolbarProps {
  /** Left cluster: search, filters. */
  children: ReactNode;
  /** Right cluster, pushed away by an auto margin. */
  end?: ReactNode;
  sticky?: boolean;
  className?: string;
}

/**
 * The filter/search bar above a table. Replaces the `.form-row` that pages were
 * repurposing as a toolbar — the source of the repeated margin overrides, since
 * `.form-row` carried a top margin meant for stacked form fields.
 */
export function Toolbar({ children, end, sticky = false, className }: ToolbarProps) {
  const classes = ["toolbar", sticky && "toolbar-sticky", className].filter(Boolean).join(" ");
  return (
    <div className={classes}>
      <div className="toolbar-start">{children}</div>
      {end && <div className="toolbar-end">{end}</div>}
    </div>
  );
}
