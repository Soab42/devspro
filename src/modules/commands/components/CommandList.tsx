import { ReactNode, useEffect, useState } from "react";
import { Badge } from "../../../ui";
import { Icon } from "../../../ui/Icon";
import { ChevronDown, ChevronRight, TriangleAlert } from "../../../ui/icons";
import type { Section } from "../library";
import type { LibraryCommand } from "../types";
import "./Commands.css";

export interface CommandListProps {
  sections: Section[];
  /** While searching every category opens, so the matches are in view. */
  searching: boolean;
  /** Narrow, for the terminal's side panel: no descriptions, actions on hover. */
  compact?: boolean;
  /** Remembers which categories are open, per place the list is shown. */
  storageKey: string;
  /** Clicking a row: type it into the terminal, or copy it. */
  onPick: (command: LibraryCommand) => void;
  pickLabel: string;
  actions: (command: LibraryCommand) => ReactNode;
}

function readOpen(key: string): Set<string> {
  try {
    const raw = localStorage.getItem(key);
    return new Set(raw ? (JSON.parse(raw) as string[]) : []);
  } catch {
    return new Set();
  }
}

/**
 * The library by section and category. Categories start folded: with several
 * hundred commands, the category names are the overview, and opening one is
 * cheaper than scrolling past the rest.
 */
export function CommandList({ sections, searching, compact = false, storageKey, onPick, pickLabel, actions }: CommandListProps) {
  const [open, setOpen] = useState<Set<string>>(() => readOpen(storageKey));
  useEffect(() => {
    try {
      localStorage.setItem(storageKey, JSON.stringify([...open]));
    } catch {
      /* private mode */
    }
  }, [open, storageKey]);

  function toggle(key: string) {
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  return (
    <div className={`cmd-list${compact ? " cmd-list-compact" : ""}`}>
      {sections.map((section) => (
        <section key={section.name} className="cmd-section" aria-label={section.name}>
          <h3 className="cmd-section-title">{section.name}</h3>
          {section.categories.map((category) => {
            const expanded = searching || open.has(category.key);
            return (
              <div key={category.key} className="cmd-category">
                <button
                  type="button"
                  className="cmd-category-head"
                  aria-expanded={expanded}
                  onClick={() => toggle(category.key)}
                  disabled={searching}
                >
                  <Icon icon={expanded ? ChevronDown : ChevronRight} size="xs" />
                  <span className="cmd-category-name">{category.name}</span>
                  <span className="cmd-category-count">{category.commands.length}</span>
                </button>
                {expanded && (
                  <ul className="cmd-rows">
                    {category.commands.map((command) => (
                      <li key={command.id} className="cmd-row">
                        <button
                          type="button"
                          className="cmd-row-main"
                          title={`${pickLabel}: ${command.command}`}
                          onClick={() => onPick(command)}
                        >
                          <span className="cmd-row-title">
                            {command.danger && (
                              <span className="cmd-row-danger" title="Destructive: asks before running">
                                <Icon icon={TriangleAlert} size="xs" />
                              </span>
                            )}
                            {command.title}
                          </span>
                          <code className="cmd-row-command">{command.command}</code>
                          {!compact && command.description && (
                            <span className="cmd-row-description">{command.description}</span>
                          )}
                        </button>
                        <span className="cmd-row-actions">
                          {!compact && command.scope === "personal" && <Badge tone="accent">personal</Badge>}
                          {actions(command)}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            );
          })}
        </section>
      ))}
    </div>
  );
}
