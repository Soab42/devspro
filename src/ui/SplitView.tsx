import { ReactNode } from "react";
import "./SplitView.css";

export interface SplitViewProps {
  /** The narrow column: places, bookmarks, a tree. */
  aside: ReactNode;
  children: ReactNode;
  /** Logical width of the aside in px. Default 200. */
  asideWidth?: 180 | 200 | 240;
  /** Accessible name for the aside landmark. */
  asideLabel: string;
  className?: string;
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
export function SplitView({ aside, children, asideWidth = 200, asideLabel, className }: SplitViewProps) {
  const cls = ["split-view", `split-view-aside-${asideWidth}`, className].filter(Boolean).join(" ");
  return (
    <div className={cls}>
      <aside className="split-view-aside" aria-label={asideLabel}>
        {aside}
      </aside>
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
