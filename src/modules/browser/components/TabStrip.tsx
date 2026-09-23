import { MouseEvent, useEffect, useRef, useState } from "react";
import { Icon } from "../../../ui";
import { Columns2, Globe, PictureInPicture2, Plus, RotateCw, Star, X, Copy } from "../../../ui/icons";
import type { Tab } from "../store";
import { tabLabel } from "../utils";

export type TabAction = "duplicate" | "popout" | "split" | "bookmark" | "close" | "close-others";

interface Props {
  tabs: Tab[];
  activeId: string | null;
  splitId: string | null;
  onActivate: (id: string) => void;
  onClose: (id: string) => void;
  onNew: () => void;
  onAction: (id: string, action: TabAction) => void;
  /** True while the context menu is showing. */
  onOverlay: (open: boolean) => void;
}

interface MenuState {
  id: string;
  x: number;
  y: number;
}

const ACTIONS: { key: TabAction; label: string; icon: typeof Plus }[] = [
  { key: "popout", label: "Pop out into a window", icon: PictureInPicture2 },
  { key: "split", label: "Show in split view", icon: Columns2 },
  { key: "duplicate", label: "Duplicate", icon: Copy },
  { key: "bookmark", label: "Bookmark", icon: Star },
  { key: "close-others", label: "Close other tabs", icon: X },
  { key: "close", label: "Close", icon: X },
];

/**
 * The tab strip. Right-click (or the ⋯ that appears on hover) opens the
 * per-tab actions; middle-click closes, like every browser.
 */
export function TabStrip({ tabs, activeId, splitId, onActivate, onClose, onNew, onAction, onOverlay }: Props) {
  const [menu, setMenu] = useState<MenuState | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    onOverlay(menu !== null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const close = (e: Event) => {
      if (menuRef.current?.contains(e.target as Node)) return;
      setMenu(null);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setMenu(null);
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", onKey);
    window.addEventListener("resize", () => setMenu(null), { once: true });
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", onKey);
    };
  }, [menu]);

  function openMenu(e: MouseEvent, id: string) {
    e.preventDefault();
    const x = Math.min(e.clientX, window.innerWidth - 220);
    setMenu({ id, x, y: e.clientY });
  }

  function onMouseUp(e: MouseEvent, id: string) {
    if (e.button === 1) {
      e.preventDefault();
      onClose(id);
    }
  }

  return (
    <div className="browser-tabs" role="tablist" aria-label="Open tabs">
      {tabs.map((t) => {
        const label = tabLabel(t.title, t.url);
        const cls = ["browser-tab", t.id === activeId && "browser-tab-active", t.id === splitId && "browser-tab-split"]
          .filter(Boolean)
          .join(" ");
        return (
          <div key={t.id} className={cls} onContextMenu={(e) => openMenu(e, t.id)} onMouseUp={(e) => onMouseUp(e, t.id)}>
            <button
              type="button"
              role="tab"
              aria-selected={t.id === activeId}
              className="browser-tab-button"
              onClick={() => onActivate(t.id)}
              title={t.url}
            >
              <span className="browser-tab-favicon">
                <Icon icon={t.loading ? RotateCw : t.id === splitId ? Columns2 : Globe} size="xs" className={t.loading ? "browser-spin" : undefined} />
              </span>
              <span className="browser-tab-label">{label}</span>
            </button>
            <button type="button" className="browser-tab-close" aria-label={`Close ${label}`} onClick={() => onClose(t.id)}>
              <Icon icon={X} size="xs" />
            </button>
          </div>
        );
      })}
      <button type="button" className="browser-tab-new" aria-label="New tab" title="New tab (Ctrl+T)" onClick={onNew}>
        <Icon icon={Plus} size="sm" />
      </button>

      {menu && (
        <div ref={menuRef} className="browser-context" role="menu" style={{ top: menu.y, left: menu.x }}>
          {ACTIONS.map((a) => (
            <button
              key={a.key}
              type="button"
              role="menuitem"
              className={a.key === "close" || a.key === "close-others" ? "browser-context-item browser-context-danger" : "browser-context-item"}
              disabled={a.key === "close-others" && tabs.length < 2}
              onClick={() => {
                setMenu(null);
                onAction(menu.id, a.key);
              }}
            >
              <Icon icon={a.icon} size="xs" />
              {a.key === "split" && menu.id === splitId ? "Leave split view" : a.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
