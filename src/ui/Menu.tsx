import { ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Ellipsis } from "./icons";
import type { LucideIcon } from "./icons";
import { Icon } from "./Icon";
import { IconButton } from "./Button";
import "./Menu.css";

export interface MenuItem {
  key: string;
  label: string;
  icon?: LucideIcon;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
  /** Draws a rule above this item — use to separate destructive actions. */
  separatorBefore?: boolean;
}

export interface MenuProps {
  items: MenuItem[];
  label?: string;
  icon?: LucideIcon;
  size?: "sm" | "md";
  align?: "start" | "end";
}

/**
 * Overflow menu for row actions.
 *
 * Several tables carried up to six inline buttons per row, which made the
 * action column wider than the data. Two or three primary verbs stay inline;
 * the rest live here.
 *
 * Rendered in a portal because the menu must escape the table's
 * `overflow: auto` scroll container — anchored inside it, the popup would be
 * clipped at the table edge.
 */
export function Menu({
  items,
  label = "More actions",
  icon = Ellipsis,
  size = "sm",
  align = "end",
}: MenuProps) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!open || !triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    const width = menuRef.current?.offsetWidth ?? 180;
    const height = menuRef.current?.offsetHeight ?? 0;

    // Flip up / clamp horizontally so the menu never opens off-screen for rows
    // near the bottom or the right edge of the window.
    const spaceBelow = window.innerHeight - rect.bottom;
    const top = spaceBelow < height + 8 ? rect.top - height - 4 : rect.bottom + 4;
    const rawLeft = align === "end" ? rect.right - width : rect.left;
    const left = Math.max(8, Math.min(rawLeft, window.innerWidth - width - 8));

    setPosition({ top, left });
  }, [open, align]);

  useEffect(() => {
    if (!open) return;

    function onPointerDown(e: MouseEvent) {
      const target = e.target as Node;
      if (menuRef.current?.contains(target) || triggerRef.current?.contains(target)) return;
      setOpen(false);
    }
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    // The portal is anchored to viewport coordinates, so it would visibly
    // detach from its row if either of these happened while open.
    function onReflow() {
      setOpen(false);
    }

    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    window.addEventListener("resize", onReflow);
    window.addEventListener("scroll", onReflow, true);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("resize", onReflow);
      window.removeEventListener("scroll", onReflow, true);
    };
  }, [open]);

  return (
    <>
      <IconButton
        ref={triggerRef}
        icon={icon}
        label={label}
        size={size}
        variant="ghost"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      />
      {open &&
        createPortal(
          <div
            ref={menuRef}
            className="menu"
            role="menu"
            style={{
              top: position?.top ?? -9999,
              left: position?.left ?? -9999,
              /* Hidden for the first frame, before useLayoutEffect has measured
                 it — otherwise it flashes at the top-left corner. */
              visibility: position ? "visible" : "hidden",
            }}
          >
            {items.map((item) => (
              <MenuButton key={item.key} item={item} onDone={() => setOpen(false)} />
            ))}
          </div>,
          document.body,
        )}
    </>
  );
}

function MenuButton({ item, onDone }: { item: MenuItem; onDone: () => void }) {
  return (
    <>
      {item.separatorBefore && <div className="menu-separator" role="separator" />}
      <button
        type="button"
        role="menuitem"
        className={`menu-item${item.danger ? " menu-item-danger" : ""}`}
        disabled={item.disabled}
        onClick={() => {
          item.onClick();
          onDone();
        }}
      >
        {item.icon && <Icon icon={item.icon} size="sm" />}
        {item.label}
      </button>
    </>
  );
}

export interface ActionGroupProps {
  children: ReactNode;
}

/** Right-aligned cluster for a table's action cell. */
export function ActionGroup({ children }: ActionGroupProps) {
  return <div className="action-group">{children}</div>;
}
