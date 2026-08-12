import { ReactNode } from "react";
import type { LucideIcon } from "../icons";
import { Icon } from "../Icon";
import { Button } from "../Button";
import { Spinner } from "../Feedback";
import { EmptyState } from "../Layout";
import { Card } from "../Card";
import type { Resource } from "./useResource";
import "./Page.css";

export type PageStatus =
  | { kind: "ready" }
  | { kind: "loading"; message?: string }
  | { kind: "error"; message: string; retry?: () => void }
  /** A precondition isn't met: Docker not installed, nginx missing, etc. */
  | {
      kind: "blocked";
      title: string;
      message: ReactNode;
      icon?: LucideIcon;
      action?: { label: string; onClick: () => void };
      tone?: "neutral" | "warning";
    };

export interface PageProps {
  title: string;
  description?: ReactNode;
  icon?: LucideIcon;
  status?: PageStatus;
  /** Page-scoped verbs: Refresh, + New. */
  actions?: ReactNode;
  /** Small context beside the title — a version badge, a count. */
  meta?: ReactNode;
  /** Rendered between the header and the body; stays mounted across states. */
  tabs?: ReactNode;
  width?: "full" | "narrow";
  children: ReactNode;
}

/**
 * The page shell. Every module page renders exactly one of these.
 *
 * The <h1> is emitted once, unconditionally, ABOVE the status branch. That is
 * the whole point: pages used to early-return a separate full shell for each of
 * loading / not-installed / ready, so Docker and MailCatcher each shipped three
 * <h1> elements and Dashboard two. Making the header structurally independent
 * of the state makes that class of bug impossible rather than merely fixed.
 *
 * `actions` deliberately render in every state except `blocked`, so a Refresh
 * button doesn't appear and disappear as data loads.
 */
export function Page({
  title,
  description,
  icon,
  status = { kind: "ready" },
  actions,
  meta,
  tabs,
  width = "full",
  children,
}: PageProps) {
  return (
    <div className={`page page-${width}`}>
      <header className="page-header">
        <div className="page-header-main">
          {icon && (
            <span className="page-header-icon">
              <Icon icon={icon} size="lg" />
            </span>
          )}
          <div className="page-header-text">
            <div className="page-header-title-row">
              <h1 className="page-title">{title}</h1>
              {meta && <div className="page-header-meta">{meta}</div>}
            </div>
            {description && <p className="page-description">{description}</p>}
          </div>
        </div>
        {actions && status.kind !== "blocked" && (
          <div className="page-header-actions">{actions}</div>
        )}
      </header>

      {tabs && status.kind !== "loading" && status.kind !== "blocked" && (
        <div className="page-tabs">{tabs}</div>
      )}

      <div className="page-body">
        <PageBody status={status}>{children}</PageBody>
      </div>
    </div>
  );
}

function PageBody({ status, children }: { status: PageStatus; children: ReactNode }) {
  switch (status.kind) {
    case "ready":
      return <>{children}</>;

    case "loading":
      return (
        <div className="page-state">
          <Spinner size="lg" />
          <p className="page-state-text">{status.message ?? "Loading…"}</p>
        </div>
      );

    case "error":
      return (
        <Card>
          <EmptyState
            size="md"
            title="Something went wrong"
            description={status.message}
            action={
              status.retry && (
                <Button variant="secondary" onClick={status.retry}>
                  Try again
                </Button>
              )
            }
          />
        </Card>
      );

    case "blocked":
      return (
        <Card>
          <EmptyState
            size="md"
            icon={status.icon}
            title={status.title}
            description={status.message}
            action={
              status.action && (
                <Button variant="primary" onClick={status.action.onClick}>
                  {status.action.label}
                </Button>
              )
            }
          />
        </Card>
      );
  }
}

/**
 * Adapts a Resource into a PageStatus, with an optional precondition that wins
 * over everything else.
 *
 * Note it only surfaces `error` while there is nothing on screen. Once data has
 * loaded, a failed refresh must not blank the page — show it inline with a
 * <Callout> instead, alongside the stale-but-useful data.
 */
export function statusOf<T>(
  resource: Resource<T>,
  blocked?: PageStatus | null,
): PageStatus {
  if (blocked) return blocked;
  if (resource.loading) {
    return resource.error
      ? { kind: "error", message: resource.error, retry: resource.refresh }
      : { kind: "loading" };
  }
  return { kind: "ready" };
}
