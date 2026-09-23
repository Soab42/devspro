import { MouseEvent, ReactNode, useEffect, useId, useRef } from "react";
import { X } from "./icons";
import type { LucideIcon } from "./icons";
import { Icon } from "./Icon";
import { IconButton } from "./Button";
import "./Modal.css";

export type ModalSize = "sm" | "md" | "lg" | "full";

/**
 * Where the dialog sits. "drawer" pins it to the trailing edge, full height —
 * for editing one record beside the list it came from, where a centred dialog
 * would cover the thing you are working against.
 *
 * A variant of Modal rather than its own component: the overlay, the focus
 * trap, Escape and the click-outside rule are identical, and a second copy of
 * those is a second place for them to drift.
 */
export type ModalPlacement = "center" | "drawer";

export interface ModalProps {
  title: string;
  description?: ReactNode;
  icon?: LucideIcon;
  size?: ModalSize;
  placement?: ModalPlacement;
  /** Tints the header icon. Does not change the footer buttons. */
  tone?: "default" | "danger";
  onClose: () => void;
  /** Pinned below the scrolling body. Use <ModalFooter> for the standard order. */
  footer?: ReactNode;
  /** A drag handle in the corner lets the user size the dialog — for an
      editor, where the right width is the file's longest line. */
  resizable?: boolean;
  children: ReactNode;
}

export function Modal({
  title,
  description,
  icon,
  size = "md",
  placement = "center",
  tone = "default",
  onClose,
  footer,
  resizable = false,
  children,
}: ModalProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        onClose();
        return;
      }

      // Focus trap. Without it, Tab walks out of the dialog and into the page
      // behind it, which is both an a11y failure and a way to click buttons you
      // can't see.
      if (e.key !== "Tab" || !panelRef.current) return;
      const focusable = panelRef.current.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      );
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }

    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onClose]);

  // Move focus into the dialog on open so keyboard users don't start out
  // stranded at the top of the document behind the overlay.
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    panelRef.current?.focus();
    return () => previous?.focus?.();
  }, []);

  // mousedown, not click: a click that STARTS inside the panel and ends on the
  // overlay (a drag while selecting text) would otherwise close the dialog and
  // discard the form.
  function handleOverlayMouseDown(e: MouseEvent<HTMLDivElement>) {
    if (e.target === e.currentTarget) onClose();
  }

  return (
    <div
      className={`modal-overlay modal-overlay-${placement}`}
      onMouseDown={handleOverlayMouseDown}
    >
      <div
        ref={panelRef}
        className={`modal-panel modal-${size} modal-placement-${placement}${resizable ? " modal-resizable" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <div className="modal-header">
          {icon && (
            <span className={`modal-header-icon modal-header-icon-${tone}`}>
              <Icon icon={icon} size="sm" />
            </span>
          )}
          <div className="modal-header-text">
            <h2 id={titleId} className="modal-title">
              {title}
            </h2>
            {description && <p className="modal-description">{description}</p>}
          </div>
          <IconButton
            icon={X}
            label="Close"
            size="sm"
            variant="ghost"
            onClick={onClose}
          />
        </div>

        <div className="modal-body">{children}</div>

        {footer && <div className="modal-footer-wrap">{footer}</div>}
      </div>
    </div>
  );
}

export interface ModalFooterProps {
  /** Destructive or tertiary action — sits far left, away from the primary. */
  start?: ReactNode;
  /** Muted context line, e.g. "waiting for the admin password prompt". */
  hint?: ReactNode;
  /** Cancel then primary, in that order. */
  children: ReactNode;
}

/**
 * Standard footer ordering, used by every dialog:
 *
 *   [ destructive ]  ————  [ hint ]  [ Cancel ] [ Primary ]
 *
 * Destructive actions are deliberately separated from the primary so a misaimed
 * click lands on nothing rather than on Delete.
 */
export function ModalFooter({ start, hint, children }: ModalFooterProps) {
  return (
    <div className="modal-footer">
      {start && <div className="modal-footer-start">{start}</div>}
      {hint && <p className="modal-footer-hint">{hint}</p>}
      <div className="modal-footer-actions">{children}</div>
    </div>
  );
}
