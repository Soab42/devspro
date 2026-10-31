import type { ReactNode } from "react";
import type { LucideIcon } from "./icons";
import { Icon } from "./Icon";
import "./ChoiceList.css";

export interface ChoiceItem {
  key: string;
  icon?: LucideIcon;
  title: ReactNode;
  /** A second, quieter line: an address, a count, a path. */
  detail?: ReactNode;
  /** At the row's end: badges, a hint. */
  trailing?: ReactNode;
  onSelect: () => void;
}

/**
 * A list of things to pick one of, each a full-width row that acts on click —
 * the body of a "pick a host" or "pick a target" dialog. Rows are real
 * buttons, so Tab and Enter work without a roving focus.
 */
export function ChoiceList({ items, ariaLabel }: { items: ChoiceItem[]; ariaLabel: string }) {
  return (
    <ul className="choice-list" aria-label={ariaLabel}>
      {items.map((item) => (
        <li key={item.key}>
          <button type="button" className="choice-list-row" onClick={item.onSelect}>
            {item.icon && <Icon icon={item.icon} size="sm" />}
            <span className="choice-list-text">
              <span className="choice-list-title">{item.title}</span>
              {item.detail && <span className="choice-list-detail">{item.detail}</span>}
            </span>
            {item.trailing}
          </button>
        </li>
      ))}
    </ul>
  );
}
