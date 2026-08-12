import type { LucideIcon } from "./icons";
import { Icon } from "./Icon";
import "./Tabs.css";

export interface TabDefinition {
  id: string;
  label: string;
  icon?: LucideIcon;
}

export interface TabsProps {
  tabs: TabDefinition[];
  active: string;
  onChange: (id: string) => void;
  ariaLabel?: string;
}

/**
 * Switches between genuinely different views of a module.
 *
 * If the choice merely narrows one view, that's a filter — use
 * <SegmentedControl> instead. Categories on the Apps page used to be Tabs for
 * this reason and were migrated.
 */
export function Tabs({ tabs, active, onChange, ariaLabel }: TabsProps) {
  return (
    <div className="tabs" role="tablist" aria-label={ariaLabel}>
      {tabs.map((tab) => (
        <button
          key={tab.id}
          role="tab"
          type="button"
          aria-selected={tab.id === active}
          tabIndex={tab.id === active ? 0 : -1}
          className={`tab-button${tab.id === active ? " tab-button-active" : ""}`}
          onClick={() => onChange(tab.id)}
        >
          {tab.icon && <Icon icon={tab.icon} size="sm" />}
          {tab.label}
        </button>
      ))}
    </div>
  );
}
