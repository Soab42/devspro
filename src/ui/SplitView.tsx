import { HTMLAttributes, PointerEvent as ReactPointerEvent, ReactNode, useState } from "react";
import "./SplitView.css";

export interface SplitViewProps {
  /** The narrow column: places, bookmarks, a tree. */
  aside: ReactNode;
  children: ReactNode;
  /** Logical width of the aside in px. Default 200. */
  asideWidth?: 180 | 200 | 240;
  /** Accessible name for the aside landmark. */
  asideLabel: string;
  /** Adds a drag handle on the aside's edge. The width is remembered under
      `storageKey` when one is given, so it survives a reload. */
  resizable?: boolean;
  storageKey?: string;
  className?: string;
}

const MIN_ASIDE = 140;
const MAX_ASIDE = 480;

function storedWidth(key: string | undefined): number | null {
  if (!key) return null;
  try {
    const raw = window.localStorage.getItem(key);
    const n = raw ? Number(raw) : NaN;
    return Number.isFinite(n) ? Math.max(MIN_ASIDE, Math.min(MAX_ASIDE, n)) : null;
  } catch {
    return null;
  }
}

/**
 * A fixed aside beside a flexible main area, both scrolling independently.
 *
 * The app sidebar is bespoke (Shell.css) because it collapses to a rail and
 * becomes a drawer; module pages with their own secondary navigation (a file
 * manager's places, a browser's bookmarks) need none of that, only the two
 * columns. Below tablet width the aside stacks above the content as a row of
 * chips, so a phone-width window still reaches every place.
 */
export function SplitView({ aside, children, asideWidth = 200, asideLabel, resizable = false, storageKey, className }: SplitViewProps) {
  const [width, setWidth] = useState<number | null>(() => storedWidth(storageKey));
  const cls = ["split-view", `split-view-aside-${asideWidth}`, resizable && "split-view-resizable", className].filter(Boolean).join(" ");

  function startResize(e: ReactPointerEvent<HTMLDivElement>) {
    e.preventDefault();
    const handle = e.currentTarget;
    const startX = e.clientX;
    const startWidth = width ?? asideWidth;
    handle.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => setWidth(Math.max(MIN_ASIDE, Math.min(MAX_ASIDE, startWidth + ev.clientX - startX)));
    const stop = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", stop);
      handle.removeEventListener("pointercancel", stop);
      setWidth((w) => {
        if (storageKey && w !== null) {
          try {
            window.localStorage.setItem(storageKey, String(w));
          } catch {
            /* private mode or blocked storage: the width just does not persist */
          }
        }
        return w;
      });
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", stop);
    handle.addEventListener("pointercancel", stop);
  }

  return (
    <div className={cls}>
      <aside className="split-view-aside" aria-label={asideLabel} style={resizable && width !== null ? { width } : undefined}>
        {aside}
      </aside>
      {resizable && (
        <div className="split-view-resizer" role="separator" aria-orientation="vertical" aria-label="Resize sidebar" onPointerDown={startResize} />
      )}
      <div className="split-view-main">{children}</div>
    </div>
  );
}

export interface SideListItem {
  key: string;
  label: string;
  icon?: ReactNode;
  /** Right-aligned secondary text — a count, a size. Truncated before the label. */
  meta?: string;
  active?: boolean;
  onSelect: () => void;
  /** Extra attributes on the row's button — drag-and-drop handlers, data-* hooks. */
  attrs?: HTMLAttributes<HTMLButtonElement> & Record<`data-${string}`, string | undefined>;
}

export interface SideListProps {
  /** Small uppercase heading above the group. */
  title?: string;
  items: SideListItem[];
}

/** A group of selectable rows for a SplitView aside. */
export function SideList({ title, items }: SideListProps) {
  return (
    <div className="side-list">
      {title && <div className="side-list-title">{title}</div>}
      <ul className="side-list-items">
        {items.map((item) => (
          <li key={item.key}>
            <button
              type="button"
              className={item.active ? "side-list-item side-list-item-active" : "side-list-item"}
              aria-current={item.active ? "true" : undefined}
              onClick={item.onSelect}
              {...item.attrs}
            >
              {item.icon && <span className="side-list-item-icon">{item.icon}</span>}
              <span className="side-list-item-label">{item.label}</span>
              {item.meta && <span className="side-list-item-meta">{item.meta}</span>}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
