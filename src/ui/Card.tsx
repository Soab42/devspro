import { ReactNode } from "react";
import type { LucideIcon } from "./icons";
import { Icon } from "./Icon";
import "./Card.css";

export type CardPadding = "none" | "sm" | "md" | "lg";

export interface CardProps {
  title?: ReactNode;
  description?: ReactNode;
  icon?: LucideIcon;
  /** Right side of the header — buttons, status pills. */
  actions?: ReactNode;
  /** Full-width row under the header, above the body (e.g. a <Toolbar>). */
  toolbar?: ReactNode;
  footer?: ReactNode;
  /** Use "none" when the body is a DataTable — the table owns its own padding. */
  padding?: CardPadding;
  span?: 1 | 2;
  headingLevel?: 2 | 3;
  as?: "section" | "div" | "article";
  className?: string;
  children?: ReactNode;
}

/**
 * Surface container. Note it carries no outer margin — spacing between cards is
 * the parent <Stack>'s job. See the note in Layout.tsx for why.
 */
export function Card({
  title,
  description,
  icon,
  actions,
  toolbar,
  footer,
  padding = "md",
  span,
  headingLevel = 2,
  as: Tag = "section",
  className,
  children,
}: CardProps) {
  const Heading = headingLevel === 3 ? "h3" : "h2";
  const hasHeader = Boolean(title || actions || icon);

  const classes = ["card", span === 2 && "card-span-2", className].filter(Boolean).join(" ");

  return (
    <Tag className={classes}>
      {hasHeader && (
        <div className="card-header">
          {icon && (
            <span className="card-header-icon">
              <Icon icon={icon} size="sm" />
            </span>
          )}
          <div className="card-header-text">
            {title && <Heading className="card-title">{title}</Heading>}
            {description && <p className="card-desc">{description}</p>}
          </div>
          {actions && <div className="card-actions">{actions}</div>}
        </div>
      )}
      {toolbar && <div className="card-toolbar">{toolbar}</div>}
      {children !== undefined && (
        <div className={`card-body card-pad-${padding}`}>{children}</div>
      )}
      {footer && <div className="card-footer">{footer}</div>}
    </Tag>
  );
}
