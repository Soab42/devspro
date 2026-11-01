import { ReactNode } from "react";
import { createPortal } from "react-dom";
import type { LucideIcon } from "../icons";
import { Icon } from "../Icon";
import { Button } from "../Button";
import { Spinner } from "../Feedback";
import { EmptyState } from "../Layout";
import { Card } from "../Card";
import type { Resource } from "./useResource";
import { usePageChrome } from "./PageChrome";
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
      action?: { label: string; onClick: () => void; loading?: boolean };
      /** A quieter second way out — Recheck beside Start, say. */
      secondaryAction?: { label: string; onClick: () => void };
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
  /**
   * Bound the page to the window's height instead of letting it grow with its
   * content, so a region inside it can scroll on its own.
   *
   * `min-height: 100%` is deliberately not enough: it leaves the page's height
   * `auto`, and a percentage height on anything below then resolves to auto
   * too, which quietly unbounds the whole chain.
   */
  fill?: boolean;
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
 *
 * In a popped-out window the shell provides an actions slot in its top bar
 * (see PageChrome). The meta and actions portal there and the header is
 * reduced to a visually hidden <h1>, so the page starts at its body and the
 * controls sit where a window's controls belong.
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
  fill = false,
  children,
}: PageProps) {
  const slot = usePageChrome()?.actionsSlot ?? null;
  const showActions = Boolean(actions) && status.kind !== "blocked";

  if (slot) {
    return (
      <div className={`page page-${width}${fill ? " page-fill" : ""}`}>
        <h1 className="page-title u-sr-only">{title}</h1>
        {createPortal(
          <>
            {meta && <div className="page-header-meta">{meta}</div>}
            {showActions && (
              <div className="page-header-actions">{actions}</div>
            )}
          </>,
          slot,
        )}
        {tabs && status.kind !== "loading" && status.kind !== "blocked" && (
          <div className="page-tabs">{tabs}</div>
        )}
        <div className="page-body">
          <PageBody status={status}>{children}</PageBody>
        </div>
      </div>
    );
  }

  return (
    <div className={`page page-${width}${fill ? " page-fill" : ""}`}>
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
        {showActions && <div className="page-header-actions">{actions}</div>}
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

function PageBody({
  status,
  children,
}: {
  status: PageStatus;
  children: ReactNode;
}) {
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
              (status.action || status.secondaryAction) && (
                <div className="page-state-actions">
                  {status.action && (
                    <Button
                      variant="primary"
                      loading={status.action.loading}
                      onClick={status.action.onClick}
                    >
                      {status.action.label}
                    </Button>
                  )}
                  {status.secondaryAction && (
                    <Button variant="secondary" onClick={status.secondaryAction.onClick}>
                      {status.secondaryAction.label}
                    </Button>
                  )}
                </div>
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
