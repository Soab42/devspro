import { ReactNode } from "react";
import type { LucideIcon } from "../icons";
import { Card, CardPadding } from "../Card";
import { EmptyState } from "../Layout";

export interface SectionEmpty {
  icon?: LucideIcon;
  title: string;
  description?: ReactNode;
  action?: ReactNode;
}

export interface SectionProps {
  title?: ReactNode;
  description?: ReactNode;
  icon?: LucideIcon;
  /** Status pill or count beside the title. */
  meta?: ReactNode;
  actions?: ReactNode;
  /** Filter row between the header and the body. */
  toolbar?: ReactNode;
  footer?: ReactNode;
  padding?: CardPadding;
  span?: 1 | 2;
  /** When true, `empty` renders instead of `children`. */
  isEmpty?: boolean;
  empty?: SectionEmpty;
  /** Use 3 when nested under something that already owns an h2, e.g. a Modal. */
  headingLevel?: 2 | 3;
  className?: string;
  children?: ReactNode;
}

/**
 * A titled region within a Page. Owns the <h2> and the empty state, so pages
 * stop hand-rolling `<p className="empty-state">` — there were eight distinct
 * copies of that, each worded and styled slightly differently.
 */
export function Section({
  title,
  description,
  icon,
  meta,
  actions,
  toolbar,
  footer,
  padding = "md",
  span,
  isEmpty = false,
  empty,
  headingLevel = 2,
  className,
  children,
}: SectionProps) {
  const headerActions =
    meta || actions ? (
      <>
        {meta}
        {actions}
      </>
    ) : undefined;

  return (
    <Card
      title={title}
      description={description}
      icon={icon}
      actions={headerActions}
      toolbar={toolbar}
      footer={footer}
      /* An empty state brings its own padding and needs to centre in the full
         width, so drop the card padding when it is showing. */
      padding={isEmpty ? "none" : padding}
      span={span}
      headingLevel={headingLevel}
      className={className}
    >
      {isEmpty && empty ? (
        <EmptyState
          icon={empty.icon}
          title={empty.title}
          description={empty.description}
          action={empty.action}
        />
      ) : (
        children
      )}
    </Card>
  );
}
